//! Generalized type relating mechanism.
//!
//! A type relation `R` relates a pair of values `(A, B)`. `A and B` are usually
//! types or regions but can be other things. Examples of type relations are
//! subtyping, type equality, etc.

use crate::ty::error::{ExpectedFound, TypeError};
use crate::ty::{self, Expr, ImplSubject, Term, TermKind, Ty, TyCtxt, TypeFoldable};
use crate::ty::{GenericArg, GenericArgKind, GenericArgsRef};
use rustc_hir as hir;
use rustc_hir::def_id::DefId;
use rustc_target::spec::abi;
use std::iter;

pub type RelateResult<'tcx, T> = Result<T, TypeError<'tcx>>;

pub type RelateResult2<'tcx> = Result<(), TypeError<'tcx>>;

pub trait TypeRelation<'tcx>: Sized {
    fn tcx(&self) -> TyCtxt<'tcx>;

    fn param_env(&self) -> ty::ParamEnv<'tcx>;

    /// Returns a static string we can use for printouts.
    fn tag(&self) -> &'static str;

    /// Returns `true` if the value `a` is the "expected" type in the
    /// relation. Just affects error messages.
    fn a_is_expected(&self) -> bool;

    /// Generic relation routine suitable for most anything.
    fn relate<T: Relate<'tcx>>(&mut self, a: T, b: T) -> RelateResult<'tcx, T> {
        Relate::relate(self, a, b)
    }

    /// Relate the two args for the given item. The default
    /// is to look up the variance for the item and proceed
    /// accordingly.
    fn relate_item_args(
        &mut self,
        item_def_id: DefId,
        a_arg: GenericArgsRef<'tcx>,
        b_arg: GenericArgsRef<'tcx>,
    ) -> RelateResult<'tcx, GenericArgsRef<'tcx>> {
        debug!(
            "relate_item_args(item_def_id={:?}, a_arg={:?}, b_arg={:?})",
            item_def_id, a_arg, b_arg
        );

        let tcx = self.tcx();
        let opt_variances = tcx.variances_of(item_def_id);
        relate_args_with_variances(self, item_def_id, opt_variances, a_arg, b_arg, true)
    }

    /// Switch variance for the purpose of relating `a` and `b`.
    fn relate_with_variance<T: Relate<'tcx>>(
        &mut self,
        variance: ty::Variance,
        info: ty::VarianceDiagInfo<'tcx>,
        a: T,
        b: T,
    ) -> RelateResult<'tcx, T>;

    // Overridable relations. You shouldn't typically call these
    // directly, instead call `relate()`, which in turn calls
    // these. This is both more uniform but also allows us to add
    // additional hooks for other types in the future if needed
    // without making older code, which called `relate`, obsolete.

    fn tys(&mut self, a: Ty<'tcx>, b: Ty<'tcx>) -> RelateResult<'tcx, Ty<'tcx>>;

    fn regions(
        &mut self,
        a: ty::Region<'tcx>,
        b: ty::Region<'tcx>,
    ) -> RelateResult<'tcx, ty::Region<'tcx>>;

    fn consts(
        &mut self,
        a: ty::Const<'tcx>,
        b: ty::Const<'tcx>,
    ) -> RelateResult<'tcx, ty::Const<'tcx>>;

    fn binders<T>(
        &mut self,
        a: ty::Binder<'tcx, T>,
        b: ty::Binder<'tcx, T>,
    ) -> RelateResult<'tcx, ty::Binder<'tcx, T>>
    where
        T: Relate<'tcx>;
}

pub trait TypeRelation2<'tcx>: Sized {
    fn tcx(&self) -> TyCtxt<'tcx>;

    fn param_env(&self) -> ty::ParamEnv<'tcx>;

    /// Returns a static string we can use for printouts.
    fn tag(&self) -> &'static str;

    /// Returns `true` if the value `a` is the "expected" type in the
    /// relation. Just affects error messages.
    fn a_is_expected(&self) -> bool;

    /// Generic relation routine suitable for most anything.
    fn relate<T: Relate2<'tcx>>(&mut self, a: T, b: T) -> RelateResult2<'tcx> {
        Relate2::relate2(self, a, b)
    }

    /// Relate the two args for the given item. The default
    /// is to look up the variance for the item and proceed
    /// accordingly.
    fn relate_item_args(
        &mut self,
        item_def_id: DefId,
        a_arg: GenericArgsRef<'tcx>,
        b_arg: GenericArgsRef<'tcx>,
    ) -> RelateResult2<'tcx> {
        debug!(
            "relate_item_args(item_def_id={:?}, a_arg={:?}, b_arg={:?})",
            item_def_id, a_arg, b_arg
        );

        let tcx = self.tcx();
        let opt_variances = tcx.variances_of(item_def_id);
        relate_args_with_variances2(self, item_def_id, opt_variances, a_arg, b_arg, true)
    }

    /// Switch variance for the purpose of relating `a` and `b`.
    fn relate_with_variance<T: Relate2<'tcx>>(
        &mut self,
        variance: ty::Variance,
        info: ty::VarianceDiagInfo<'tcx>,
        a: T,
        b: T,
    ) -> RelateResult2<'tcx>;

    // Overridable relations. You shouldn't typically call these
    // directly, instead call `relate()`, which in turn calls
    // these. This is both more uniform but also allows us to add
    // additional hooks for other types in the future if needed
    // without making older code, which called `relate`, obsolete.

    fn tys(&mut self, a: Ty<'tcx>, b: Ty<'tcx>) -> RelateResult2<'tcx>;

    fn regions(&mut self, a: ty::Region<'tcx>, b: ty::Region<'tcx>) -> RelateResult2<'tcx>;

    fn consts(&mut self, a: ty::Const<'tcx>, b: ty::Const<'tcx>) -> RelateResult2<'tcx>;

    fn binders<T>(&mut self, a: ty::Binder<'tcx, T>, b: ty::Binder<'tcx, T>) -> RelateResult2<'tcx>
    where
        T: Relate2<'tcx>;
}

pub trait Relate<'tcx>: Relate2<'tcx> + TypeFoldable<TyCtxt<'tcx>> + PartialEq + Copy {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: Self,
        b: Self,
    ) -> RelateResult<'tcx, Self>;
}

pub trait Relate2<'tcx>: TypeFoldable<TyCtxt<'tcx>> + PartialEq + Copy {
    fn relate2<R: TypeRelation2<'tcx>>(relation: &mut R, a: Self, b: Self) -> RelateResult2<'tcx>;
}

///////////////////////////////////////////////////////////////////////////
// Relate impls

pub fn relate_type_and_mut<'tcx, R: TypeRelation<'tcx>>(
    relation: &mut R,
    a: ty::TypeAndMut<'tcx>,
    b: ty::TypeAndMut<'tcx>,
    base_ty: Ty<'tcx>,
) -> RelateResult<'tcx, ty::TypeAndMut<'tcx>> {
    debug!("{}.mts({:?}, {:?})", relation.tag(), a, b);
    if a.mutbl != b.mutbl {
        Err(TypeError::Mutability)
    } else {
        let mutbl = a.mutbl;
        let (variance, info) = match mutbl {
            hir::Mutability::Not => (ty::Covariant, ty::VarianceDiagInfo::None),
            hir::Mutability::Mut => {
                (ty::Invariant, ty::VarianceDiagInfo::Invariant { ty: base_ty, param_index: 0 })
            }
        };
        let ty = relation.relate_with_variance(variance, info, a.ty, b.ty)?;
        Ok(ty::TypeAndMut { ty, mutbl })
    }
}

