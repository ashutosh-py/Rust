// run-pass

const fn foo(x: usize) -> usize {
    return x;
}

fn main() {
    [0; foo(2)];
}
