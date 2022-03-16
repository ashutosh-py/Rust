// revisions: stable unstable

#![cfg_attr(unstable, feature(unstable))] // The feature from the ./auxiliary/staged-api.rs file.
#![feature(const_trait_impl)]
#![feature(staged_api)]
#![stable(feature = "rust1", since = "1.0.0")]

// aux-build: staged-api.rs
extern crate staged_api;

use staged_api::*;

#[stable(feature = "rust1", since = "1.0.0")]
pub struct Foo;

#[stable(feature = "rust1", since = "1.0.0")]
#[cfg_attr(unstable, rustc_const_unstable(feature = "foo", issue = "none"))]
impl const MyTrait for Foo {
    //[stable]~^ ERROR trait implementations cannot be const stable yet
    fn func() {}
}

// Const stability has no impact on usage in non-const contexts.
fn non_const_context() {
    Unstable::func();
    Foo::func();
}

#[unstable(feature = "none", issue = "none")]
const fn const_context() {
    Unstable::func();
    // ^ This is okay regardless of whether the `unstable` feature is enabled, as this function is
    // not const-stable.
    Foo::func();
    //[unstable]~^ not yet stable as a const fn
}

#[stable(feature = "rust1", since = "1.0.0")]
const fn stable_const_context() {
    Unstable::func();
    //[unstable]~^ ERROR not yet stable as a const fn
    Foo::func();
    //[unstable]~^ ERROR not yet stable as a const fn
}

fn main() {}