pub fn relate_type_and_mut2<'tcx, R: TypeRelation2<'tcx>>(
    relation: &mut R,
    a: ty::TypeAndMut<'tcx>,
    b: ty::TypeAndMut<'tcx>,
    base_ty: Ty<'tcx>,
) -> RelateResult2<'tcx> {
    debug!("{}.mts({:?}, {:?})", relation.tag(), a, b);
    if a.mutbl != b.mutbl {
        Err(TypeError::Mutability)
    } else {
        let mutbl = a.mutbl;
        let (variance, info) = match mutbl {
            hir::Mutability::Not => (ty::Covariant, ty::VarianceDiagInfo::None),
            hir::Mutability::Mut => {
                (ty::Invariant, ty::VarianceDiagInfo::Invariant { ty: base_ty, param_index: 0 })
            }
        };
        relation.relate_with_variance(variance, info, a.ty, b.ty)?;
        Ok(())
    }
}

pub fn relate_args_with_variances<'tcx, R: TypeRelation<'tcx>>(
    relation: &mut R,
    ty_def_id: DefId,
    variances: &[ty::Variance],
    a_arg: GenericArgsRef<'tcx>,
    b_arg: GenericArgsRef<'tcx>,
    fetch_ty_for_diag: bool,
) -> RelateResult<'tcx, GenericArgsRef<'tcx>> {
    let tcx = relation.tcx();

    let mut cached_ty = None;
    let params = iter::zip(a_arg, b_arg).enumerate().map(|(i, (a, b))| {
        let variance = variances[i];
        let variance_info = if variance == ty::Invariant && fetch_ty_for_diag {
            let ty =
                *cached_ty.get_or_insert_with(|| tcx.type_of(ty_def_id).instantiate(tcx, a_arg));
            ty::VarianceDiagInfo::Invariant { ty, param_index: i.try_into().unwrap() }
        } else {
            ty::VarianceDiagInfo::default()
        };
        relation.relate_with_variance(variance, variance_info, a, b)
    });

    tcx.mk_args_from_iter(params)
}

pub fn relate_args_with_variances2<'tcx, R: TypeRelation2<'tcx>>(
    relation: &mut R,
    ty_def_id: DefId,
    variances: &[ty::Variance],
    a_arg: GenericArgsRef<'tcx>,
    b_arg: GenericArgsRef<'tcx>,
    fetch_ty_for_diag: bool,
) -> RelateResult2<'tcx> {
    let tcx = relation.tcx();
    let mut cached_ty = None;

    for (i, (a, b)) in iter::zip(a_arg, b_arg).enumerate() {
        let variance = variances[i];
        let variance_info = if variance == ty::Invariant && fetch_ty_for_diag {
            let ty =
                *cached_ty.get_or_insert_with(|| tcx.type_of(ty_def_id).instantiate(tcx, a_arg));
            ty::VarianceDiagInfo::Invariant { ty, param_index: i.try_into().unwrap() }
        } else {
            ty::VarianceDiagInfo::default()
        };
        relation.relate_with_variance(variance, variance_info, a, b)?;
    }

    Ok(())
}

impl<'tcx> Relate<'tcx> for ty::FnSig<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ty::FnSig<'tcx>,
        b: ty::FnSig<'tcx>,
    ) -> RelateResult<'tcx, ty::FnSig<'tcx>> {
        let tcx = relation.tcx();

        if a.c_variadic != b.c_variadic {
            return Err(TypeError::VariadicMismatch(expected_found(
                relation,
                a.c_variadic,
                b.c_variadic,
            )));
        }
        let unsafety = relation.relate(a.unsafety, b.unsafety)?;
        let abi = relation.relate(a.abi, b.abi)?;

        if a.inputs().len() != b.inputs().len() {
            return Err(TypeError::ArgCount);
        }

        let inputs_and_output = iter::zip(a.inputs(), b.inputs())
            .map(|(&a, &b)| ((a, b), false))
            .chain(iter::once(((a.output(), b.output()), true)))
            .map(|((a, b), is_output)| {
                if is_output {
                    relation.relate(a, b)
                } else {
                    relation.relate_with_variance(
                        ty::Contravariant,
                        ty::VarianceDiagInfo::default(),
                        a,
                        b,
                    )
                }
            })
            .enumerate()
            .map(|(i, r)| match r {
                Err(TypeError::Sorts(exp_found) | TypeError::ArgumentSorts(exp_found, _)) => {
                    Err(TypeError::ArgumentSorts(exp_found, i))
                }
                Err(TypeError::Mutability | TypeError::ArgumentMutability(_)) => {
                    Err(TypeError::ArgumentMutability(i))
                }
                r => r,
            });
        Ok(ty::FnSig {
            inputs_and_output: tcx.mk_type_list_from_iter(inputs_and_output)?,
            c_variadic: a.c_variadic,
            unsafety,
            abi,
        })
    }
}

impl<'tcx> Relate2<'tcx> for ty::FnSig<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ty::FnSig<'tcx>,
        b: ty::FnSig<'tcx>,
    ) -> RelateResult2<'tcx> {
        if a.c_variadic != b.c_variadic {
            return Err(TypeError::VariadicMismatch(expected_found2(
                relation,
                a.c_variadic,
                b.c_variadic,
            )));
        }
        relation.relate(a.unsafety, b.unsafety)?;
        relation.relate(a.abi, b.abi)?;

        if a.inputs().len() != b.inputs().len() {
            return Err(TypeError::ArgCount);
        }

        let len = a.inputs_and_output.len();
        for (i, (a, b)) in iter::zip(a.inputs_and_output, b.inputs_and_output).enumerate() {
            let result = if i == len - 1 {
                relation.relate(a, b)
            } else {
                relation.relate_with_variance(
                    ty::Contravariant,
                    ty::VarianceDiagInfo::default(),
                    a,
                    b,
                )
            };

            result.map_err(|err| match err {
                TypeError::Sorts(exp_found) | TypeError::ArgumentSorts(exp_found, _) => {
                    TypeError::ArgumentSorts(exp_found, i)
                }
                TypeError::Mutability | TypeError::ArgumentMutability(_) => {
                    TypeError::ArgumentMutability(i)
                }
                e => e,
            })?;
        }

        Ok(())
    }
}

impl<'tcx> Relate<'tcx> for ty::BoundConstness {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ty::BoundConstness,
        b: ty::BoundConstness,
    ) -> RelateResult<'tcx, ty::BoundConstness> {
        if a != b {
            Err(TypeError::ConstnessMismatch(expected_found(relation, a, b)))
        } else {
            Ok(a)
        }
    }
}

impl<'tcx> Relate2<'tcx> for ty::BoundConstness {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ty::BoundConstness,
        b: ty::BoundConstness,
    ) -> RelateResult2<'tcx> {
        if a != b {
            Err(TypeError::ConstnessMismatch(expected_found2(relation, a, b)))
        } else {
            Ok(())
        }
    }
}

impl<'tcx> Relate<'tcx> for hir::Unsafety {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: hir::Unsafety,
        b: hir::Unsafety,
    ) -> RelateResult<'tcx, hir::Unsafety> {
        if a != b {
            Err(TypeError::UnsafetyMismatch(expected_found(relation, a, b)))
        } else {
            Ok(a)
        }
    }
}

impl<'tcx> Relate2<'tcx> for hir::Unsafety {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: hir::Unsafety,
        b: hir::Unsafety,
    ) -> RelateResult2<'tcx> {
        if a != b {
            Err(TypeError::UnsafetyMismatch(expected_found2(relation, a, b)))
        } else {
            Ok(())
        }
    }
}

impl<'tcx> Relate<'tcx> for abi::Abi {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: abi::Abi,
        b: abi::Abi,
    ) -> RelateResult<'tcx, abi::Abi> {
        if a == b { Ok(a) } else { Err(TypeError::AbiMismatch(expected_found(relation, a, b))) }
    }
}

