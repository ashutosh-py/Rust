use super::ItemCtxt;
use crate::astconv::{AstConv, PredicateFilter};
use rustc_data_structures::fx::FxIndexMap;
use rustc_hir as hir;
use rustc_infer::traits::util;
use rustc_middle::ty::fold::shift_vars;
use rustc_middle::ty::{self, Ty, TyCtxt, TypeFoldable, TypeFolder, TypeVisitableExt};
use rustc_middle::ty::{GenericArgs, ToPredicate, TypeSuperFoldable};
use rustc_span::def_id::{DefId, LocalDefId};
use rustc_span::Span;

/// For associated types we include both bounds written on the type
/// (`type X: Trait`) and predicates from the trait: `where Self::X: Trait`.
///
/// Note that this filtering is done with the items identity args to
/// simplify checking that these bounds are met in impls. This means that
/// a bound such as `for<'b> <Self as X<'b>>::U: Clone` can't be used, as in
/// `hr-associated-type-bound-1.rs`.
fn associated_type_bounds<'tcx>(
    tcx: TyCtxt<'tcx>,
    assoc_item_def_id: LocalDefId,
    ast_bounds: &'tcx [hir::GenericBound<'tcx>],
    span: Span,
) -> &'tcx [(ty::Clause<'tcx>, Span)] {
    let item_ty = Ty::new_projection(
        tcx,
        assoc_item_def_id.to_def_id(),
        GenericArgs::identity_for_item(tcx, assoc_item_def_id),
    );

    let icx = ItemCtxt::new(tcx, assoc_item_def_id);
    let mut bounds = icx.astconv().compute_bounds(item_ty, ast_bounds, PredicateFilter::All);
    // Associated types are implicitly sized unless a `?Sized` bound is found
    icx.astconv().add_implicitly_sized(&mut bounds, item_ty, ast_bounds, None, span);

    let trait_def_id = tcx.local_parent(assoc_item_def_id);
    let trait_predicates = tcx.trait_explicit_predicates_and_bounds(trait_def_id);

    let item_trait_ref = ty::TraitRef::identity(tcx, tcx.parent(assoc_item_def_id.to_def_id()));
    let bounds_from_parent =
        trait_predicates.predicates.iter().copied().filter_map(|(pred, span)| {
            let mut clause_ty = match pred.kind().skip_binder() {
                ty::ClauseKind::Trait(tr) => tr.self_ty(),
                ty::ClauseKind::Projection(proj) => proj.projection_ty.self_ty(),
                ty::ClauseKind::TypeOutlives(outlives) => outlives.0,
                _ => return None,
            };

            // The code below is quite involved, so let me explain.
            //
            // We loop here, because we also want to collect vars for nested associated items as
            // well. For example, given a clause like `Self::A::B`, we want to add that to the
            // item bounds for `A`, so that we may use that bound in the case that `Self::A::B` is
            // rigid.
            //
            // Secondly, regarding bound vars, when we see a where clause that mentions a GAT
            // like `for<'a, ...> Self::Assoc<'a, ...>: Bound<'b, ...>`, we want to turn that into
            // an item bound on the GAT, where all of the GAT args are substituted with the GAT's
            // param regions, and then keep all of the other late-bound vars in the bound around.
            // We need to "compress" the binder so that it doesn't mention any of those vars that
            // were mapped to params.
            let gat_vars = loop {
                if let ty::Alias(ty::Projection, alias_ty) = *clause_ty.kind() {
                    if alias_ty.trait_ref(tcx) == item_trait_ref {
                        break &alias_ty.args[item_trait_ref.args.len()..];
                    } else {
                        clause_ty = alias_ty.self_ty();
                        continue;
                    }
                }

                return None;
            };
            // Special-case: No GAT vars, no mapping needed.
            if gat_vars.is_empty() {
                return Some((pred, span));
            }

            // First, check that all of the GAT args are substituted with a unique late-bound arg.
            // If we find a duplicate, then it can't be mapped to the definition's params.
            let mut mapping = FxIndexMap::default();
            let generics = tcx.generics_of(assoc_item_def_id);
            for (param, var) in std::iter::zip(&generics.params, gat_vars) {
                let existing = match var.unpack() {
                    ty::GenericArgKind::Lifetime(re) => {
                        if let ty::RegionKind::ReBound(ty::INNERMOST, bv) = re.kind() {
                            mapping.insert(bv.var, tcx.mk_param_from_def(param))
                        } else {
                            return None;
                        }
                    }
                    ty::GenericArgKind::Type(ty) => {
                        if let ty::Bound(ty::INNERMOST, bv) = *ty.kind() {
                            mapping.insert(bv.var, tcx.mk_param_from_def(param))
                        } else {
                            return None;
                        }
                    }
                    ty::GenericArgKind::Const(ct) => {
                        if let ty::ConstKind::Bound(ty::INNERMOST, bv) = ct.kind() {
                            mapping.insert(bv, tcx.mk_param_from_def(param))
                        } else {
                            return None;
                        }
                    }
                };

                if existing.is_some() {
                    return None;
                }
            }

            // Finally, map all of the args in the GAT to the params we expect, and compress
            // the remaining late-bound vars so that they count up from var 0.
            let mut folder = MapAndCompressBoundVars {
                tcx,
                binder: ty::INNERMOST,
                still_bound_vars: vec![],
                mapping,
            };
            let pred = pred.kind().skip_binder().fold_with(&mut folder);

            Some((
                ty::Binder::bind_with_vars(
                    pred,
                    tcx.mk_bound_variable_kinds(&folder.still_bound_vars),
                )
                .to_predicate(tcx),
                span,
            ))
        });

    let all_bounds = tcx.arena.alloc_from_iter(bounds.clauses().chain(bounds_from_parent));
    debug!(
        "associated_type_bounds({}) = {:?}",
        tcx.def_path_str(assoc_item_def_id.to_def_id()),
        all_bounds
    );
    all_bounds
}

