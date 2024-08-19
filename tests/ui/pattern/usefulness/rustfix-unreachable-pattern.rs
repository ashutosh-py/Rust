//@ run-rustfix
#![feature(never_patterns)]
#![deny(unreachable_patterns)]
#![allow(incomplete_features)]

enum Void {}

#[rustfmt::skip]
fn main() {
    let res: Result<(), Void> = Ok(());
    match res {
        Ok(_) => {}
        Err(_) => {} //~ ERROR unreachable
        Err(_) => {}, //~ ERROR unreachable
    }

    match res {
        Ok(_x) => {}
        Err(!), //~ ERROR unreachable
        Err(!) //~ ERROR unreachable
    }
}