impl<'tcx> Relate2<'tcx> for abi::Abi {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: abi::Abi,
        b: abi::Abi,
    ) -> RelateResult2<'tcx> {
        if a == b { Ok(()) } else { Err(TypeError::AbiMismatch(expected_found2(relation, a, b))) }
    }
}

impl<'tcx> Relate<'tcx> for ty::AliasTy<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ty::AliasTy<'tcx>,
        b: ty::AliasTy<'tcx>,
    ) -> RelateResult<'tcx, ty::AliasTy<'tcx>> {
        if a.def_id != b.def_id {
            Err(TypeError::ProjectionMismatched(expected_found(relation, a.def_id, b.def_id)))
        } else {
            let args = relation.relate(a.args, b.args)?;
            Ok(relation.tcx().mk_alias_ty(a.def_id, args))
        }
    }
}

impl<'tcx> Relate2<'tcx> for ty::AliasTy<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ty::AliasTy<'tcx>,
        b: ty::AliasTy<'tcx>,
    ) -> RelateResult2<'tcx> {
        if a.def_id != b.def_id {
            Err(TypeError::ProjectionMismatched(expected_found2(relation, a.def_id, b.def_id)))
        } else {
            relation.relate(a.args, b.args)?;
            Ok(())
        }
    }
}

impl<'tcx> Relate<'tcx> for ty::ExistentialProjection<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ty::ExistentialProjection<'tcx>,
        b: ty::ExistentialProjection<'tcx>,
    ) -> RelateResult<'tcx, ty::ExistentialProjection<'tcx>> {
        if a.def_id != b.def_id {
            Err(TypeError::ProjectionMismatched(expected_found(relation, a.def_id, b.def_id)))
        } else {
            let term = relation.relate_with_variance(
                ty::Invariant,
                ty::VarianceDiagInfo::default(),
                a.term,
                b.term,
            )?;
            let args = relation.relate_with_variance(
                ty::Invariant,
                ty::VarianceDiagInfo::default(),
                a.args,
                b.args,
            )?;
            Ok(ty::ExistentialProjection { def_id: a.def_id, args, term })
        }
    }
}

impl<'tcx> Relate2<'tcx> for ty::ExistentialProjection<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ty::ExistentialProjection<'tcx>,
        b: ty::ExistentialProjection<'tcx>,
    ) -> RelateResult2<'tcx> {
        if a.def_id != b.def_id {
            Err(TypeError::ProjectionMismatched(expected_found2(relation, a.def_id, b.def_id)))
        } else {
            relation.relate_with_variance(
                ty::Invariant,
                ty::VarianceDiagInfo::default(),
                a.term,
                b.term,
            )?;
            relation.relate_with_variance(
                ty::Invariant,
                ty::VarianceDiagInfo::default(),
                a.args,
                b.args,
            )?;
            Ok(())
        }
    }
}

impl<'tcx> Relate<'tcx> for ty::TraitRef<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ty::TraitRef<'tcx>,
        b: ty::TraitRef<'tcx>,
    ) -> RelateResult<'tcx, ty::TraitRef<'tcx>> {
        // Different traits cannot be related.
        if a.def_id != b.def_id {
            Err(TypeError::Traits(expected_found(relation, a.def_id, b.def_id)))
        } else {
            let args = relation.relate(a.args, b.args)?;
            Ok(ty::TraitRef::new(relation.tcx(), a.def_id, args))
        }
    }
}

impl<'tcx> Relate2<'tcx> for ty::TraitRef<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ty::TraitRef<'tcx>,
        b: ty::TraitRef<'tcx>,
    ) -> RelateResult2<'tcx> {
        // Different traits cannot be related.
        if a.def_id != b.def_id {
            Err(TypeError::Traits(expected_found2(relation, a.def_id, b.def_id)))
        } else {
            relation.relate(a.args, b.args)?;
            Ok(())
        }
    }
}

impl<'tcx> Relate<'tcx> for ty::ExistentialTraitRef<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ty::ExistentialTraitRef<'tcx>,
        b: ty::ExistentialTraitRef<'tcx>,
    ) -> RelateResult<'tcx, ty::ExistentialTraitRef<'tcx>> {
        // Different traits cannot be related.
        if a.def_id != b.def_id {
            Err(TypeError::Traits(expected_found(relation, a.def_id, b.def_id)))
        } else {
            let args = relation.relate(a.args, b.args)?;
            Ok(ty::ExistentialTraitRef { def_id: a.def_id, args })
        }
    }
}

impl<'tcx> Relate2<'tcx> for ty::ExistentialTraitRef<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ty::ExistentialTraitRef<'tcx>,
        b: ty::ExistentialTraitRef<'tcx>,
    ) -> RelateResult2<'tcx> {
        // Different traits cannot be related.
        if a.def_id != b.def_id {
            Err(TypeError::Traits(expected_found2(relation, a.def_id, b.def_id)))
        } else {
            relation.relate(a.args, b.args)?;
            Ok(())
        }
    }
}

#[derive(PartialEq, Copy, Debug, Clone, TypeFoldable, TypeVisitable)]
struct GeneratorWitness<'tcx>(&'tcx ty::List<Ty<'tcx>>);

impl<'tcx> Relate<'tcx> for GeneratorWitness<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: GeneratorWitness<'tcx>,
        b: GeneratorWitness<'tcx>,
    ) -> RelateResult<'tcx, GeneratorWitness<'tcx>> {
        assert_eq!(a.0.len(), b.0.len());
        let tcx = relation.tcx();
        let types =
            tcx.mk_type_list_from_iter(iter::zip(a.0, b.0).map(|(a, b)| relation.relate(a, b)))?;
        Ok(GeneratorWitness(types))
    }
}

impl<'tcx> Relate2<'tcx> for GeneratorWitness<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: GeneratorWitness<'tcx>,
        b: GeneratorWitness<'tcx>,
    ) -> RelateResult2<'tcx> {
        for (a, b) in std::iter::zip(a.0, b.0) {
            relation.relate(a, b)?;
        }
        Ok(())
    }
}

impl<'tcx> Relate<'tcx> for ImplSubject<'tcx> {
    #[inline]
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ImplSubject<'tcx>,
        b: ImplSubject<'tcx>,
    ) -> RelateResult<'tcx, ImplSubject<'tcx>> {
        match (a, b) {
            (ImplSubject::Trait(trait_ref_a), ImplSubject::Trait(trait_ref_b)) => {
                let trait_ref = ty::TraitRef::relate(relation, trait_ref_a, trait_ref_b)?;
                Ok(ImplSubject::Trait(trait_ref))
            }
            (ImplSubject::Inherent(ty_a), ImplSubject::Inherent(ty_b)) => {
                let ty = Ty::relate(relation, ty_a, ty_b)?;
                Ok(ImplSubject::Inherent(ty))
            }
            (ImplSubject::Trait(_), ImplSubject::Inherent(_))
            | (ImplSubject::Inherent(_), ImplSubject::Trait(_)) => {
                bug!("can not relate TraitRef and Ty");
            }
        }
    }
}

impl<'tcx> Relate2<'tcx> for ImplSubject<'tcx> {
    #[inline]
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ImplSubject<'tcx>,
        b: ImplSubject<'tcx>,
    ) -> RelateResult2<'tcx> {
        match (a, b) {
            (ImplSubject::Trait(trait_ref_a), ImplSubject::Trait(trait_ref_b)) => {
                relation.relate(trait_ref_a, trait_ref_b)?;
                Ok(())
            }
            (ImplSubject::Inherent(ty_a), ImplSubject::Inherent(ty_b)) => {
                relation.relate(ty_a, ty_b)?;
                Ok(())
            }
            (ImplSubject::Trait(_), ImplSubject::Inherent(_))
            | (ImplSubject::Inherent(_), ImplSubject::Trait(_)) => {
                bug!("can not relate TraitRef and Ty");
            }
        }
    }
}

