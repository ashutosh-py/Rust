// This test verifies that never type fallback preserves the following code in a
// compiling state. This pattern is fairly common in the wild, notably seen in
// wasmtime v0.16. Typically this is some closure wrapper that expects a
// collection of 'known' signatures, and -> ! is not included in that set.
//
// This test is specifically targeted by the unit type fallback when
// encountering a set of obligations like `?T: Foo` and `Trait::Projection =
// ?T`. In the code below, these are `R: Bar` and `Fn::Output = R`.
//
// check-fail

trait Bar {}
impl Bar for () {}
impl Bar for u32 {}

fn foo<R: Bar>(_: impl Fn() -> R) {}

fn main() {
    foo(|| panic!()); //~ ERROR the trait bound `!: Bar` is not satisfied
}
