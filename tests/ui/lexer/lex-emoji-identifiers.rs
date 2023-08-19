fn invalid_emoji_usages() {
    let arrow↔️ = "basic emoji"; //~ ERROR: identifiers cannot contain emoji
    let planet🪐 = "basic emoji"; //~ ERROR: identifiers cannot contain emoji
    let wireless🛜 = "basic emoji"; //~ ERROR: identifiers cannot contain emoji
    let key1️⃣ = "keycap sequence"; //~ ERROR: identifiers cannot contain emoji
    let flag🇺🇳 = "flag sequence"; //~ ERROR: identifiers cannot contain emoji
    let wales🏴 = "tag sequence"; //~ ERROR: identifiers cannot contain emoji
    let folded🙏🏿 = "modifier sequence"; //~ ERROR: identifiers cannot contain emoji
}

fn main() {
    invalid_emoji_usages();
}
