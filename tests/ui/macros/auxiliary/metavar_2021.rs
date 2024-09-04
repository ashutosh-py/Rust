//@ edition: 2021

#![feature(macro_metavar_expr)]

#[macro_export]
macro_rules! make_matcher {
    ($name:ident, $fragment_type:ident) => {
        #[macro_export]
        macro_rules! $name {
            ($$_:$fragment_type) => { true };
            ($$($$_:tt)*) => { false };
        }
    }
}

make_matcher!(is_expr_from_2021, expr);
