//@ check-pass
// https://github.com/rust-lang/rust/issues/129541

#[derive(Clone)]
struct Test {
    field: std::borrow::Cow<'static, [Self]>,
}

fn main(){}
