use crate::{LateContext, LateLintPass, LintContext};

use hir::{Expr, Pat};
use rustc_errors::Applicability;
use rustc_hir as hir;
use rustc_infer::traits::TraitEngine;
use rustc_infer::{infer::TyCtxtInferExt, traits::ObligationCause};
use rustc_middle::ty::{self, List};
use rustc_span::{sym, Span};
use rustc_trait_selection::traits::TraitEngineExt;

declare_lint! {
    /// ### What it does
    ///
    /// Checks for `for` loops over `Option` or `Result` values.
    ///
    /// ### Why is this bad?
    /// Readability. This is more clearly expressed as an `if
    /// let`.
    ///
    /// ### Example
    ///
    /// ```rust
    /// # let opt = Some(1);
    /// # let res: Result<i32, std::io::Error> = Ok(1);
    /// for x in opt {
    ///     // ..
    /// }
    ///
    /// for x in &res {
    ///     // ..
    /// }
    ///
    /// for x in res.iter() {
    ///     // ..
    /// }
    /// ```
    ///
    /// Use instead:
    /// ```rust
    /// # let opt = Some(1);
    /// # let res: Result<i32, std::io::Error> = Ok(1);
    /// if let Some(x) = opt {
    ///     // ..
    /// }
    ///
    /// if let Ok(x) = res {
    ///     // ..
    /// }
    /// ```
    pub FOR_LOOP_OVER_FALLIBLES,
    Warn,
    "for-looping over an `Option` or a `Result`, which is more clearly expressed as an `if let`"
}

declare_lint_pass!(ForLoopOverFallibles => [FOR_LOOP_OVER_FALLIBLES]);

impl<'tcx> LateLintPass<'tcx> for ForLoopOverFallibles {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'_>) {
        let Some((pat, arg)) = extract_for_loop(expr) else { return };

        let ty = cx.typeck_results().expr_ty(arg);

        let &ty::Adt(adt, substs) = ty.kind() else { return };

        let (article, ty, var) = match adt.did() {
            did if cx.tcx.is_diagnostic_item(sym::Option, did) => ("an", "Option", "Some"),
            did if cx.tcx.is_diagnostic_item(sym::Result, did) => ("a", "Result", "Ok"),
            _ => return,
        };

        let msg = format!(
            "for loop over {article} `{ty}`. This is more readably written as an `if let` statement",
        );

        cx.struct_span_lint(FOR_LOOP_OVER_FALLIBLES, arg.span, |diag| {
            let mut warn = diag.build(msg);

            if let Some(recv) = extract_iterator_next_call(cx, arg)
            && let Ok(recv_snip) = cx.sess().source_map().span_to_snippet(recv.span)
            {
                warn.span_suggestion(
                    recv.span.between(arg.span.shrink_to_hi()),
                    format!("to iterate over `{recv_snip}` remove the call to `next`"),
                    "",
                    Applicability::MaybeIncorrect
                );
            } else {
                warn.multipart_suggestion_verbose(
                    format!("to check pattern in a loop use `while let`"),
                    vec![
                        // NB can't use `until` here because `expr.span` and `pat.span` have different syntax contexts
                        (expr.span.with_hi(pat.span.lo()), format!("while let {var}(")),
                        (pat.span.between(arg.span), format!(") = ")),
                    ],
                    Applicability::MaybeIncorrect
                );
            }

            if suggest_question_mark(cx, adt, substs, expr.span) {
                warn.span_suggestion(
                    arg.span.shrink_to_hi(),
                    "consider unwrapping the `Result` with `?` to iterate over its contents",
                    "?",
                    Applicability::MaybeIncorrect,
                );
            }

            warn.multipart_suggestion_verbose(
                "consider using `if let` to clear intent",
                vec![
                    // NB can't use `until` here because `expr.span` and `pat.span` have different syntax contexts
                    (expr.span.with_hi(pat.span.lo()), format!("if let {var}(")),
                    (pat.span.between(arg.span), format!(") = ")),
                ],
                Applicability::MachineApplicable,
            );

            warn.emit()
        })
    }
}

fn extract_for_loop<'tcx>(expr: &Expr<'tcx>) -> Option<(&'tcx Pat<'tcx>, &'tcx Expr<'tcx>)> {
    if let hir::ExprKind::DropTemps(e) = expr.kind
    && let hir::ExprKind::Match(iterexpr, [arm], hir::MatchSource::ForLoopDesugar) = e.kind
    && let hir::ExprKind::Call(_, [arg]) = iterexpr.kind
    && let hir::ExprKind::Loop(block, ..) = arm.body.kind
    && let [stmt] = block.stmts
    && let hir::StmtKind::Expr(e) = stmt.kind
    && let hir::ExprKind::Match(_, [_, some_arm], _) = e.kind
    && let hir::PatKind::Struct(_, [field], _) = some_arm.pat.kind
    {
        Some((field.pat, arg))
    } else {
        None
    }
}

fn extract_iterator_next_call<'tcx>(
    cx: &LateContext<'_>,
    expr: &Expr<'tcx>,
) -> Option<&'tcx Expr<'tcx>> {
    // This won't work for `Iterator::next(iter)`, is this an issue?
    if let hir::ExprKind::MethodCall(_, [recv], _) = expr.kind
    && cx.typeck_results().type_dependent_def_id(expr.hir_id) == cx.tcx.lang_items().next_fn()
    {
        Some(recv)
    } else {
        return None
    }
}

fn suggest_question_mark<'tcx>(
    cx: &LateContext<'tcx>,
    adt: ty::AdtDef<'tcx>,
    substs: &List<ty::GenericArg<'tcx>>,
    span: Span,
) -> bool {
    let Some(body_id) = cx.enclosing_body else { return false };
    let Some(into_iterator_did) = cx.tcx.get_diagnostic_item(sym::IntoIterator) else { return false };

    if !cx.tcx.is_diagnostic_item(sym::Result, adt.did()) {
        return false;
    }

    // Check that the function/closure/constant we are in has a `Result` type.
    // Otherwise suggesting using `?` may not be a good idea.
    {
        let ty = cx.typeck_results().expr_ty(&cx.tcx.hir().body(body_id).value);
        let ty::Adt(ret_adt, ..) = ty.kind() else { return false };
        if !cx.tcx.is_diagnostic_item(sym::Result, ret_adt.did()) {
            return false;
        }
    }

    let ty = substs.type_at(0);
    let is_iterator = cx.tcx.infer_ctxt().enter(|infcx| {
        let mut fulfill_cx = <dyn TraitEngine<'_>>::new(infcx.tcx);

        let cause = ObligationCause::new(
            span,
            body_id.hir_id,
            rustc_infer::traits::ObligationCauseCode::MiscObligation,
        );
        fulfill_cx.register_bound(
            &infcx,
            ty::ParamEnv::empty(),
            // Erase any region vids from the type, which may not be resolved
            infcx.tcx.erase_regions(ty),
            into_iterator_did,
            cause,
        );

        // Select all, including ambiguous predicates
        let errors = fulfill_cx.select_all_or_error(&infcx);

        errors.is_empty()
    });

    is_iterator
}
