// Regression test for #80988
//
//@ check-pass

#![forbid(warnings)]

#[deny(warnings)]
//~^ WARNING incompatible with previous forbid
//~| WARNING this will change its meaning in a future release!
fn main() {}
