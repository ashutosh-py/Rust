use rustc_hir::{Expr, ExprKind, LangItem};
use rustc_middle::ty::Ty;
use rustc_session::{declare_lint, declare_lint_pass};
use rustc_span::symbol::{sym, Ident};

use crate::lints::InstantlyDangling;
use crate::{LateContext, LateLintPass, LintContext};

declare_lint! {
    /// The `temporary_cstring_as_ptr` lint detects getting the inner pointer of
    /// a temporary `CString`.
    ///
    /// ### Example
    ///
    /// ```rust
    /// # #![allow(unused)]
    /// # use std::ffi::CString;
    /// let c_str = CString::new("foo").unwrap().as_ptr();
    /// ```
    ///
    /// {{produces}}
    ///
    /// ### Explanation
    ///
    /// The inner pointer of a `CString` lives only as long as the `CString` it
    /// points to. Getting the inner pointer of a *temporary* `CString` allows the `CString`
    /// to be dropped at the end of the statement, as it is not being referenced as far as the
    /// typesystem is concerned. This means outside of the statement the pointer will point to
    /// freed memory, which causes undefined behavior if the pointer is later dereferenced.
    pub TEMPORARY_CSTRING_AS_PTR,
    Warn,
    "detects getting the inner pointer of a temporary `CString`"
}

// FIXME: does not catch UnsafeCell::get
// FIXME: does not catch getting a ref to a temporary and then converting it to a ptr
declare_lint! {
    /// TODO
    pub INSTANTLY_DANGLING_POINTER,
    Warn,
    "detects getting a pointer that will immediately dangle"
}

declare_lint_pass!(DanglingPointers => [TEMPORARY_CSTRING_AS_PTR, INSTANTLY_DANGLING_POINTER]);

impl<'tcx> LateLintPass<'tcx> for DanglingPointers {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'_>) {
        // We have a method call.
        let ExprKind::MethodCall(method, receiver, _args, _span) = expr.kind else {
            return;
        };
        let Ident { name: method_name, span: method_span } = method.ident;

        // The method is `.as_ptr()` or `.as_mut_ptr`.
        if method_name != sym::as_ptr && method_name != sym::as_mut_ptr {
            return;
        }

        // It is called on a temporary rvalue.
        if !is_temporary_rvalue(receiver) {
            return;
        }

        // The temporary value's type is array, box, Vec, String, or CString
        let ty = cx.typeck_results().expr_ty(receiver);
        let Some(is_cstring) = as_container(cx, ty) else {
            return;
        };

        let span = method_span;
        let decorator = InstantlyDangling {
            callee: method_name,
            ty: ty.to_string(),
            ptr_span: method_span,
            temporary_span: receiver.span,
        };

        if is_cstring {
            cx.emit_span_lint(TEMPORARY_CSTRING_AS_PTR, span, decorator);
        } else {
            cx.emit_span_lint(INSTANTLY_DANGLING_POINTER, span, decorator);
        };
    }
}

fn is_temporary_rvalue(expr: &Expr<'_>) -> bool {
    match expr.kind {
        // We are not interested in these
        ExprKind::Cast(_, _) | ExprKind::Closure(_) | ExprKind::Tup(_) => false,

        // Const is not temporary.
        ExprKind::ConstBlock(_) | ExprKind::Repeat(_, _) => false,

        // This is literally lvalue.
        ExprKind::Path(_) => false,

        // Calls return rvalues.
        ExprKind::Call(_, _) | ExprKind::MethodCall(_, _, _, _) | ExprKind::Binary(_, _, _) => true,

        // Produces lvalue.
        ExprKind::Unary(_, _) | ExprKind::Index(_, _, _) => false,

        // Inner blocks are rvalues.
        ExprKind::If(_, _, _)
        | ExprKind::Loop(_, _, _, _)
        | ExprKind::Match(_, _, _)
        | ExprKind::Block(_, _) => true,

        ExprKind::DropTemps(inner) => is_temporary_rvalue(inner),
        ExprKind::Field(parent, _) => is_temporary_rvalue(parent),

        ExprKind::Struct(_, _, _) => true,
        // These are 'static
        ExprKind::Lit(_) => false,
        // FIXME: False negatives are possible, but arrays get promoted to 'static way too often.
        ExprKind::Array(_) => false,

        // These typecheck to `!`
        ExprKind::Break(_, _) | ExprKind::Continue(_) | ExprKind::Ret(_) | ExprKind::Become(_) => {
            false
        }

        // These typecheck to `()`
        ExprKind::Assign(_, _, _) | ExprKind::AssignOp(_, _, _) | ExprKind::Yield(_, _) => false,

        // Not applicable
        ExprKind::Type(_, _) | ExprKind::Err(_) | ExprKind::Let(_) => false,

        // These are compiler-magic macros
        ExprKind::AddrOf(_, _, _) | ExprKind::OffsetOf(_, _) | ExprKind::InlineAsm(_) => false,
    }
}

// None => not a container
// Some(true) => CString
// Some(false) => String, Vec, box, array, MaybeUninit, Cell
fn as_container(cx: &LateContext<'_>, ty: Ty<'_>) -> Option<bool> {
    if ty.is_array() {
        Some(false)
    } else if ty.is_box() {
        let inner = ty.boxed_ty();
        // We only care about Box<[..]>, Box<str>, Box<CStr>,
        // or Box<T> iff T is another type we care about
        if inner.is_slice()
            || inner.is_str()
            || inner.ty_adt_def().is_some_and(|def| cx.tcx.is_lang_item(def.did(), LangItem::CStr))
            || as_container(cx, inner).is_some()
        {
            Some(false)
        } else {
            None
        }
    } else if let Some(def) = ty.ty_adt_def() {
        for lang_item in [LangItem::String, LangItem::MaybeUninit] {
            if cx.tcx.is_lang_item(def.did(), lang_item) {
                return Some(false);
            }
        }
        match cx.tcx.get_diagnostic_name(def.did()) {
            Some(sym::cstring_type) => Some(true),
            Some(sym::Vec | sym::Cell) => Some(false),
            _ => None,
        }
    } else {
        None
    }
}
