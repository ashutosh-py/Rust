#![feature(const_fn)]
pub fn abc() {}

pub fn bcd(_: u8) {}

pub fn cde() -> u16 {
    0xcde
}

pub fn def() {}

pub fn efg<A>(a: A, _: A) -> A {
    a
}

pub fn fgh(a: u8, _: u16) -> u8 {
    a
}

pub fn ghi(a: u8, _: u8) -> u16 {
    a as u16
}

pub const fn hij() -> u8 {
    0
}

pub fn ijk() -> u8 {
    0
}
