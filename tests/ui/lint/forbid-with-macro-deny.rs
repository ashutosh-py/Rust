//@ aux-build:deny-macro.rs
//@ check-pass

#![forbid(unsafe_code)]

extern crate deny_macro;

fn main() {
    deny_macro::emit_deny! {}
}