impl<'tcx> Relate<'tcx> for Ty<'tcx> {
    #[inline]
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: Ty<'tcx>,
        b: Ty<'tcx>,
    ) -> RelateResult<'tcx, Ty<'tcx>> {
        relation.tys(a, b)
    }
}

impl<'tcx> Relate2<'tcx> for Ty<'tcx> {
    #[inline]
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: Ty<'tcx>,
        b: Ty<'tcx>,
    ) -> RelateResult2<'tcx> {
        relation.tys(a, b)
    }
}

/// Relates `a` and `b` structurally, calling the relation for all nested values.
/// Any semantic equality, e.g. of projections, and inference variables have to be
/// handled by the caller.
#[instrument(level = "trace", skip(relation), ret)]
pub fn structurally_relate_tys<'tcx, R: TypeRelation<'tcx>>(
    relation: &mut R,
    a: Ty<'tcx>,
    b: Ty<'tcx>,
) -> RelateResult<'tcx, Ty<'tcx>> {
    let tcx = relation.tcx();
    match (a.kind(), b.kind()) {
        (&ty::Infer(_), _) | (_, &ty::Infer(_)) => {
            // The caller should handle these cases!
            bug!("var types encountered in structurally_relate_tys")
        }

        (ty::Bound(..), _) | (_, ty::Bound(..)) => {
            bug!("bound types encountered in structurally_relate_tys")
        }

        (&ty::Error(guar), _) | (_, &ty::Error(guar)) => Ok(Ty::new_error(tcx, guar)),

        (&ty::Never, _)
        | (&ty::Char, _)
        | (&ty::Bool, _)
        | (&ty::Int(_), _)
        | (&ty::Uint(_), _)
        | (&ty::Float(_), _)
        | (&ty::Str, _)
            if a == b =>
        {
            Ok(a)
        }

        (ty::Param(a_p), ty::Param(b_p)) if a_p.index == b_p.index => Ok(a),

        (ty::Placeholder(p1), ty::Placeholder(p2)) if p1 == p2 => Ok(a),

        (&ty::Adt(a_def, a_args), &ty::Adt(b_def, b_args)) if a_def == b_def => {
            let args = relation.relate_item_args(a_def.did(), a_args, b_args)?;
            Ok(Ty::new_adt(tcx, a_def, args))
        }

        (&ty::Foreign(a_id), &ty::Foreign(b_id)) if a_id == b_id => Ok(Ty::new_foreign(tcx, a_id)),

        (&ty::Dynamic(a_obj, a_region, a_repr), &ty::Dynamic(b_obj, b_region, b_repr))
            if a_repr == b_repr =>
        {
            let region_bound = relation.relate(a_region, b_region)?;
            Ok(Ty::new_dynamic(tcx, relation.relate(a_obj, b_obj)?, region_bound, a_repr))
        }

        (&ty::Generator(a_id, a_args, movability), &ty::Generator(b_id, b_args, _))
            if a_id == b_id =>
        {
            // All Generator types with the same id represent
            // the (anonymous) type of the same generator expression. So
            // all of their regions should be equated.
            let args = relation.relate(a_args, b_args)?;
            Ok(Ty::new_generator(tcx, a_id, args, movability))
        }

        (&ty::GeneratorWitness(a_types), &ty::GeneratorWitness(b_types)) => {
            // Wrap our types with a temporary GeneratorWitness struct
            // inside the binder so we can related them
            let a_types = a_types.map_bound(GeneratorWitness);
            let b_types = b_types.map_bound(GeneratorWitness);
            // Then remove the GeneratorWitness for the result
            let types = relation.relate(a_types, b_types)?.map_bound(|witness| witness.0);
            Ok(Ty::new_generator_witness(tcx, types))
        }

        (&ty::GeneratorWitnessMIR(a_id, a_args), &ty::GeneratorWitnessMIR(b_id, b_args))
            if a_id == b_id =>
        {
            // All GeneratorWitness types with the same id represent
            // the (anonymous) type of the same generator expression. So
            // all of their regions should be equated.
            let args = relation.relate(a_args, b_args)?;
            Ok(Ty::new_generator_witness_mir(tcx, a_id, args))
        }

        (&ty::Closure(a_id, a_args), &ty::Closure(b_id, b_args)) if a_id == b_id => {
            // All Closure types with the same id represent
            // the (anonymous) type of the same closure expression. So
            // all of their regions should be equated.
            let args = relation.relate(a_args, b_args)?;
            Ok(Ty::new_closure(tcx, a_id, &args))
        }

        (&ty::RawPtr(a_mt), &ty::RawPtr(b_mt)) => {
            let mt = relate_type_and_mut(relation, a_mt, b_mt, a)?;
            Ok(Ty::new_ptr(tcx, mt))
        }

        (&ty::Ref(a_r, a_ty, a_mutbl), &ty::Ref(b_r, b_ty, b_mutbl)) => {
            let r = relation.relate(a_r, b_r)?;
            let a_mt = ty::TypeAndMut { ty: a_ty, mutbl: a_mutbl };
            let b_mt = ty::TypeAndMut { ty: b_ty, mutbl: b_mutbl };
            let mt = relate_type_and_mut(relation, a_mt, b_mt, a)?;
            Ok(Ty::new_ref(tcx, r, mt))
        }

        (&ty::Array(a_t, sz_a), &ty::Array(b_t, sz_b)) => {
            let t = relation.relate(a_t, b_t)?;
            match relation.relate(sz_a, sz_b) {
                Ok(sz) => Ok(Ty::new_array_with_const_len(tcx, t, sz)),
                Err(err) => {
                    // Check whether the lengths are both concrete/known values,
                    // but are unequal, for better diagnostics.
                    //
                    // It might seem dubious to eagerly evaluate these constants here,
                    // we however cannot end up with errors in `Relate` during both
                    // `type_of` and `predicates_of`. This means that evaluating the
                    // constants should not cause cycle errors here.
                    let sz_a = sz_a.try_eval_target_usize(tcx, relation.param_env());
                    let sz_b = sz_b.try_eval_target_usize(tcx, relation.param_env());
                    match (sz_a, sz_b) {
                        (Some(sz_a_val), Some(sz_b_val)) if sz_a_val != sz_b_val => Err(
                            TypeError::FixedArraySize(expected_found(relation, sz_a_val, sz_b_val)),
                        ),
                        _ => Err(err),
                    }
                }
            }
        }

        (&ty::Slice(a_t), &ty::Slice(b_t)) => {
            let t = relation.relate(a_t, b_t)?;
            Ok(Ty::new_slice(tcx, t))
        }

        (&ty::Tuple(as_), &ty::Tuple(bs)) => {
            if as_.len() == bs.len() {
                Ok(Ty::new_tup_from_iter(
                    tcx,
                    iter::zip(as_, bs).map(|(a, b)| relation.relate(a, b)),
                )?)
            } else if !(as_.is_empty() || bs.is_empty()) {
                Err(TypeError::TupleSize(expected_found(relation, as_.len(), bs.len())))
            } else {
                Err(TypeError::Sorts(expected_found(relation, a, b)))
            }
        }

        (&ty::FnDef(a_def_id, a_args), &ty::FnDef(b_def_id, b_args)) if a_def_id == b_def_id => {
            let args = relation.relate_item_args(a_def_id, a_args, b_args)?;
            Ok(Ty::new_fn_def(tcx, a_def_id, args))
        }

        (&ty::FnPtr(a_fty), &ty::FnPtr(b_fty)) => {
            let fty = relation.relate(a_fty, b_fty)?;
            Ok(Ty::new_fn_ptr(tcx, fty))
        }

        // The args of opaque types may not all be invariant, so we have
        // to treat them separately from other aliases.
        (
            &ty::Alias(ty::Opaque, ty::AliasTy { def_id: a_def_id, args: a_args, .. }),
            &ty::Alias(ty::Opaque, ty::AliasTy { def_id: b_def_id, args: b_args, .. }),
        ) if a_def_id == b_def_id => {
            let opt_variances = tcx.variances_of(a_def_id);
            let args = relate_args_with_variances(
                relation,
                a_def_id,
                opt_variances,
                a_args,
                b_args,
                false, // do not fetch `type_of(a_def_id)`, as it will cause a cycle
            )?;
            Ok(Ty::new_opaque(tcx, a_def_id, args))
        }

        // Alias tend to mostly already be handled downstream due to normalization.
        (&ty::Alias(a_kind, a_data), &ty::Alias(b_kind, b_data)) => {
            let alias_ty = relation.relate(a_data, b_data)?;
            assert_eq!(a_kind, b_kind);
            Ok(Ty::new_alias(tcx, a_kind, alias_ty))
        }

        _ => Err(TypeError::Sorts(expected_found(relation, a, b))),
    }
}

