//@ check-fail

#[derive(Copy, Clone)]
pub struct ChildStdin {
    inner: AnonPipe,
}

#[derive(Copy, Clone)]
struct AnonPipe(private::Void);

mod private {
    #[derive(Copy, Clone)]
    pub struct Void(PrivateVoid);
    #[derive(Copy, Clone)]
    enum PrivateVoid {}
}

const FOO: () = {
    union Foo {
        a: ChildStdin,
        b: (),
    }
    let x = unsafe { Foo { b: () }.a };
    //~^ ERROR: evaluation of constant value failed
    let x = &x.inner;
};

fn main() {}