struct MapAndCompressBoundVars<'tcx> {
    tcx: TyCtxt<'tcx>,
    /// How deep are we? Makes sure we don't touch the vars of nested binders.
    binder: ty::DebruijnIndex,
    /// List of bound vars that remain unsubstituted because they were not
    /// mentioned in the GAT's args.
    still_bound_vars: Vec<ty::BoundVariableKind>,
    /// Subtle invariant: If the `GenericArg` is bound, then it should be
    /// stored with the debruijn index of `INNERMOST` so it can be shifted
    /// correctly during substitution.
    mapping: FxIndexMap<ty::BoundVar, ty::GenericArg<'tcx>>,
}

impl<'tcx> TypeFolder<TyCtxt<'tcx>> for MapAndCompressBoundVars<'tcx> {
    fn interner(&self) -> TyCtxt<'tcx> {
        self.tcx
    }

    fn fold_binder<T>(&mut self, t: ty::Binder<'tcx, T>) -> ty::Binder<'tcx, T>
    where
        ty::Binder<'tcx, T>: TypeSuperFoldable<TyCtxt<'tcx>>,
    {
        self.binder.shift_in(1);
        let out = t.super_fold_with(self);
        self.binder.shift_out(1);
        out
    }

    fn fold_ty(&mut self, ty: Ty<'tcx>) -> Ty<'tcx> {
        if !ty.has_bound_vars() {
            return ty;
        }

        if let ty::Bound(binder, old_bound) = *ty.kind()
            && self.binder == binder
        {
            let mapped = if let Some(mapped) = self.mapping.get(&old_bound.var) {
                mapped.expect_ty()
            } else {
                // If we didn't find a mapped generic, then make a new one.
                // Allocate a new var idx, and insert a new bound ty.
                let var = ty::BoundVar::from_usize(self.still_bound_vars.len());
                self.still_bound_vars.push(ty::BoundVariableKind::Ty(old_bound.kind));
                let mapped = Ty::new_bound(
                    self.tcx,
                    ty::INNERMOST,
                    ty::BoundTy { var, kind: old_bound.kind },
                );
                self.mapping.insert(old_bound.var, mapped.into());
                mapped
            };

            shift_vars(self.tcx, mapped, self.binder.as_u32())
        } else {
            ty.super_fold_with(self)
        }
    }

    fn fold_region(&mut self, re: ty::Region<'tcx>) -> ty::Region<'tcx> {
        if let ty::ReBound(binder, old_bound) = re.kind()
            && self.binder == binder
        {
            let mapped = if let Some(mapped) = self.mapping.get(&old_bound.var) {
                mapped.expect_region()
            } else {
                let var = ty::BoundVar::from_usize(self.still_bound_vars.len());
                self.still_bound_vars.push(ty::BoundVariableKind::Region(old_bound.kind));
                let mapped = ty::Region::new_bound(
                    self.tcx,
                    ty::INNERMOST,
                    ty::BoundRegion { var, kind: old_bound.kind },
                );
                self.mapping.insert(old_bound.var, mapped.into());
                mapped
            };

            shift_vars(self.tcx, mapped, self.binder.as_u32())
        } else {
            re
        }
    }

    fn fold_const(&mut self, ct: ty::Const<'tcx>) -> ty::Const<'tcx> {
        if !ct.has_bound_vars() {
            return ct;
        }

        if let ty::ConstKind::Bound(binder, old_var) = ct.kind()
            && self.binder == binder
        {
            let mapped = if let Some(mapped) = self.mapping.get(&old_var) {
                mapped.expect_const()
            } else {
                let var = ty::BoundVar::from_usize(self.still_bound_vars.len());
                self.still_bound_vars.push(ty::BoundVariableKind::Const);
                let mapped = ty::Const::new_bound(self.tcx, ty::INNERMOST, var, ct.ty());
                self.mapping.insert(old_var, mapped.into());
                mapped
            };

            shift_vars(self.tcx, mapped, self.binder.as_u32())
        } else {
            ct.super_fold_with(self)
        }
    }

    fn fold_predicate(&mut self, p: ty::Predicate<'tcx>) -> ty::Predicate<'tcx> {
        if !p.has_bound_vars() { p } else { p.super_fold_with(self) }
    }
}

/// Opaque types don't inherit bounds from their parent: for return position
/// impl trait it isn't possible to write a suitable predicate on the
/// containing function and for type-alias impl trait we don't have a backwards
/// compatibility issue.
#[instrument(level = "trace", skip(tcx), ret)]
fn opaque_type_bounds<'tcx>(
    tcx: TyCtxt<'tcx>,
    opaque_def_id: LocalDefId,
    ast_bounds: &'tcx [hir::GenericBound<'tcx>],
    item_ty: Ty<'tcx>,
    span: Span,
) -> &'tcx [(ty::Clause<'tcx>, Span)] {
    ty::print::with_reduced_queries!({
        let icx = ItemCtxt::new(tcx, opaque_def_id);
        let mut bounds = icx.astconv().compute_bounds(item_ty, ast_bounds, PredicateFilter::All);
        // Opaque types are implicitly sized unless a `?Sized` bound is found
        icx.astconv().add_implicitly_sized(&mut bounds, item_ty, ast_bounds, None, span);
        debug!(?bounds);

        tcx.arena.alloc_from_iter(bounds.clauses())
    })
}

