//@ run-pass
#![feature(default_field_values)]

#[derive(Debug)]
pub struct S;

#[derive(Debug, Default)]
pub struct Foo {
    pub bar: S = S,
    pub baz: i32 = 42 + 3,
}

fn main () {
    let x = Foo { .. };
    let y = Foo::default();
    let z = Foo { baz: 1, .. };

    assert_eq!(45, x.baz);
    assert_eq!(45, y.baz);
    assert_eq!(1, z.baz);
}
