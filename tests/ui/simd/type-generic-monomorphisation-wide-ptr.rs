// build-fail

#![feature(repr_simd)]
// ignore-tidy-linelength
//@error-in-other-file:monomorphising SIMD type `S<[*mut [u8]; 4]>` with a non-primitive-scalar (integer/float/pointer) element type `*mut [u8]`

#[repr(simd)]
struct S<T>(T);

fn main() {
    let _v: Option<S<[*mut [u8]; 4]>> = None;
}
