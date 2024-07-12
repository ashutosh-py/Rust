// Proc macros using `mixed_site` spans exhibit usual properties of `macro_rules` hygiene.

//@ aux-build:mixed-site-span.rs

#[macro_use]
extern crate mixed_site_span;

struct ItemUse;

fn main() {
    'label_use: loop {
        let local_use = 1;
        proc_macro_rules!();
        //~^ ERROR use of undeclared label `'label_use`
        //~| ERROR cannot find value `local_use`
        ItemDef; // OK
        local_def; //~ ERROR cannot find value `local_def`
    }
}

macro_rules! pass_dollar_crate {
    () => (proc_macro_rules!($crate);) //~ ERROR cannot find type `ItemUse`
}
pass_dollar_crate!();
