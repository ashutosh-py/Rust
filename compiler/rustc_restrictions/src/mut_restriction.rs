use rustc_hir::def::Res;
use rustc_hir::def_id::{DefId, LocalDefId};
use rustc_hir::Node;
use rustc_middle::mir::visit::{PlaceContext, Visitor};
use rustc_middle::mir::{AggregateKind, Rvalue};
use rustc_middle::mir::{Body, Location, Place, ProjectionElem, Statement, Terminator};
use rustc_middle::query::Providers;
use rustc_middle::span_bug;
use rustc_middle::ty::{MutRestriction, Restriction, TyCtxt};
use rustc_span::Span;

use crate::errors;

pub(crate) fn provide(providers: &mut Providers) {
    *providers = Providers {
        mut_restriction,
        check_mut_restriction,
        adt_expression_restriction,
        ..*providers
    };
}

fn mut_restriction(tcx: TyCtxt<'_>, def_id: LocalDefId) -> MutRestriction {
    tracing::debug!("mut_restriction({def_id:?})");

    match tcx.resolutions(()).mut_restrictions.get(&def_id.to_def_id()) {
        Some(restriction) => *restriction,
        None => span_bug!(tcx.def_span(def_id), "mut restriction not found for {def_id:?}"),
    }
}

fn check_mut_restriction(tcx: TyCtxt<'_>, def_id: LocalDefId) {
    tracing::debug!("check_mut_restriction({def_id:?})");

    let hir_id = tcx.hir().local_def_id_to_hir_id(def_id);

    // FIXME(jhpratt) Does this need to be handled somehow?
    if matches!(tcx.hir().get(hir_id), Node::AnonConst(_)) {
        return;
    }

    let body = &tcx.mir_built(def_id).borrow();
    let mut checker = MutRestrictionChecker { tcx, body, span: body.span };
    checker.visit_body(body);
}

/// Obtain the restriction on ADT expressions. This occurs when an ADT field has its mutability
/// restricted.
// This is a query to allow the compiler to cache the output. This avoids the need to recompute the
// same information for every ADT expression.
fn adt_expression_restriction(tcx: TyCtxt<'_>, variant_def_id: DefId) -> MutRestriction {
    let res = Res::Def(tcx.def_kind(variant_def_id), variant_def_id);
    let variant = tcx.expect_variant_res(res);

    Restriction::strictest_of(
        variant.fields.iter().map(|field| tcx.mut_restriction(field.did)),
        tcx,
    )
}

struct MutRestrictionChecker<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    body: &'a Body<'tcx>,
    span: Span,
}

impl<'tcx> Visitor<'tcx> for MutRestrictionChecker<'_, 'tcx> {
    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, location: Location) {
        self.span = terminator.source_info.span;
        self.super_terminator(terminator, location);
    }

    fn visit_statement(&mut self, statement: &Statement<'tcx>, location: Location) {
        self.span = statement.source_info.span;
        self.super_statement(statement, location);
    }

    fn visit_place(&mut self, place: &Place<'tcx>, context: PlaceContext, location: Location) {
        if context.is_mutating_use() {
            let body_did = self.body.source.instance.def_id();

            for (place_base, elem) in place.iter_projections() {
                match elem {
                    ProjectionElem::Field(field, _ty) => {
                        let field_ty = place_base.ty(self.body, self.tcx);
                        if !field_ty.ty.is_adt() {
                            continue;
                        }
                        let field_def = field_ty.field_def(field);
                        let field_mut_restriction = self.tcx.mut_restriction(field_def.did);

                        if !field_mut_restriction.is_allowed_in(body_did, self.tcx) {
                            self.tcx.sess.emit_err(errors::MutOfRestrictedField {
                                mut_span: self.span,
                                restriction_span: field_mut_restriction.span(),
                                restriction_path: field_mut_restriction
                                    .restriction_path(self.tcx, body_did.krate),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        self.super_place(place, context, location)
    }

    fn visit_rvalue(&mut self, rvalue: &Rvalue<'tcx>, location: Location) {
        if let Rvalue::Aggregate(box AggregateKind::Adt(def_id, variant_idx, _, _, _), _) = rvalue {
            let adt_def = self.tcx.type_of(def_id).skip_binder().ty_adt_def().unwrap();
            let variant = adt_def.variant(*variant_idx);

            let construction_restriction = self.tcx.adt_expression_restriction(variant.def_id);

            let body_did = self.body.source.instance.def_id();
            if !construction_restriction.is_allowed_in(body_did, self.tcx) {
                self.tcx.sess.emit_err(errors::ConstructionOfTyWithMutRestrictedField {
                    construction_span: self.span,
                    restriction_span: construction_restriction.span(),
                    restriction_path: construction_restriction
                        .restriction_path(self.tcx, body_did.krate),
                    note: (),
                    article: "a",
                    description: adt_def.variant_descr(),
                    name: variant.name.to_ident_string(),
                });
            }
        }

        self.super_rvalue(rvalue, location);
    }
}
