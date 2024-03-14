//@ compile-flags: -F deprecated
//@ check-pass

#[allow(deprecated)] //~ WARNING allow(deprecated) incompatible
fn main() {
}
