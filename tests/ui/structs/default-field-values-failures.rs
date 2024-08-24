#![feature(default_field_values)]

#[derive(Debug)]
pub struct S;

#[derive(Debug, Default)]
pub struct Foo {
    pub bar: S = S,
    pub baz: i32 = 42 + 3,
}

#[derive(Debug, Default)]
pub struct Bar {
    pub bar: S, //~ ERROR the trait bound `S: Default` is not satisfied
    pub baz: i32 = 42 + 3,
}

fn main () {
    let _ = Foo { .. }; // ok
    let _ = Foo::default(); // silenced
    let _ = Bar { .. }; //~ ERROR mandatory field
    let _ = Bar::default(); // silenced
    let _ = Bar { bar: S, .. }; // ok
}
