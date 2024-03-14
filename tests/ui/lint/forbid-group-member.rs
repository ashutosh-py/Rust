// Check what happens when we forbid a group but
// then allow a member of that group.
//
//@ check-pass

#![forbid(unused)]

#[allow(unused_variables)]
//~^ WARNING incompatible with previous forbid
//~| WARNING this will change its meaning in a future release!
fn main() {
    let a: ();
}
