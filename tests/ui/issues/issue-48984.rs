// build-pass

// aux-build:issue-48984-aux.rs
extern crate issue48984aux;
use issue48984aux::Bar;

fn do_thing<T: Bar>() { }

fn main() { }
