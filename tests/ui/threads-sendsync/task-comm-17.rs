//@run
#![allow(unused_must_use)]
//@ignore-target-emscripten no threads support
// pretty-expanded FIXME #23616

// Issue #922

// This test is specifically about spawning temporary closures.

use std::thread;

fn f() {
}

pub fn main() {
    thread::spawn(move|| f() ).join();
}
