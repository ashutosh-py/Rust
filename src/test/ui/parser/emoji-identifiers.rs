struct ABig👩‍👩‍👧‍👧Family; //~ ERROR identifiers cannot contain emojis
struct 👀; //~ ERROR identifiers cannot contain emojis
impl 👀 {
    fn full_of_✨() -> 👀 { //~ ERROR identifiers cannot contain emojis
        👀
    }
}
fn i_like_to_😅_a_lot() -> 👀 { //~ ERROR identifiers cannot contain emojis
    👀::full_of✨() //~ ERROR no function or associated item named `full_of✨` found for struct `👀`
    //~^ ERROR identifiers cannot contain emojis
}
fn main() {
    let _ = i_like_to_😄_a_lot() ➖ 4; //~ ERROR cannot find function `i_like_to_😄_a_lot` in this scope
    //~^ ERROR identifiers cannot contain emojis
    //~| ERROR unknown start of token: \u{2796}
}
