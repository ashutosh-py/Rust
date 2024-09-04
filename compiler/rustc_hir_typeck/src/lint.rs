use rustc_hir::def_id::DefId;
use rustc_hir::{HirId, HirIdSet};
use rustc_middle::hir::place::{PlaceBase, PlaceWithHirId};
use rustc_middle::mir::FakeReadCause;
use rustc_middle::query::Providers;
use rustc_middle::ty::{self, TyCtxt};
use tracing::instrument;

use crate::expr_use_visitor::{Delegate, ExprUseVisitor};

#[derive(Default)]
struct ExtractConsumingNodeDelegate {
    nodes: HirIdSet,
}

impl<'tcx> Delegate<'tcx> for ExtractConsumingNodeDelegate {
    #[instrument(level = "debug", skip(self))]
    fn consume(&mut self, place_with_id: &PlaceWithHirId<'tcx>, _: HirId) {
        match place_with_id.place.base {
            PlaceBase::Rvalue => {
                self.nodes.insert(place_with_id.hir_id);
            }
            PlaceBase::Local(id) => {
                self.nodes.insert(id);
            }
            PlaceBase::Upvar(upvar) => {
                self.nodes.insert(upvar.var_path.hir_id);
            }
            PlaceBase::StaticItem => {}
        }
    }

    fn borrow(
        &mut self,
        _place_with_id: &PlaceWithHirId<'tcx>,
        _diag_expr_id: HirId,
        _bk: ty::BorrowKind,
    ) {
        // do nothing
    }

    fn mutate(&mut self, _assignee_place: &PlaceWithHirId<'tcx>, _diag_expr_id: HirId) {
        // do nothing
    }

    fn fake_read(
        &mut self,
        _place_with_id: &PlaceWithHirId<'tcx>,
        _cause: FakeReadCause,
        _diag_expr_id: HirId,
    ) {
        // do nothing
    }
}

fn extract_tail_expr_consuming_nodes<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> &'tcx HirIdSet {
    let hir = tcx.hir();
    let body = hir.body_owned_by(def_id.expect_local());
    let mut delegate = ExtractConsumingNodeDelegate::default();
    let euv = ExprUseVisitor::new((tcx, def_id.expect_local()), &mut delegate);
    let _ = euv.walk_expr(body.value);
    tcx.arena.alloc(delegate.nodes)
}

pub(crate) fn provide(providers: &mut Providers) {
    providers.extract_tail_expr_consuming_nodes = extract_tail_expr_consuming_nodes;
}
