//! Removes assignments to ZST places.

use crate::MirPass;
use rustc_middle::mir::interpret::ConstValue;
use rustc_middle::mir::visit::*;
use rustc_middle::mir::*;
use rustc_middle::ty::{self, Ty, TyCtxt};

pub struct RemoveZsts;

impl<'tcx> MirPass<'tcx> for RemoveZsts {
    fn is_enabled(&self, sess: &rustc_session::Session) -> bool {
        sess.mir_opt_level() > 0
    }

    fn run_pass(&self, tcx: TyCtxt<'tcx>, body: &mut Body<'tcx>) {
        // Avoid query cycles (generators require optimized MIR for layout).
        if tcx.type_of(body.source.def_id()).is_generator() {
            return;
        }
        let param_env = tcx.param_env_reveal_all_normalized(body.source.def_id());
        let local_decls = &body.local_decls;
        let mut replacer = Replacer { tcx, param_env, local_decls };
        for var_debug_info in &mut body.var_debug_info {
            replacer.visit_var_debug_info(var_debug_info);
        }
        for (bb, data) in body.basic_blocks.as_mut_preserves_cfg().iter_enumerated_mut() {
            replacer.visit_basic_block_data(bb, data);
        }
    }
}

struct Replacer<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    param_env: ty::ParamEnv<'tcx>,
    local_decls: &'a LocalDecls<'tcx>,
}

/// A cheap, approximate check to avoid unnecessary `layout_of` calls.
fn maybe_zst(ty: Ty<'_>) -> bool {
    match ty.kind() {
        // maybe ZST (could be more precise)
        ty::Adt(..)
        | ty::Array(..)
        | ty::Closure(..)
        | ty::Tuple(..)
        | ty::Alias(ty::Opaque, ..) => true,
        // definitely ZST
        ty::FnDef(..) | ty::Never => true,
        // unreachable or can't be ZST
        _ => false,
    }
}

impl<'tcx> Replacer<'_, 'tcx> {
    fn is_zst(&self, ty: Ty<'tcx>) -> bool {
        if !maybe_zst(ty) {
            return false;
        }
        let Ok(layout) = self.tcx.layout_of(self.param_env.and(ty)) else {
            return false;
        };
        layout.is_zst()
    }

    fn make_zst(&self, ty: Ty<'tcx>) -> Constant<'tcx> {
        debug_assert!(self.is_zst(ty));
        Constant {
            span: rustc_span::DUMMY_SP,
            user_ty: None,
            literal: ConstantKind::Val(ConstValue::ZeroSized, ty),
        }
    }
}

impl<'tcx> MutVisitor<'tcx> for Replacer<'_, 'tcx> {
    fn tcx(&self) -> TyCtxt<'tcx> {
        self.tcx
    }

    fn visit_var_debug_info(&mut self, var_debug_info: &mut VarDebugInfo<'tcx>) {
        match var_debug_info.value {
            VarDebugInfoContents::Const(_) => {}
            VarDebugInfoContents::Place(place) => {
                let place_ty = place.ty(self.local_decls, self.tcx).ty;
                if self.is_zst(place_ty) {
                    var_debug_info.value = VarDebugInfoContents::Const(self.make_zst(place_ty))
                }
            }
            VarDebugInfoContents::Composite { ty, fragments: _ } => {
                if self.is_zst(ty) {
                    var_debug_info.value = VarDebugInfoContents::Const(self.make_zst(ty))
                }
            }
        }
    }

    fn visit_operand(&mut self, operand: &mut Operand<'tcx>, loc: Location) {
        if let Operand::Constant(_) = operand {
            return;
        }
        let op_ty = operand.ty(self.local_decls, self.tcx);
        if self.is_zst(op_ty)
            && self.tcx.consider_optimizing(|| {
                format!("RemoveZsts - Operand: {:?} Location: {:?}", operand, loc)
            })
        {
            *operand = Operand::Constant(Box::new(self.make_zst(op_ty)))
        }
    }

    fn visit_statement(&mut self, statement: &mut Statement<'tcx>, loc: Location) {
        if let StatementKind::Assign(box (place, _)) | StatementKind::Deinit(box place) =
            statement.kind
        {
            let place_ty = place.ty(self.local_decls, self.tcx).ty;
            if self.is_zst(place_ty)
                && self.tcx.consider_optimizing(|| {
                    format!(
                        "RemoveZsts - Place: {:?} SourceInfo: {:?}",
                        place, statement.source_info
                    )
                })
            {
                statement.make_nop();
            }
        }
        self.super_statement(statement, loc);
    }
}
