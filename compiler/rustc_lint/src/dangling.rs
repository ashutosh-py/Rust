use rustc_hir::{Expr, ExprKind, LangItem};
use rustc_middle::ty::{Ty, TyCtxt};
use rustc_session::{declare_lint, declare_lint_pass};
use rustc_span::symbol::sym;

use crate::lints::InstantlyDangling;
use crate::{LateContext, LateLintPass, LintContext};

// FIXME: does not catch UnsafeCell::get
// FIXME: does not catch getting a ref to a temporary and then converting it to a ptr
declare_lint! {
    /// The `dangling_pointers_from_temporaries` lint detects getting a pointer to data
    /// of a temporary that will immediately get dropped.
    ///
    /// ### Example
    ///
    /// ```rust
    /// # #![allow(unused)]
    /// # unsafe fn use_data(ptr: *const u8) {
    /// #     dbg!(unsafe { ptr.read() });
    /// # }
    /// fn gather_and_use(bytes: impl Iterator<Item = u8>) {
    ///     let x: *const u8 = bytes.collect::<Vec<u8>>().as_ptr();
    ///     unsafe { use_data(x) }
    /// }
    /// ```
    ///
    /// {{produces}}
    ///
    /// ### Explanation
    ///
    /// Getting a pointer from a temporary value will not prolong its lifetime,
    /// which means that the value can be dropped and the allocation freed
    /// while the pointer still exists, making the pointer dangling.
    /// This is not an error (as far as the type system is concerned)
    /// but probably is not what the user intended either.
    ///
    /// If you need stronger guarantees, consider using references instead,
    /// as they are statically verified by the borrow-checker to never dangle.
    pub DANGLING_POINTERS_FROM_TEMPORARIES,
    Warn,
    "detects getting a pointer from a temporary"
}

declare_lint_pass!(DanglingPointers => [DANGLING_POINTERS_FROM_TEMPORARIES]);

impl<'tcx> LateLintPass<'tcx> for DanglingPointers {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'_>) {
        if let ExprKind::MethodCall(method, receiver, _args, _span) = expr.kind
            && matches!(method.ident.name, sym::as_ptr | sym::as_mut_ptr)
            && is_temporary_rvalue(receiver)
            && let ty = cx.typeck_results().expr_ty(receiver)
            && is_interesting(cx.tcx, ty)
        {
            cx.emit_span_lint(
                DANGLING_POINTERS_FROM_TEMPORARIES,
                method.ident.span,
                InstantlyDangling {
                    callee: method.ident.name,
                    ty,
                    ptr_span: method.ident.span,
                    temporary_span: receiver.span,
                },
            )
        }
    }
}

fn is_temporary_rvalue(expr: &Expr<'_>) -> bool {
    match expr.kind {
        // Const is not temporary.
        ExprKind::ConstBlock(..) | ExprKind::Repeat(..) | ExprKind::Lit(..) => false,

        // This is literally lvalue.
        ExprKind::Path(..) => false,

        // Calls return rvalues.
        ExprKind::Call(..) | ExprKind::MethodCall(..) | ExprKind::Binary(..) => true,

        // Inner blocks are rvalues.
        ExprKind::If(..) | ExprKind::Loop(..) | ExprKind::Match(..) | ExprKind::Block(..) => true,

        // FIXME: these should probably recurse and typecheck along the way.
        //        Some false negatives are possible for now.
        ExprKind::Index(..) | ExprKind::Field(..) | ExprKind::Unary(..) => false,

        ExprKind::Struct(..) => true,

        // FIXME: this has false negatives, but I do not want to deal with 'static/const promotion just yet.
        ExprKind::Array(..) => false,

        // These typecheck to `!`
        ExprKind::Break(..) | ExprKind::Continue(..) | ExprKind::Ret(..) | ExprKind::Become(..) => {
            false
        }

        // These typecheck to `()`
        ExprKind::Assign(..) | ExprKind::AssignOp(..) | ExprKind::Yield(..) => false,

        // Compiler-magic macros
        ExprKind::AddrOf(..) | ExprKind::OffsetOf(..) | ExprKind::InlineAsm(..) => false,

        // We are not interested in these
        ExprKind::Cast(..)
        | ExprKind::Closure(..)
        | ExprKind::Tup(..)
        | ExprKind::DropTemps(..)
        | ExprKind::Let(..) => false,

        // Not applicable
        ExprKind::Type(..) | ExprKind::Err(..) => false,
    }
}

// Array, Vec, String, CString, MaybeUninit, Cell, Box<[_]>, Box<str>, Box<CStr>,
// or any of the above in arbitrary many nested Box'es.
fn is_interesting(tcx: TyCtxt<'_>, ty: Ty<'_>) -> bool {
    if ty.is_array() {
        true
    } else if ty.is_box() {
        let inner = ty.boxed_ty();
        inner.is_slice()
            || inner.is_str()
            || inner.ty_adt_def().is_some_and(|def| tcx.is_lang_item(def.did(), LangItem::CStr))
            || is_interesting(tcx, inner)
    } else if let Some(def) = ty.ty_adt_def() {
        for lang_item in [LangItem::String, LangItem::MaybeUninit] {
            if tcx.is_lang_item(def.did(), lang_item) {
                return true;
            }
        }
        tcx.get_diagnostic_name(def.did())
            .is_some_and(|name| matches!(name, sym::cstring_type | sym::Vec | sym::Cell))
    } else {
        false
    }
}
