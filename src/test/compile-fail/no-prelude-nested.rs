// Copyright 2013 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![feature(no_prelude)]

// Test that things from the prelude aren't in scope. Use many of them
// so that renaming some things won't magically make this test fail
// for the wrong reason (e.g. if `Add` changes to `Addition`, and
// `no_prelude` stops working, then the `impl Add` will still
// fail with the same error message).
//
// Unlike `no_implicit_prelude`, `no_prelude` doesn't cascade into nested
// modules, this makes the impl in foo::baz work.

#[no_prelude]
mod foo {
    mod baz {
        struct Test;
        impl From<Test> for Test { fn from(t: Test) { Test }}
        impl Clone for Test { fn clone(&self) { Test } }
        impl Eq for Test {}

        fn foo() {
            drop(2)
        }
    }

    struct Test;
    impl From for Test {} //~ ERROR: not in scope
    impl Clone for Test {} //~ ERROR: not in scope
    impl Iterator for Test {} //~ ERROR: not in scope
    impl ToString for Test {} //~ ERROR: not in scope
    impl Eq for Test {} //~ ERROR: not in scope

    fn foo() {
        drop(2) //~ ERROR: unresolved name
    }
}

fn qux() {
    #[no_prelude]
    mod qux_inner {
        struct Test;
        impl From for Test {} //~ ERROR: not in scope
        impl Clone for Test {} //~ ERROR: not in scope
        impl Iterator for Test {} //~ ERROR: not in scope
        impl ToString for Test {} //~ ERROR: not in scope
        impl Eq for Test {} //~ ERROR: not in scope

        fn foo() {
            drop(2) //~ ERROR: unresolved name
        }
    }
}


fn main() {
    // these should work fine
    drop(2)
}
