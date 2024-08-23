#![feature(autodiff)]
//@ pretty-mode:expanded
//@ pretty-compare-only
//@ pp-exact:autodiff_reverse.pp

// Test that reverse mode ad macros are expanded correctly.

#[autodiff(df, Reverse, Duplicated, Const, Active)]
pub fn f1(x: &[f64], y: f64) -> f64 {
    unimplemented!()
}

fn main() {}
