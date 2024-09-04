//@ compile-flags: --target=x86_64-unknown-linux-gnu --crate-type=lib
//@ needs-llvm-components: x86
//@ compile-flags: -Ctarget-feature=-soft-float
#![feature(no_core, lang_items)]
#![no_std]
#![no_core]

#[lang = "sized"]
pub trait Sized {}

fn main() {}
