#![feature(type_alias_impl_trait)]

type Foo = impl std::fmt::Debug;

trait Blah {
    type Output;
    fn method();
}

impl Blah for u32 {
    type Output = Foo;
    fn method() {
        let x: Foo = 22_u32;
        //~^ ERROR: opaque type constrained without being represented in the signature
    }
}

fn main() {}
