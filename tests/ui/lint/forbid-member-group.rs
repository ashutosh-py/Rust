// Check what happens when we forbid a member of
// a group but then allow the group.
//@ check-pass

#![forbid(unused_variables)]

#[allow(unused)]
//~^ WARNING incompatible with previous forbid
fn main() {
    let a: ();
}