pub(super) fn explicit_item_bounds(
    tcx: TyCtxt<'_>,
    def_id: LocalDefId,
) -> ty::EarlyBinder<&'_ [(ty::Clause<'_>, Span)]> {
    match tcx.opt_rpitit_info(def_id.to_def_id()) {
        // RPITIT's bounds are the same as opaque type bounds, but with
        // a projection self type.
        Some(ty::ImplTraitInTraitData::Trait { opaque_def_id, .. }) => {
            let item = tcx.hir_node_by_def_id(opaque_def_id.expect_local()).expect_item();
            let opaque_ty = item.expect_opaque_ty();
            return ty::EarlyBinder::bind(opaque_type_bounds(
                tcx,
                opaque_def_id.expect_local(),
                opaque_ty.bounds,
                Ty::new_projection(
                    tcx,
                    def_id.to_def_id(),
                    ty::GenericArgs::identity_for_item(tcx, def_id),
                ),
                item.span,
            ));
        }
        Some(ty::ImplTraitInTraitData::Impl { .. }) => span_bug!(
            tcx.def_span(def_id),
            "item bounds for RPITIT in impl to be fed on def-id creation"
        ),
        None => {}
    }

    let bounds = match tcx.hir_node_by_def_id(def_id) {
        hir::Node::TraitItem(hir::TraitItem {
            kind: hir::TraitItemKind::Type(bounds, _),
            span,
            ..
        }) => associated_type_bounds(tcx, def_id, bounds, *span),
        hir::Node::Item(hir::Item {
            kind: hir::ItemKind::OpaqueTy(hir::OpaqueTy { bounds, in_trait: false, .. }),
            span,
            ..
        }) => {
            let args = GenericArgs::identity_for_item(tcx, def_id);
            let item_ty = Ty::new_opaque(tcx, def_id.to_def_id(), args);
            opaque_type_bounds(tcx, def_id, bounds, item_ty, *span)
        }
        // Since RPITITs are astconv'd as projections in `ast_ty_to_ty`, when we're asking
        // for the item bounds of the *opaques* in a trait's default method signature, we
        // need to map these projections back to opaques.
        hir::Node::Item(hir::Item {
            kind: hir::ItemKind::OpaqueTy(hir::OpaqueTy { bounds, in_trait: true, origin, .. }),
            span,
            ..
        }) => {
            let (hir::OpaqueTyOrigin::FnReturn(fn_def_id)
            | hir::OpaqueTyOrigin::AsyncFn(fn_def_id)) = *origin
            else {
                span_bug!(*span, "RPITIT cannot be a TAIT, but got origin {origin:?}");
            };
            let args = GenericArgs::identity_for_item(tcx, def_id);
            let item_ty = Ty::new_opaque(tcx, def_id.to_def_id(), args);
            tcx.arena.alloc_slice(
                &opaque_type_bounds(tcx, def_id, bounds, item_ty, *span)
                    .to_vec()
                    .fold_with(&mut AssocTyToOpaque { tcx, fn_def_id: fn_def_id.to_def_id() }),
            )
        }
        hir::Node::Item(hir::Item { kind: hir::ItemKind::TyAlias(..), .. }) => &[],
        _ => bug!("item_bounds called on {:?}", def_id),
    };
    ty::EarlyBinder::bind(bounds)
}

