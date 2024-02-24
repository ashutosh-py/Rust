// Check what happens when we forbid a bigger group but
// then deny a subset of that group.

#![forbid(warnings)]
#![deny(forbidden_lint_groups)]

#[allow(nonstandard_style)]
//~^ ERROR incompatible with previous
//~| WARNING this will change its meaning in a future release!
//~| ERROR incompatible with previous
//~| WARNING this will change its meaning in a future release!
//~| ERROR incompatible with previous
//~| WARNING this will change its meaning in a future release!
fn main() {}
