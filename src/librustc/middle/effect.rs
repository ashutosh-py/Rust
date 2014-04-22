// Copyright 2012-2013 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Enforces the Rust effect system. Currently there is just one effect,
/// `unsafe`.

use middle::ty;
use middle::typeck::{MethodCall, MethodMap};
use util::ppaux;

use syntax::ast;
use syntax::codemap::Span;
use syntax::print::pprust;
use syntax::visit;
use syntax::visit::Visitor;

#[deriving(Eq, Clone)]
enum UnsafeContext {
    SafeContext,
    UnsafeFn,
    UnsafeBlock(ast::NodeId),
}

fn type_is_unsafe_function(ty: ty::t) -> bool {
    match ty::get(ty).sty {
        ty::ty_bare_fn(ref f) => f.fn_style == ast::UnsafeFn,
        ty::ty_closure(ref f) => f.fn_style == ast::UnsafeFn,
        _ => false,
    }
}

#[deriving(Eq, Clone)]
struct EffectEnv {
    /// Whether we're in an unsafe context.
    unsafe_context: UnsafeContext,

    /// Whether mut static usage should
    /// be forbidden regardless.
    allow_share: bool,
}

struct EffectCheckVisitor<'a> {
    tcx: &'a ty::ctxt,

    /// The method map.
    method_map: MethodMap,
}

impl<'a> EffectCheckVisitor<'a> {
    fn require_unsafe(&mut self, span: Span, env: &EffectEnv, description: &str) {
        match env.unsafe_context {
            SafeContext => {
                // Report an error.
                self.tcx.sess.span_err(span,
                                  format!("{} requires unsafe function or block",
                                       description))
            }
            UnsafeBlock(block_id) => {
                // OK, but record this.
                debug!("effect: recording unsafe block as used: {:?}", block_id);
                self.tcx.used_unsafe.borrow_mut().insert(block_id);
            }
            UnsafeFn => {}
        }
    }

    fn check_str_index(&mut self, e: @ast::Expr) {
        let base_type = match e.node {
            ast::ExprIndex(base, _) => ty::node_id_to_type(self.tcx, base.id),
            _ => return
        };
        debug!("effect: checking index with base type {}",
                ppaux::ty_to_str(self.tcx, base_type));
        match ty::get(base_type).sty {
            ty::ty_str(..) => {
                self.tcx.sess.span_err(e.span,
                    "modification of string types is not allowed");
            }
            _ => {}
        }
    }

    fn expr_is_mut_static(&mut self, e: &ast::Expr) -> bool {
        match self.tcx.def_map.borrow().find(&e.id) {
            Some(&ast::DefStatic(_, true)) => true,
            _ => false
        }
    }
}

impl<'a> Visitor<EffectEnv> for EffectCheckVisitor<'a> {
    fn visit_fn(&mut self, fn_kind: &visit::FnKind, fn_decl: &ast::FnDecl,
                block: &ast::Block, span: Span, node_id: ast::NodeId, env: EffectEnv) {

        let (is_item_fn, is_unsafe_fn) = match *fn_kind {
            visit::FkItemFn(_, _, fn_style, _) =>
                (true, fn_style == ast::UnsafeFn),
            visit::FkMethod(_, _, method) =>
                (true, method.fn_style == ast::UnsafeFn),
            _ => (false, false),
        };

        let mut env = env;

        if is_unsafe_fn {
            env.unsafe_context = UnsafeFn;
        } else if is_item_fn {
            env.unsafe_context = SafeContext;
        }

        visit::walk_fn(self, fn_kind, fn_decl, block, span, node_id, env);
    }

    fn visit_block(&mut self, block: &ast::Block, env: EffectEnv) {
        let mut env = env;
        match block.rules {
            ast::DefaultBlock => {}
            ast::UnsafeBlock(source) => {
                // By default only the outermost `unsafe` block is
                // "used" and so nested unsafe blocks are pointless
                // (the inner ones are unnecessary and we actually
                // warn about them). As such, there are two cases when
                // we need to create a new context, when we're
                // - outside `unsafe` and found a `unsafe` block
                //   (normal case)
                // - inside `unsafe` but found an `unsafe` block
                //   created internally to the compiler
                //
                // The second case is necessary to ensure that the
                // compiler `unsafe` blocks don't accidentally "use"
                // external blocks (e.g. `unsafe { println("") }`,
                // expands to `unsafe { ... unsafe { ... } }` where
                // the inner one is compiler generated).
                if env.unsafe_context == SafeContext || source == ast::CompilerGenerated {
                    env.unsafe_context = UnsafeBlock(block.id);
                }
            }
        }

        visit::walk_block(self, block, env);
    }