/// Relates `a` and `b` structurally, calling the relation for all nested values.
/// Any semantic equality, e.g. of projections, and inference variables have to be
/// handled by the caller.
#[instrument(level = "trace", skip(relation), ret)]
pub fn structurally_relate_tys2<'tcx, R: TypeRelation2<'tcx>>(
    relation: &mut R,
    a: Ty<'tcx>,
    b: Ty<'tcx>,
) -> RelateResult2<'tcx> {
    let tcx = relation.tcx();
    match (a.kind(), b.kind()) {
        (&ty::Infer(_), _) | (_, &ty::Infer(_)) => {
            // The caller should handle these cases!
            bug!("var types encountered in structurally_relate_tys")
        }

        (ty::Bound(..), _) | (_, ty::Bound(..)) => {
            bug!("bound types encountered in structurally_relate_tys")
        }

        (&ty::Error(_), _) | (_, &ty::Error(_)) => Ok(()),

        (&ty::Never, _)
        | (&ty::Char, _)
        | (&ty::Bool, _)
        | (&ty::Int(_), _)
        | (&ty::Uint(_), _)
        | (&ty::Float(_), _)
        | (&ty::Str, _)
            if a == b =>
        {
            Ok(())
        }

        (ty::Param(a_p), ty::Param(b_p)) if a_p.index == b_p.index => Ok(()),

        (ty::Placeholder(p1), ty::Placeholder(p2)) if p1 == p2 => Ok(()),

        (&ty::Adt(a_def, a_args), &ty::Adt(b_def, b_args)) if a_def == b_def => {
            relation.relate_item_args(a_def.did(), a_args, b_args)?;
            Ok(())
        }

        (&ty::Foreign(a_id), &ty::Foreign(b_id)) if a_id == b_id => Ok(()),

        (&ty::Dynamic(a_obj, a_region, a_repr), &ty::Dynamic(b_obj, b_region, b_repr))
            if a_repr == b_repr =>
        {
            relation.relate(a_region, b_region)?;
            relation.relate(a_obj, b_obj)?;
            Ok(())
        }

        (&ty::Generator(a_id, a_args, _), &ty::Generator(b_id, b_args, _)) if a_id == b_id => {
            relation.relate(a_args, b_args)?;
            Ok(())
        }

        (&ty::GeneratorWitness(a_types), &ty::GeneratorWitness(b_types)) => {
            // Wrap our types with a temporary GeneratorWitness struct
            // inside the binder so we can related them
            let a_types = a_types.map_bound(GeneratorWitness);
            let b_types = b_types.map_bound(GeneratorWitness);
            relation.relate(a_types, b_types)?;
            Ok(())
        }

        (&ty::GeneratorWitnessMIR(a_id, a_args), &ty::GeneratorWitnessMIR(b_id, b_args))
            if a_id == b_id =>
        {
            relation.relate(a_args, b_args)?;
            Ok(())
        }

        (&ty::Closure(a_id, a_args), &ty::Closure(b_id, b_args)) if a_id == b_id => {
            relation.relate(a_args, b_args)?;
            Ok(())
        }

        (&ty::RawPtr(a_mt), &ty::RawPtr(b_mt)) => {
            relate_type_and_mut2(relation, a_mt, b_mt, a)?;
            Ok(())
        }

        (&ty::Ref(a_r, a_ty, a_mutbl), &ty::Ref(b_r, b_ty, b_mutbl)) => {
            relation.relate(a_r, b_r)?;
            let a_mt = ty::TypeAndMut { ty: a_ty, mutbl: a_mutbl };
            let b_mt = ty::TypeAndMut { ty: b_ty, mutbl: b_mutbl };
            relate_type_and_mut2(relation, a_mt, b_mt, a)?;
            Ok(())
        }

        (&ty::Array(a_t, sz_a), &ty::Array(b_t, sz_b)) => {
            relation.relate(a_t, b_t)?;
            match relation.relate(sz_a, sz_b) {
                Ok(()) => Ok(()),
                Err(err) => {
                    // Check whether the lengths are both concrete/known values,
                    // but are unequal, for better diagnostics.
                    //
                    // It might seem dubious to eagerly evaluate these constants here,
                    // we however cannot end up with errors in `Relate` during both
                    // `type_of` and `predicates_of`. This means that evaluating the
                    // constants should not cause cycle errors here.
                    let sz_a = sz_a.try_eval_target_usize(tcx, relation.param_env());
                    let sz_b = sz_b.try_eval_target_usize(tcx, relation.param_env());
                    match (sz_a, sz_b) {
                        (Some(sz_a_val), Some(sz_b_val)) if sz_a_val != sz_b_val => {
                            Err(TypeError::FixedArraySize(expected_found2(
                                relation, sz_a_val, sz_b_val,
                            )))
                        }
                        _ => Err(err),
                    }
                }
            }
        }

        (&ty::Slice(a_t), &ty::Slice(b_t)) => {
            relation.relate(a_t, b_t)?;
            Ok(())
        }

        (&ty::Tuple(as_), &ty::Tuple(bs)) => {
            if as_.len() == bs.len() {
                for (a, b) in std::iter::zip(as_, bs) {
                    relation.relate(a, b)?;
                }
                Ok(())
            } else if !(as_.is_empty() || bs.is_empty()) {
                Err(TypeError::TupleSize(expected_found2(relation, as_.len(), bs.len())))
            } else {
                Err(TypeError::Sorts(expected_found2(relation, a, b)))
            }
        }

        (&ty::FnDef(a_def_id, a_args), &ty::FnDef(b_def_id, b_args)) if a_def_id == b_def_id => {
            relation.relate_item_args(a_def_id, a_args, b_args)?;
            Ok(())
        }

        (&ty::FnPtr(a_fty), &ty::FnPtr(b_fty)) => {
            relation.relate(a_fty, b_fty)?;
            Ok(())
        }

        // The args of opaque types may not all be invariant, so we have
        // to treat them separately from other aliases.
        (
            &ty::Alias(ty::Opaque, ty::AliasTy { def_id: a_def_id, args: a_args, .. }),
            &ty::Alias(ty::Opaque, ty::AliasTy { def_id: b_def_id, args: b_args, .. }),
        ) if a_def_id == b_def_id => {
            let opt_variances = tcx.variances_of(a_def_id);
            relate_args_with_variances2(
                relation,
                a_def_id,
                opt_variances,
                a_args,
                b_args,
                false, // do not fetch `type_of(a_def_id)`, as it will cause a cycle
            )?;
            Ok(())
        }

        // Alias tend to mostly already be handled downstream due to normalization.
        (&ty::Alias(a_kind, a_data), &ty::Alias(b_kind, b_data)) => {
            relation.relate(a_data, b_data)?;
            assert_eq!(a_kind, b_kind);
            Ok(())
        }

        _ => Err(TypeError::Sorts(expected_found2(relation, a, b))),
    }
}

