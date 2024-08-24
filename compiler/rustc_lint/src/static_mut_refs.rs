use rustc_hir as hir;
use rustc_hir::{Expr, Stmt};
use rustc_middle::ty::{Mutability, TyKind};
use rustc_session::lint::FutureIncompatibilityReason;
use rustc_session::{declare_lint, declare_lint_pass};
use rustc_span::edition::Edition;
use rustc_span::Span;

use crate::lints::{MutRefSugg, RefOfMutStatic};
use crate::{LateContext, LateLintPass, LintContext};

declare_lint! {
    /// The `static_mut_refs` lint checks for shared or mutable references
    /// of mutable static inside `unsafe` blocks and `unsafe` functions.
    ///
    /// ### Example
    ///
    /// ```rust,edition2021
    /// fn main() {
    ///     static mut X: i32 = 23;
    ///     static mut Y: i32 = 24;
    ///
    ///     unsafe {
    ///         let y = &X;
    ///         let ref x = X;
    ///         let (x, y) = (&X, &Y);
    ///         foo(&X);
    ///     }
    /// }
    ///
    /// unsafe fn _foo() {
    ///     static mut X: i32 = 23;
    ///     static mut Y: i32 = 24;
    ///
    ///     let y = &X;
    ///     let ref x = X;
    ///     let (x, y) = (&X, &Y);
    ///     foo(&X);
    /// }
    ///
    /// fn foo<'a>(_x: &'a i32) {}
    /// ```
    ///
    /// {{produces}}
    ///
    /// ### Explanation
    ///
    /// Shared or mutable references of mutable static are almost always a mistake and
    /// can lead to undefined behavior and various other problems in your code.
    ///
    /// This lint is "warn" by default on editions up to 2021, in 2024 is "deny".
    pub STATIC_MUT_REFS,
    Warn,
    "shared references or mutable references of mutable static is discouraged",
    @future_incompatible = FutureIncompatibleInfo {
        reason: FutureIncompatibilityReason::EditionError(Edition::Edition2024),
        reference: "issue #114447 <https://github.com/rust-lang/rust/issues/114447>",
        explain_reason: false,
    };
    @edition Edition2024 => Deny;
}

declare_lint_pass!(StaticMutRefs => [STATIC_MUT_REFS]);

impl<'tcx> LateLintPass<'tcx> for StaticMutRefs {
    #[allow(rustc::usage_of_ty_tykind)]
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &Expr<'_>) {
        let err_span = expr.span;
        match expr.kind {
            hir::ExprKind::AddrOf(borrow_kind, m, ex)
                if matches!(borrow_kind, hir::BorrowKind::Ref)
                    && let Some(err_span) = path_is_static_mut(ex, err_span) =>
            {
                emit_static_mut_refs(
                    cx,
                    err_span,
                    err_span.with_hi(ex.span.lo()),
                    err_span.shrink_to_hi(),
                    Some(m),
                    !expr.span.from_expansion(),
                );
            }
            hir::ExprKind::Index(expr, _, _)
                if let Some(err_span) = path_is_static_mut(expr, err_span) =>
            {
                emit_static_mut_refs(
                    cx,
                    err_span,
                    err_span.with_hi(expr.span.lo()),
                    err_span.shrink_to_hi(),
                    None,
                    false,
                );
            }
            hir::ExprKind::MethodCall(_, e, _, _)
                if let Some(err_span) = path_is_static_mut(e, expr.span)
                    && let typeck = cx.tcx.typeck(expr.hir_id.owner)
                    && let Some(method_def_id) = typeck.type_dependent_def_id(expr.hir_id)
                    && let inputs =
                        cx.tcx.fn_sig(method_def_id).skip_binder().inputs().skip_binder()
                    && !inputs.is_empty()
                    && inputs[0].is_ref() =>
            {
                let m =
                    if let TyKind::Ref(_, _, m) = inputs[0].kind() { *m } else { Mutability::Not };

                emit_static_mut_refs(
                    cx,
                    err_span,
                    err_span.shrink_to_lo(),
                    err_span.shrink_to_hi(),
                    Some(m),
                    false,
                );
            }
            _ => {}
        }
    }

    fn check_stmt(&mut self, cx: &LateContext<'tcx>, stmt: &Stmt<'_>) {
        if let hir::StmtKind::Let(loc) = stmt.kind
            && let hir::PatKind::Binding(ba, _, _, _) = loc.pat.kind
            && let hir::ByRef::Yes(rmutbl) = ba.0
            && let Some(init) = loc.init
            && let Some(err_span) = path_is_static_mut(init, init.span)
        {
            emit_static_mut_refs(
                cx,
                err_span,
                err_span.shrink_to_lo(),
                err_span.shrink_to_hi(),
                Some(rmutbl),
                false,
            );
        }
    }
}

fn path_is_static_mut(mut expr: &hir::Expr<'_>, mut err_span: Span) -> Option<Span> {
    if err_span.from_expansion() {
        err_span = expr.span;
    }

    while let hir::ExprKind::Field(e, _) = expr.kind {
        expr = e;
    }

    if let hir::ExprKind::Path(qpath) = expr.kind
        && let hir::QPath::Resolved(_, path) = qpath
        && let hir::def::Res::Def(def_kind, _) = path.res
        && let hir::def::DefKind::Static { safety: _, mutability: Mutability::Mut, nested: false } =
            def_kind
    {
        return Some(err_span);
    }
    None
}

fn emit_static_mut_refs(
    cx: &LateContext<'_>,
    span: Span,
    lo: Span,
    hi: Span,
    mutable: Option<Mutability>,
    suggest_addr_of: bool,
) {
    let (shared_label, shared_note, mut_note, sugg) = match mutable {
        Some(Mutability::Mut) => {
            let sugg = if suggest_addr_of { Some(MutRefSugg::Mut { lo, hi }) } else { None };
            ("mutable ", false, true, sugg)
        }
        Some(Mutability::Not) => {
            let sugg = if suggest_addr_of { Some(MutRefSugg::Shared { lo, hi }) } else { None };
            ("shared ", true, false, sugg)
        }
        None => ("", true, true, None),
    };

    cx.emit_span_lint(
        STATIC_MUT_REFS,
        span,
        RefOfMutStatic { span, sugg, shared_label, shared_note, mut_note },
    );
}
