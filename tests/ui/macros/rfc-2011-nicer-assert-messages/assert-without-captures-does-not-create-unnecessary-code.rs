//@aux-build:common.rs
//@only-target-x86_64
//@run
// needs-unwind Asserting on contents of error message

#![feature(core_intrinsics, generic_assert)]

extern crate common;

fn main() {
  common::test!(
    let mut _nothing = ();
    [ 1 == 3 ] => "Assertion failed: 1 == 3"
  );
}