/// Relates `a` and `b` structurally, calling the relation for all nested values.
/// Any semantic equality, e.g. of unevaluated consts, and inference variables have
/// to be handled by the caller.
///
/// FIXME: This is not totally structual, which probably should be fixed.
/// See the HACKs below.
pub fn structurally_relate_consts<'tcx, R: TypeRelation<'tcx>>(
    relation: &mut R,
    mut a: ty::Const<'tcx>,
    mut b: ty::Const<'tcx>,
) -> RelateResult<'tcx, ty::Const<'tcx>> {
    debug!("{}.structurally_relate_consts(a = {:?}, b = {:?})", relation.tag(), a, b);
    let tcx = relation.tcx();

    if tcx.features().generic_const_exprs {
        a = tcx.expand_abstract_consts(a);
        b = tcx.expand_abstract_consts(b);
    }

    debug!("{}.structurally_relate_consts(normed_a = {:?}, normed_b = {:?})", relation.tag(), a, b);

    // Currently, the values that can be unified are primitive types,
    // and those that derive both `PartialEq` and `Eq`, corresponding
    // to structural-match types.
    let is_match = match (a.kind(), b.kind()) {
        (ty::ConstKind::Infer(_), _) | (_, ty::ConstKind::Infer(_)) => {
            // The caller should handle these cases!
            bug!("var types encountered in structurally_relate_consts: {:?} {:?}", a, b)
        }

        (ty::ConstKind::Error(_), _) => return Ok(a),
        (_, ty::ConstKind::Error(_)) => return Ok(b),

        (ty::ConstKind::Param(a_p), ty::ConstKind::Param(b_p)) => a_p.index == b_p.index,
        (ty::ConstKind::Placeholder(p1), ty::ConstKind::Placeholder(p2)) => p1 == p2,
        (ty::ConstKind::Value(a_val), ty::ConstKind::Value(b_val)) => a_val == b_val,

        // While this is slightly incorrect, it shouldn't matter for `min_const_generics`
        // and is the better alternative to waiting until `generic_const_exprs` can
        // be stabilized.
        (ty::ConstKind::Unevaluated(au), ty::ConstKind::Unevaluated(bu)) if au.def == bu.def => {
            assert_eq!(a.ty(), b.ty());
            let args = relation.relate_with_variance(
                ty::Variance::Invariant,
                ty::VarianceDiagInfo::default(),
                au.args,
                bu.args,
            )?;
            return Ok(ty::Const::new_unevaluated(
                tcx,
                ty::UnevaluatedConst { def: au.def, args },
                a.ty(),
            ));
        }
        // Before calling relate on exprs, it is necessary to ensure that the nested consts
        // have identical types.
        (ty::ConstKind::Expr(ae), ty::ConstKind::Expr(be)) => {
            let r = relation;

            // FIXME(generic_const_exprs): is it possible to relate two consts which are not identical
            // exprs? Should we care about that?
            // FIXME(generic_const_exprs): relating the `ty()`s is a little weird since it is supposed to
            // ICE If they mismatch. Unfortunately `ConstKind::Expr` is a little special and can be thought
            // of as being generic over the argument types, however this is implicit so these types don't get
            // related when we relate the args of the item this const arg is for.
            let expr = match (ae, be) {
                (Expr::Binop(a_op, al, ar), Expr::Binop(b_op, bl, br)) if a_op == b_op => {
                    r.relate(al.ty(), bl.ty())?;
                    r.relate(ar.ty(), br.ty())?;
                    Expr::Binop(a_op, r.consts(al, bl)?, r.consts(ar, br)?)
                }
                (Expr::UnOp(a_op, av), Expr::UnOp(b_op, bv)) if a_op == b_op => {
                    r.relate(av.ty(), bv.ty())?;
                    Expr::UnOp(a_op, r.consts(av, bv)?)
                }
                (Expr::Cast(ak, av, at), Expr::Cast(bk, bv, bt)) if ak == bk => {
                    r.relate(av.ty(), bv.ty())?;
                    Expr::Cast(ak, r.consts(av, bv)?, r.tys(at, bt)?)
                }
                (Expr::FunctionCall(af, aa), Expr::FunctionCall(bf, ba))
                    if aa.len() == ba.len() =>
                {
                    r.relate(af.ty(), bf.ty())?;
                    let func = r.consts(af, bf)?;
                    let mut related_args = Vec::with_capacity(aa.len());
                    for (a_arg, b_arg) in aa.iter().zip(ba.iter()) {
                        related_args.push(r.consts(a_arg, b_arg)?);
                    }
                    let related_args = tcx.mk_const_list(&related_args);
                    Expr::FunctionCall(func, related_args)
                }
                _ => return Err(TypeError::ConstMismatch(expected_found(r, a, b))),
            };
            return Ok(ty::Const::new_expr(tcx, expr, a.ty()));
        }
        _ => false,
    };
    if is_match { Ok(a) } else { Err(TypeError::ConstMismatch(expected_found(relation, a, b))) }
}

/// Relates `a` and `b` structurally, calling the relation for all nested values.
/// Any semantic equality, e.g. of unevaluated consts, and inference variables have
/// to be handled by the caller.
///
/// FIXME: This is not totally structual, which probably should be fixed.
/// See the HACKs below.
pub fn structurally_relate_consts2<'tcx, R: TypeRelation2<'tcx>>(
    relation: &mut R,
    mut a: ty::Const<'tcx>,
    mut b: ty::Const<'tcx>,
) -> RelateResult2<'tcx> {
    debug!("{}.structurally_relate_consts(a = {:?}, b = {:?})", relation.tag(), a, b);
    let tcx = relation.tcx();

    if tcx.features().generic_const_exprs {
        a = tcx.expand_abstract_consts(a);
        b = tcx.expand_abstract_consts(b);
    }

    debug!("{}.structurally_relate_consts(normed_a = {:?}, normed_b = {:?})", relation.tag(), a, b);

    // Currently, the values that can be unified are primitive types,
    // and those that derive both `PartialEq` and `Eq`, corresponding
    // to structural-match types.
    let is_match = match (a.kind(), b.kind()) {
        (ty::ConstKind::Infer(_), _) | (_, ty::ConstKind::Infer(_)) => {
            // The caller should handle these cases!
            bug!("var types encountered in structurally_relate_consts: {:?} {:?}", a, b)
        }

        (ty::ConstKind::Error(_), _) => return Ok(()),
        (_, ty::ConstKind::Error(_)) => return Ok(()),

        (ty::ConstKind::Param(a_p), ty::ConstKind::Param(b_p)) => a_p.index == b_p.index,
        (ty::ConstKind::Placeholder(p1), ty::ConstKind::Placeholder(p2)) => p1 == p2,
        (ty::ConstKind::Value(a_val), ty::ConstKind::Value(b_val)) => a_val == b_val,

        // While this is slightly incorrect, it shouldn't matter for `min_const_generics`
        // and is the better alternative to waiting until `generic_const_exprs` can
        // be stabilized.
        (ty::ConstKind::Unevaluated(au), ty::ConstKind::Unevaluated(bu)) if au.def == bu.def => {
            assert_eq!(a.ty(), b.ty());
            relation.relate_with_variance(
                ty::Variance::Invariant,
                ty::VarianceDiagInfo::default(),
                au.args,
                bu.args,
            )?;
            return Ok(());
        }
        // Before calling relate on exprs, it is necessary to ensure that the nested consts
        // have identical types.
        (ty::ConstKind::Expr(ae), ty::ConstKind::Expr(be)) => {
            let r = relation;

            // FIXME(generic_const_exprs): is it possible to relate two consts which are not identical
            // exprs? Should we care about that?
            // FIXME(generic_const_exprs): relating the `ty()`s is a little weird since it is supposed to
            // ICE If they mismatch. Unfortunately `ConstKind::Expr` is a little special and can be thought
            // of as being generic over the argument types, however this is implicit so these types don't get
            // related when we relate the args of the item this const arg is for.
            match (ae, be) {
                (Expr::Binop(a_op, al, ar), Expr::Binop(b_op, bl, br)) if a_op == b_op => {
                    r.relate(al.ty(), bl.ty())?;
                    r.relate(ar.ty(), br.ty())?;
                    r.consts(al, bl)?;
                    r.consts(ar, br)?;
                }
                (Expr::UnOp(a_op, av), Expr::UnOp(b_op, bv)) if a_op == b_op => {
                    r.relate(av.ty(), bv.ty())?;
                    r.consts(av, bv)?;
                }
                (Expr::Cast(ak, av, at), Expr::Cast(bk, bv, bt)) if ak == bk => {
                    r.relate(av.ty(), bv.ty())?;
                    r.consts(av, bv)?;
                    r.tys(at, bt)?;
                }
                (Expr::FunctionCall(af, aa), Expr::FunctionCall(bf, ba))
                    if aa.len() == ba.len() =>
                {
                    r.relate(af.ty(), bf.ty())?;
                    r.consts(af, bf)?;
                    for (a_arg, b_arg) in aa.iter().zip(ba.iter()) {
                        r.consts(a_arg, b_arg)?;
                    }
                }
                _ => return Err(TypeError::ConstMismatch(expected_found2(r, a, b))),
            };
            return Ok(());
        }
        _ => false,
    };
    if is_match { Ok(()) } else { Err(TypeError::ConstMismatch(expected_found2(relation, a, b))) }
}