pub(super) fn item_bounds(
    tcx: TyCtxt<'_>,
    def_id: DefId,
) -> ty::EarlyBinder<&'_ ty::List<ty::Clause<'_>>> {
    tcx.explicit_item_bounds(def_id).map_bound(|bounds| {
        tcx.mk_clauses_from_iter(util::elaborate(tcx, bounds.iter().map(|&(bound, _span)| bound)))
    })
}

struct AssocTyToOpaque<'tcx> {
    tcx: TyCtxt<'tcx>,
    fn_def_id: DefId,
}

impl<'tcx> TypeFolder<TyCtxt<'tcx>> for AssocTyToOpaque<'tcx> {
    fn interner(&self) -> TyCtxt<'tcx> {
        self.tcx
    }

    fn fold_ty(&mut self, ty: Ty<'tcx>) -> Ty<'tcx> {
        if let ty::Alias(ty::Projection, projection_ty) = ty.kind()
            && let Some(ty::ImplTraitInTraitData::Trait { fn_def_id, .. }) =
                self.tcx.opt_rpitit_info(projection_ty.def_id)
            && fn_def_id == self.fn_def_id
        {
            self.tcx.type_of(projection_ty.def_id).instantiate(self.tcx, projection_ty.args)
        } else {
            ty
        }
    }
}
