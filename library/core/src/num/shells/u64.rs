//! Redundant constants module for the [`u64` primitive type][u64].
//!
//! New code should use the associated constants directly on the primitive type.

#![stable(feature = "rust1", since = "1.0.0")]
#![deprecated(
    since = "1.69.0",
    note = "all constants in this module replaced by associated constants on `u64`"
)]

int_module! { u64 }