    fn visit_expr(&mut self, expr: &ast::Expr, env: EffectEnv) {
        let mut env = env;
        debug!("visit_expr(expr={}, allow_share={})",
               pprust::expr_to_str(expr), env.allow_share);
        match expr.node {
            ast::ExprMethodCall(_, _, ref args) => {
                let method_call = MethodCall::expr(expr.id);
                let base_type = self.method_map.borrow().get(&method_call).ty;
                debug!("effect: method call case, base type is {}",
                       ppaux::ty_to_str(self.tcx, base_type));
                if type_is_unsafe_function(base_type) {
                    self.require_unsafe(expr.span, &env,
                                        "invocation of unsafe method")
                }

                // This is a method call, hence we just check the first
                // expression in the call args which corresponds `Self`
                if self.expr_is_mut_static(*args.get(0)) {
                    let adj_ty = ty::expr_ty_adjusted(self.tcx, *args.get(0),
                                                      &*self.method_map.borrow());
                    match ty::get(adj_ty).sty {
                        ty::ty_rptr(_, mt) if mt.mutbl == ast::MutMutable => {
                            self.require_unsafe(expr.span, &env,
                                                "mutable borrow of mutable static");
                        }
                        _ => {}
                    }
                }

                env.allow_share = true;
            }
            ast::ExprIndex(base, index) => {
                self.visit_expr(base, env.clone());

                // It is safe to access share static mut
                // in index expressions.
                env.allow_share = true;
                return self.visit_expr(index, env);
            }
            ast::ExprCall(base, _) => {
                let base_type = ty::node_id_to_type(self.tcx, base.id);
                debug!("effect: call case, base type is {}",
                       ppaux::ty_to_str(self.tcx, base_type));
                if type_is_unsafe_function(base_type) {
                    self.require_unsafe(expr.span, &env, "call to unsafe function")
                }

                env.allow_share = true;
            }
            ast::ExprUnary(ast::UnDeref, base) => {
                let base_type = ty::node_id_to_type(self.tcx, base.id);
                debug!("effect: unary case, base type is {}",
                        ppaux::ty_to_str(self.tcx, base_type));
                match ty::get(base_type).sty {
                    ty::ty_ptr(_) => {
                        self.require_unsafe(expr.span, &env,
                                            "dereference of unsafe pointer")
                    }
                    _ => {}
                }
            }
            ast::ExprAssign(lhs, rhs) | ast::ExprAssignOp(_, lhs, rhs) => {
                self.check_str_index(lhs);

                debug!("assign(rhs={}, lhs={})",
                       pprust::expr_to_str(rhs),
                       pprust::expr_to_str(lhs))

                env.allow_share = true;
                self.visit_expr(rhs, env.clone());

                // we want to ignore `Share` statics
                // *just* in the LHS of the assignment.
                env.allow_share = false;
                return self.visit_expr(lhs, env);
            }
            ast::ExprAddrOf(ast::MutImmutable, _) => {
                env.allow_share =  true;
            }
            ast::ExprAddrOf(ast::MutMutable, base) => {
                 if self.expr_is_mut_static(base) {
                     self.require_unsafe(expr.span, &env,
                                         "mutable borrow of mutable static");
                 }

                self.check_str_index(base);
                env.allow_share = true;
            }
            ast::ExprInlineAsm(..) => {
                self.require_unsafe(expr.span, &env, "use of inline assembly")
            }
            ast::ExprPath(..) => {
                if self.expr_is_mut_static(expr) {
                    let ety = ty::node_id_to_type(self.tcx, expr.id);
                    if !env.allow_share || !ty::type_is_sharable(self.tcx, ety) {
                        self.require_unsafe(expr.span, &env, "this use of mutable static");
                    }
                }
            }
            _ => {}
        }

        visit::walk_expr(self, expr, env);
    }
}

pub fn check_crate(tcx: &ty::ctxt, method_map: MethodMap, krate: &ast::Crate) {
    let mut visitor = EffectCheckVisitor {
        tcx: tcx,
        method_map: method_map,
    };

    let env = EffectEnv{
        allow_share: false,
        unsafe_context: SafeContext,
    };

    visit::walk_crate(&mut visitor, krate, env);
}
