#![feature(box_syntax)]

use std::cell::RefCell;

static boxed: Box<RefCell<isize>> = box RefCell::new(0);
//~^ ERROR allocations are not allowed in statics
//~| ERROR `std::cell::RefCell<isize>` cannot be shared between threads safely [E0277]

fn main() { }
