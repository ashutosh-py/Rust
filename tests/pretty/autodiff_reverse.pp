#![feature(prelude_import)]
#![no_std]
#![feature(autodiff)]
#[prelude_import]
use ::std::prelude::rust_2015::*;
#[macro_use]
extern crate std;
//@ pretty-mode:expanded
//@ pretty-compare-only
//@ pp-exact:autodiff_reverse.pp

// Test that reverse mode ad macros are expanded correctly.

#[rustc_autodiff]
#[inline(never)]
pub fn f1(x: &[f64], y: f64) -> f64 {

    ::core::panicking::panic("not implemented")
}
#[rustc_autodiff(Reverse, Duplicated, Const, Active,)]
#[inline(never)]
pub fn df(x: &[f64], dx: &mut [f64], y: f64, dret: f64) -> f64 {
    unsafe { asm!("NOP"); };
    ::core::hint::black_box(f1(x, y));
    ::core::hint::black_box((dx, dret));
    ::core::hint::black_box(f1(x, y))
}
fn main() {}
