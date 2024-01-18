// `mut` resets the binding mode.
#![deny(dereferencing_mut_binding)]

fn main() {
    let (x, mut y) = &(0, 0);
    //~^ ERROR dereferencing `mut`
    let _: &u32 = x;
    let _: u32 = y;

    match &Some(5i32) {
        Some(mut n) => {
            //~^ ERROR dereferencing `mut`
            n += 1;
            let _ = n;
        }
        None => {}
    };
    if let Some(mut n) = &Some(5i32) {
        //~^ ERROR dereferencing `mut`
        n += 1;
        let _ = n;
    };
    match &Some(5i32) {
        &Some(mut n) => {
            n += 1;
            let _ = n;
        }
        None => {}
    };
}