impl<'tcx> Relate<'tcx> for &'tcx ty::List<ty::PolyExistentialPredicate<'tcx>> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: Self,
        b: Self,
    ) -> RelateResult<'tcx, Self> {
        let tcx = relation.tcx();

        // FIXME: this is wasteful, but want to do a perf run to see how slow it is.
        // We need to perform this deduplication as we sometimes generate duplicate projections
        // in `a`.
        let mut a_v: Vec<_> = a.into_iter().collect();
        let mut b_v: Vec<_> = b.into_iter().collect();
        // `skip_binder` here is okay because `stable_cmp` doesn't look at binders
        a_v.sort_by(|a, b| a.skip_binder().stable_cmp(tcx, &b.skip_binder()));
        a_v.dedup();
        b_v.sort_by(|a, b| a.skip_binder().stable_cmp(tcx, &b.skip_binder()));
        b_v.dedup();
        if a_v.len() != b_v.len() {
            return Err(TypeError::ExistentialMismatch(expected_found(relation, a, b)));
        }

        let v = iter::zip(a_v, b_v).map(|(ep_a, ep_b)| {
            use crate::ty::ExistentialPredicate::*;
            match (ep_a.skip_binder(), ep_b.skip_binder()) {
                (Trait(a), Trait(b)) => Ok(ep_a
                    .rebind(Trait(relation.relate(ep_a.rebind(a), ep_b.rebind(b))?.skip_binder()))),
                (Projection(a), Projection(b)) => Ok(ep_a.rebind(Projection(
                    relation.relate(ep_a.rebind(a), ep_b.rebind(b))?.skip_binder(),
                ))),
                (AutoTrait(a), AutoTrait(b)) if a == b => Ok(ep_a.rebind(AutoTrait(a))),
                _ => Err(TypeError::ExistentialMismatch(expected_found(relation, a, b))),
            }
        });
        tcx.mk_poly_existential_predicates_from_iter(v)
    }
}

impl<'tcx> Relate2<'tcx> for &'tcx ty::List<ty::PolyExistentialPredicate<'tcx>> {
    fn relate2<R: TypeRelation2<'tcx>>(relation: &mut R, a: Self, b: Self) -> RelateResult2<'tcx> {
        let tcx = relation.tcx();

        // FIXME: this is wasteful, but want to do a perf run to see how slow it is.
        // We need to perform this deduplication as we sometimes generate duplicate projections
        // in `a`.
        let mut a_v: Vec<_> = a.into_iter().collect();
        let mut b_v: Vec<_> = b.into_iter().collect();
        // `skip_binder` here is okay because `stable_cmp` doesn't look at binders
        a_v.sort_by(|a, b| a.skip_binder().stable_cmp(tcx, &b.skip_binder()));
        a_v.dedup();
        b_v.sort_by(|a, b| a.skip_binder().stable_cmp(tcx, &b.skip_binder()));
        b_v.dedup();
        if a_v.len() != b_v.len() {
            return Err(TypeError::ExistentialMismatch(expected_found2(relation, a, b)));
        }

        for (ep_a, ep_b) in std::iter::zip(a_v, b_v) {
            use crate::ty::ExistentialPredicate::*;
            match (ep_a.skip_binder(), ep_b.skip_binder()) {
                (Trait(a), Trait(b)) => {
                    relation.relate(ep_a.rebind(a), ep_b.rebind(b))?;
                }
                (Projection(a), Projection(b)) => {
                    relation.relate(ep_a.rebind(a), ep_b.rebind(b))?;
                }
                (AutoTrait(a), AutoTrait(b)) if a == b => {}
                _ => return Err(TypeError::ExistentialMismatch(expected_found2(relation, a, b))),
            }
        }

        Ok(())
    }
}

impl<'tcx> Relate<'tcx> for ty::ClosureArgs<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ty::ClosureArgs<'tcx>,
        b: ty::ClosureArgs<'tcx>,
    ) -> RelateResult<'tcx, ty::ClosureArgs<'tcx>> {
        let args = relation.relate(a.args, b.args)?;
        Ok(ty::ClosureArgs { args })
    }
}

impl<'tcx> Relate<'tcx> for ty::GeneratorArgs<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ty::GeneratorArgs<'tcx>,
        b: ty::GeneratorArgs<'tcx>,
    ) -> RelateResult<'tcx, ty::GeneratorArgs<'tcx>> {
        let args = relation.relate(a.args, b.args)?;
        Ok(ty::GeneratorArgs { args })
    }
}

impl<'tcx> Relate2<'tcx> for ty::ClosureArgs<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ty::ClosureArgs<'tcx>,
        b: ty::ClosureArgs<'tcx>,
    ) -> RelateResult2<'tcx> {
        relation.relate(a.args, b.args)
    }
}

impl<'tcx> Relate2<'tcx> for ty::GeneratorArgs<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ty::GeneratorArgs<'tcx>,
        b: ty::GeneratorArgs<'tcx>,
    ) -> RelateResult2<'tcx> {
        relation.relate(a.args, b.args)
    }
}

impl<'tcx> Relate<'tcx> for GenericArgsRef<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: GenericArgsRef<'tcx>,
        b: GenericArgsRef<'tcx>,
    ) -> RelateResult<'tcx, GenericArgsRef<'tcx>> {
        relation.tcx().mk_args_from_iter(iter::zip(a, b).map(|(a, b)| {
            relation.relate_with_variance(ty::Invariant, ty::VarianceDiagInfo::default(), a, b)
        }))
    }
}

impl<'tcx> Relate2<'tcx> for GenericArgsRef<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: GenericArgsRef<'tcx>,
        b: GenericArgsRef<'tcx>,
    ) -> RelateResult2<'tcx> {
        for (a, b) in iter::zip(a, b) {
            relation.relate_with_variance(ty::Invariant, ty::VarianceDiagInfo::default(), a, b)?;
        }
        Ok(())
    }
}

impl<'tcx> Relate<'tcx> for ty::Region<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ty::Region<'tcx>,
        b: ty::Region<'tcx>,
    ) -> RelateResult<'tcx, ty::Region<'tcx>> {
        relation.regions(a, b)
    }
}

