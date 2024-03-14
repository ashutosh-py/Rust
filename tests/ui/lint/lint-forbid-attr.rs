//@ check-pass
#![forbid(deprecated)]

#[allow(deprecated)]
//~^ WARNING allow(deprecated) incompatible
fn main() {
}
