// build-fail

// Regression test for #66975
#![warn(const_err)]

struct PrintName;

impl PrintName {
    const VOID: ! = panic!();
    //~^ ERROR evaluation of constant value failed
}

fn main() {
    let _ = PrintName::VOID;
}