impl<'tcx> Relate2<'tcx> for ty::Region<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ty::Region<'tcx>,
        b: ty::Region<'tcx>,
    ) -> RelateResult2<'tcx> {
        relation.regions(a, b)
    }
}

impl<'tcx> Relate<'tcx> for ty::Const<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ty::Const<'tcx>,
        b: ty::Const<'tcx>,
    ) -> RelateResult<'tcx, ty::Const<'tcx>> {
        relation.consts(a, b)
    }
}

impl<'tcx> Relate2<'tcx> for ty::Const<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ty::Const<'tcx>,
        b: ty::Const<'tcx>,
    ) -> RelateResult2<'tcx> {
        relation.consts(a, b)
    }
}

impl<'tcx, T: Relate<'tcx>> Relate<'tcx> for ty::Binder<'tcx, T> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ty::Binder<'tcx, T>,
        b: ty::Binder<'tcx, T>,
    ) -> RelateResult<'tcx, ty::Binder<'tcx, T>> {
        relation.binders(a, b)
    }
}

impl<'tcx, T: Relate2<'tcx>> Relate2<'tcx> for ty::Binder<'tcx, T> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ty::Binder<'tcx, T>,
        b: ty::Binder<'tcx, T>,
    ) -> RelateResult2<'tcx> {
        relation.binders(a, b)
    }
}

impl<'tcx> Relate<'tcx> for GenericArg<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: GenericArg<'tcx>,
        b: GenericArg<'tcx>,
    ) -> RelateResult<'tcx, GenericArg<'tcx>> {
        match (a.unpack(), b.unpack()) {
            (GenericArgKind::Lifetime(a_lt), GenericArgKind::Lifetime(b_lt)) => {
                Ok(relation.relate(a_lt, b_lt)?.into())
            }
            (GenericArgKind::Type(a_ty), GenericArgKind::Type(b_ty)) => {
                Ok(relation.relate(a_ty, b_ty)?.into())
            }
            (GenericArgKind::Const(a_ct), GenericArgKind::Const(b_ct)) => {
                Ok(relation.relate(a_ct, b_ct)?.into())
            }
            (GenericArgKind::Lifetime(unpacked), x) => {
                bug!("impossible case reached: can't relate: {:?} with {:?}", unpacked, x)
            }
            (GenericArgKind::Type(unpacked), x) => {
                bug!("impossible case reached: can't relate: {:?} with {:?}", unpacked, x)
            }
            (GenericArgKind::Const(unpacked), x) => {
                bug!("impossible case reached: can't relate: {:?} with {:?}", unpacked, x)
            }
        }
    }
}

impl<'tcx> Relate2<'tcx> for GenericArg<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: GenericArg<'tcx>,
        b: GenericArg<'tcx>,
    ) -> RelateResult2<'tcx> {
        match (a.unpack(), b.unpack()) {
            (GenericArgKind::Lifetime(a_lt), GenericArgKind::Lifetime(b_lt)) => {
                relation.relate(a_lt, b_lt)?;
                Ok(())
            }
            (GenericArgKind::Type(a_ty), GenericArgKind::Type(b_ty)) => {
                relation.relate(a_ty, b_ty)?;
                Ok(())
            }
            (GenericArgKind::Const(a_ct), GenericArgKind::Const(b_ct)) => {
                relation.relate(a_ct, b_ct)?;
                Ok(())
            }
            (GenericArgKind::Lifetime(unpacked), x) => {
                bug!("impossible case reached: can't relate: {:?} with {:?}", unpacked, x)
            }
            (GenericArgKind::Type(unpacked), x) => {
                bug!("impossible case reached: can't relate: {:?} with {:?}", unpacked, x)
            }
            (GenericArgKind::Const(unpacked), x) => {
                bug!("impossible case reached: can't relate: {:?} with {:?}", unpacked, x)
            }
        }
    }
}

impl<'tcx> Relate<'tcx> for ty::ImplPolarity {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ty::ImplPolarity,
        b: ty::ImplPolarity,
    ) -> RelateResult<'tcx, ty::ImplPolarity> {
        if a != b {
            Err(TypeError::PolarityMismatch(expected_found(relation, a, b)))
        } else {
            Ok(a)
        }
    }
}

impl<'tcx> Relate2<'tcx> for ty::ImplPolarity {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ty::ImplPolarity,
        b: ty::ImplPolarity,
    ) -> RelateResult2<'tcx> {
        if a != b {
            Err(TypeError::PolarityMismatch(expected_found2(relation, a, b)))
        } else {
            Ok(())
        }
    }
}

impl<'tcx> Relate<'tcx> for ty::TraitPredicate<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ty::TraitPredicate<'tcx>,
        b: ty::TraitPredicate<'tcx>,
    ) -> RelateResult<'tcx, ty::TraitPredicate<'tcx>> {
        Ok(ty::TraitPredicate {
            trait_ref: relation.relate(a.trait_ref, b.trait_ref)?,
            polarity: relation.relate(a.polarity, b.polarity)?,
        })
    }
}

impl<'tcx> Relate2<'tcx> for ty::TraitPredicate<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ty::TraitPredicate<'tcx>,
        b: ty::TraitPredicate<'tcx>,
    ) -> RelateResult2<'tcx> {
        relation.relate(a.trait_ref, b.trait_ref)?;
        relation.relate(a.polarity, b.polarity)?;
        Ok(())
    }
}

impl<'tcx> Relate<'tcx> for Term<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: Self,
        b: Self,
    ) -> RelateResult<'tcx, Self> {
        Ok(match (a.unpack(), b.unpack()) {
            (TermKind::Ty(a), TermKind::Ty(b)) => relation.relate(a, b)?.into(),
            (TermKind::Const(a), TermKind::Const(b)) => relation.relate(a, b)?.into(),
            _ => return Err(TypeError::Mismatch),
        })
    }
}

impl<'tcx> Relate2<'tcx> for Term<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(relation: &mut R, a: Self, b: Self) -> RelateResult2<'tcx> {
        match (a.unpack(), b.unpack()) {
            (TermKind::Ty(a), TermKind::Ty(b)) => relation.relate(a, b),
            (TermKind::Const(a), TermKind::Const(b)) => relation.relate(a, b),
            _ => Err(TypeError::Mismatch),
        }
    }
}

impl<'tcx> Relate<'tcx> for ty::ProjectionPredicate<'tcx> {
    fn relate<R: TypeRelation<'tcx>>(
        relation: &mut R,
        a: ty::ProjectionPredicate<'tcx>,
        b: ty::ProjectionPredicate<'tcx>,
    ) -> RelateResult<'tcx, ty::ProjectionPredicate<'tcx>> {
        Ok(ty::ProjectionPredicate {
            projection_ty: relation.relate(a.projection_ty, b.projection_ty)?,
            term: relation.relate(a.term, b.term)?,
        })
    }
}

impl<'tcx> Relate2<'tcx> for ty::ProjectionPredicate<'tcx> {
    fn relate2<R: TypeRelation2<'tcx>>(
        relation: &mut R,
        a: ty::ProjectionPredicate<'tcx>,
        b: ty::ProjectionPredicate<'tcx>,
    ) -> RelateResult2<'tcx> {
        relation.relate(a.projection_ty, b.projection_ty)?;
        relation.relate(a.term, b.term)?;
        Ok(())
    }
}

///////////////////////////////////////////////////////////////////////////
// Error handling

pub fn expected_found<'tcx, R, T>(relation: &mut R, a: T, b: T) -> ExpectedFound<T>
where
    R: TypeRelation<'tcx>,
{
    ExpectedFound::new(relation.a_is_expected(), a, b)
}

pub fn expected_found2<'tcx, R, T>(relation: &mut R, a: T, b: T) -> ExpectedFound<T>
where
    R: TypeRelation2<'tcx>,
{
    ExpectedFound::new(relation.a_is_expected(), a, b)
}
