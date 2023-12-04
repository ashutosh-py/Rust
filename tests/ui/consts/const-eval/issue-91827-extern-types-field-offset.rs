// Test that we can handle unsized types with an extern type tail part.
// Regression test for issue #91827.

#![feature(extern_types)]

use std::ptr::addr_of;

extern "C" {
    type Opaque;
}

struct Newtype(Opaque);

struct S {
    i: i32,
    a: Opaque,
}

const NEWTYPE: () = unsafe {
    // Projecting to the newtype works, because it is always at offset 0.
    let x: &Newtype = unsafe { &*(1usize as *const Newtype) };
    let field = &x.0;
};

const OFFSET: () = unsafe {
    // This needs to compute the field offset, but we don't know the type's alignment, so this fail.
    let x: &S = unsafe { &*(1usize as *const S) };
    let field = &x.a; //~ERROR: evaluation of constant value failed
    //~| does not have a known offset
};

fn main() {}
