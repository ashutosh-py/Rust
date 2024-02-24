//@ compile-flags: -Zdeduplicate-diagnostics=yes

#![feature(lint_reasons)]

#[forbid(unused_variables)]
//~^ NOTE `forbid` level set here
#[expect(unused_variables)]
//~^ WARNING incompatible with previous forbid [E0453]
//~| NOTE overruled by previous forbid
//~| NOTE on by default
//~| WARNING this lint expectation is unfulfilled
//~| NOTE on by default
fn expect_forbidden_lint_1() {}

#[forbid(while_true)]
//~^ NOTE `forbid` level set here
//~| NOTE the lint level is defined here
#[expect(while_true)]
//~^ WARNING incompatible with previous forbid [E0453]
//~| NOTE overruled by previous forbid
//~| WARNING this lint expectation is unfulfilled
fn expect_forbidden_lint_2() {
    // This while loop will produce a `while_true` lint as the lint level
    // at this node is still `forbid` and the `while_true` check happens
    // before the compilation terminates due to `E0453`
    while true {}
    //~^ ERROR denote infinite loops with `loop { ... }`
    //~| HELP use `loop`
}

fn main() {
    expect_forbidden_lint_1();
    expect_forbidden_lint_2();
}
