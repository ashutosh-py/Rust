// Copyright 2013-2016 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use convert::TryFrom;
use mem;
use ops;
use usize;

use super::{FusedIterator, TrustedLen};

/// Objects that have a notion of *successor* and *predecessor*
/// for the purpose of range iterators.
///
/// This trait is `unsafe` because implementations of the `unsafe` trait `TrustedLen`
/// depend on its implementations being correct.
#[unstable(feature = "step_trait",
           reason = "recently redesigned",
           issue = "42168")]
pub unsafe trait Step: Clone + PartialOrd + Sized {
    /// Returns the number of *successor* steps needed to get from `start` to `end`.
    ///
    /// Returns `None` if that number would overflow `usize`
    /// (or is infinite, if `end` would never be reached).
    ///
    /// This must hold for any `a`, `b`, and `n`:
    ///
    /// * `steps_between(&a, &b) == Some(0)` if and only if `a >= b`.
    /// * `steps_between(&a, &b) == Some(n)` if and only if `a.forward(n) == Some(b)`
    /// * `steps_between(&a, &b) == Some(n)` if and only if `b.backward(n) == Some(a)`
    fn steps_between(start: &Self, end: &Self) -> Option<usize>;

    /// Returns the value that would be obtained by taking the *successor* of `self`,
    /// `step_count` times.
    ///
    /// Returns `None` if this would overflow the range of values supported by the type `Self`.
    ///
    /// Note: `step_count == 1` is a common case,
    /// used for example in `Iterator::next` for ranges.
    ///
    /// This must hold for any `a`, `n`, and `m` where `n + m` doesn’t overflow:
    ///
    /// * `a.forward(n).and_then(|x| x.forward(m)) == a.forward(n + m)`
    fn forward(&self, step_count: usize) -> Option<Self>;

    /// Returns the value that would be obtained by taking the *predecessor* of `self`,
    /// `step_count` times.
    ///
    /// Returns `None` if this would overflow the range of values supported by the type `Self`.
    ///
    /// Note: `step_count == 1` is a common case,
    /// used for example in `Iterator::next_back` for ranges.
    ///
    /// This must hold for any `a`, `n`, and `m` where `n + m` doesn’t overflow:
    ///
    /// * `a.backward(n).and_then(|x| x.backward(m)) == a.backward(n + m)`
    fn backward(&self, step_count: usize) -> Option<Self>;
}

macro_rules! step_integer_impls {
    (
        narrower than or same width as usize:
            $( [ $narrower_unsigned:ident $narrower_signed: ident ] ),+;
        wider than usize:
            $( [ $wider_unsigned:ident $wider_signed: ident ] ),+;
    ) => {
        $(
            #[unstable(feature = "step_trait",
                       reason = "recently redesigned",
                       issue = "42168")]
            unsafe impl Step for $narrower_unsigned {
                #[inline]
                fn steps_between(start: &Self, end: &Self) -> Option<usize> {
                    if *start < *end {
                        // This relies on $narrower_unsigned <= usize
                        Some((*end - *start) as usize)
                    } else {
                        Some(0)
                    }
                }

                #[inline]
                fn forward(&self, n: usize) -> Option<Self> {
                    match Self::try_from(n) {
                        Ok(n_converted) => self.checked_add(n_converted),
                        Err(_) => None,  // if n is out of range, `something_unsigned + n` is too
                    }
                }

                #[inline]
                fn backward(&self, n: usize) -> Option<Self> {
                    match Self::try_from(n) {
                        Ok(n_converted) => self.checked_sub(n_converted),
                        Err(_) => None,  // if n is out of range, `something_in_range - n` is too
                    }
                }
            }

            #[unstable(feature = "step_trait",
                       reason = "recently redesigned",
                       issue = "42168")]
            unsafe impl Step for $narrower_signed {
                #[inline]
                fn steps_between(start: &Self, end: &Self) -> Option<usize> {
                    if *start < *end {
                        // This relies on $narrower_signed <= usize
                        //
                        // Casting to isize extends the width but preserves the sign.
                        // Use wrapping_sub in isize space and cast to usize
                        // to compute the difference that may not fit inside the range of isize.
                        Some((*end as isize).wrapping_sub(*start as isize) as usize)
                    } else {
                        Some(0)
                    }
                }

                #[inline]
                fn forward(&self, n: usize) -> Option<Self> {
                    match <$narrower_unsigned>::try_from(n) {
                        Ok(n_unsigned) => {
                            // Wrapping in unsigned space handles cases like
                            // `-120_i8.forward(200) == Some(80_i8)`,
                            // even though 200_usize is out of range for i8.
                            let self_unsigned = *self as $narrower_unsigned;
                            let wrapped = self_unsigned.wrapping_add(n_unsigned) as Self;
                            if wrapped >= *self {
                                Some(wrapped)
                            } else {
                                None  // Addition overflowed
                            }
                        }
                        // If n is out of range of e.g. u8,
                        // then it is bigger than the entire range for i8 is wide
                        // so `any_i8 + n` would overflow i8.
                        Err(_) => None,
                    }
                }
                #[inline]
                fn backward(&self, n: usize) -> Option<Self> {
                    match <$narrower_unsigned>::try_from(n) {
                        Ok(n_unsigned) => {
                            // Wrapping in unsigned space handles cases like
                            // `-120_i8.forward(200) == Some(80_i8)`,
                            // even though 200_usize is out of range for i8.
                            let self_unsigned = *self as $narrower_unsigned;
                            let wrapped = self_unsigned.wrapping_sub(n_unsigned) as Self;
                            if wrapped <= *self {
                                Some(wrapped)
                            } else {
                                None  // Subtraction underflowed
                            }
                        }
                        // If n is out of range of e.g. u8,
                        // then it is bigger than the entire range for i8 is wide
                        // so `any_i8 - n` would underflow i8.
                        Err(_) => None,
                    }
                }
            }
        )+

        $(
            #[unstable(feature = "step_trait",
                       reason = "recently redesigned",
                       issue = "42168")]
            unsafe impl Step for $wider_unsigned {
                #[inline]
                fn steps_between(start: &Self, end: &Self) -> Option<usize> {
                    if *start < *end {
                        usize::try_from(*end - *start).ok()
                    } else {
                        Some(0)
                    }
                }

                #[inline]
                fn forward(&self, n: usize) -> Option<Self> {
                    self.checked_add(n as Self)
                }

                #[inline]
                fn backward(&self, n: usize) -> Option<Self> {
                    self.checked_sub(n as Self)
                }
            }

            #[unstable(feature = "step_trait",
                       reason = "recently redesigned",
                       issue = "42168")]
            unsafe impl Step for $wider_signed {
                #[inline]
                fn steps_between(start: &Self, end: &Self) -> Option<usize> {
                    if *start < *end {
                        match end.checked_sub(*start) {
                            Some(diff) => usize::try_from(diff).ok(),
                            // If the difference is too big for e.g. i128,
                            // it’s also gonna be too big for usize with fewer bits.
                            None => None
                        }
                    } else {
                        Some(0)
                    }
                }

                #[inline]
                fn forward(&self, n: usize) -> Option<Self> {
                    self.checked_add(n as Self)
                }

                #[inline]
                fn backward(&self, n: usize) -> Option<Self> {
                    self.checked_sub(n as Self)
                }
            }
        )+
    }
}

#[cfg(target_pointer_width = "64")]
step_integer_impls! {
    narrower than or same width as usize: [u8 i8], [u16 i16], [u32 i32], [u64 i64], [usize isize];
    wider than usize: [u128 i128];
}

#[cfg(target_pointer_width = "32")]
step_integer_impls! {
    narrower than or same width as usize: [u8 i8], [u16 i16], [u32 i32], [usize isize];
    wider than usize: [u64 i64], [u128 i128];
}

#[cfg(target_pointer_width = "16")]
step_integer_impls! {
    narrower than or same width as usize: [u8 i8], [u16 i16], [usize isize];
    wider than usize: [u32 i32], [u64 i64], [u128 i128];
}

macro_rules! range_exact_iter_impl {
    ($($t:ty)*) => ($(
        #[stable(feature = "rust1", since = "1.0.0")]
        impl ExactSizeIterator for ops::Range<$t> { }
    )*)
}

macro_rules! range_incl_exact_iter_impl {
    ($($t:ty)*) => ($(
        #[unstable(feature = "inclusive_range",
                   reason = "recently added, follows RFC",
                   issue = "28237")]
        impl ExactSizeIterator for ops::RangeInclusive<$t> { }
    )*)
}

#[stable(feature = "rust1", since = "1.0.0")]
impl<A: Step> Iterator for ops::Range<A> {
    type Item = A;

    #[inline]
    fn next(&mut self) -> Option<A> {
        if self.start < self.end {
            // `start + 1` should not overflow since `end` exists such that `start < end`
            let mut n = self.start.forward(1).expect("overflow in Range::next");
            mem::swap(&mut n, &mut self.start);
            Some(n)
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        // NOTE: the safety of `unsafe impl TrustedLen` depends on this being correct!
        match Step::steps_between(&self.start, &self.end) {
            Some(hint) => (hint, Some(hint)),
            None => (usize::MAX, None)
        }
    }

    #[inline]
    fn nth(&mut self, n: usize) -> Option<A> {
        if let Some(plus_n) = self.start.forward(n) {
            if plus_n < self.end {
                // `plus_n + 1` should not overflow since `end` exists such that `plus_n < end`
                self.start = plus_n.forward(1).expect("overflow in Range::nth");
                return Some(plus_n)
            }
        }

        self.start = self.end.clone();
        None
    }
}

// These macros generate `ExactSizeIterator` impls for various range types.
//
// * `ExactSizeIterator::len` is required to always return an exact `usize`,
//    so no range can be longer than usize::MAX.
// * For integer types in `Range<_>` this is the case for types narrower than or as wide as `usize`.
//   For integer types in `RangeInclusive<_>`
//   this is the case for types *strictly narrower* than `usize`
//   since e.g. `(0...u64::MAX).len()` would be `u64::MAX + 1`.
// * It hurts portability to have APIs (including `impl`s)
//   that are only available on some platforms,
//   so only `impl`s that are always valid should exist, regardless of the current target platform.
//   (NOTE: https://github.com/rust-lang/rust/issues/41619 might change this.)
// * Support exists in-tree for MSP430: https://forge.rust-lang.org/platform-support.html#tier-3
//   and out-of-tree for AVR: https://github.com/avr-rust/rust
//   Both are platforms where `usize` is 16 bits wide.
range_exact_iter_impl! {
    usize u8 u16
    isize i8 i16

    // These are incorrect per the reasoning above,
    // but removing them would be a breaking change as they were stabilized in Rust 1.0.0.
    // So `(0..66_000_u32).len()` for example will compile without error or warnings
    // on 16-bit platforms, but panic at run-time.
    u32
    i32
}
range_incl_exact_iter_impl! {
    u8
    i8
}

#[stable(feature = "rust1", since = "1.0.0")]
impl<A: Step> DoubleEndedIterator for ops::Range<A> {
    #[inline]
    fn next_back(&mut self) -> Option<A> {
        if self.start < self.end {
            // `end - 1` should not overflow since `start` exists such that `start < end`
            self.end = self.end.backward(1).expect("overflow in Range::nth_back");
            Some(self.end.clone())
        } else {
            None
        }
    }
}

#[unstable(feature = "trusted_len", issue = "37572")]
unsafe impl<T: Step> TrustedLen for ops::Range<T> {}

#[unstable(feature = "fused", issue = "35602")]
impl<A: Step> FusedIterator for ops::Range<A> {}

#[stable(feature = "rust1", since = "1.0.0")]
impl<A: Step> Iterator for ops::RangeFrom<A> {
    type Item = A;

    #[inline]
    fn next(&mut self) -> Option<A> {
        // Overflow can happen here. Panic when it does.
        let mut n = self.start.forward(1).expect("overflow in RangeFrom::next");
        mem::swap(&mut n, &mut self.start);
        Some(n)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (usize::MAX, None)
    }

    #[inline]
    fn nth(&mut self, n: usize) -> Option<A> {
        // Overflow can happen here. Panic when it does.
        let plus_n = self.start.forward(n).expect("overflow in RangeFrom::nth");
        self.start = plus_n.forward(1).expect("overflow in RangeFrom::nth");
        Some(plus_n)
    }
}

#[unstable(feature = "fused", issue = "35602")]
impl<A: Step> FusedIterator for ops::RangeFrom<A> {}

#[unstable(feature = "inclusive_range", reason = "recently added, follows RFC", issue = "28237")]
impl<A: Step> Iterator for ops::RangeInclusive<A> {
    type Item = A;

    #[inline]
    fn next(&mut self) -> Option<A> {
        use cmp::Ordering::*;

        match self.start.partial_cmp(&self.end) {
            Some(Less) => {
                // `start + 1` should not overflow since `end` exists such that `start < end`
                let n = self.start.forward(1).expect("overflow in RangeInclusive::next");
                Some(mem::replace(&mut self.start, n))
            },
            Some(Equal) => {
                let last;
                if let Some(end_plus_one) = self.end.forward(1) {
                    last = mem::replace(&mut self.start, end_plus_one);
                } else {
                    last = self.start.clone();
                    // `start == end`, and `end + 1` underflowed.
                    // `start - 1` overflowing would imply a type with only one valid value?
                    self.end = self.start.backward(1).expect("overflow in RangeInclusive::next");
                }
                Some(last)
            },
            _ => None,
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        // This seems redundant with a similar comparison that `Step::steps_between`
        // implementations need to do, but it separates the `start > end` case from `start == end`.
        // `steps_between` returns `Some(0)` in both of these cases, but we only want to add 1
        // in the latter.
        if !(self.start <= self.end) {
            return (0, Some(0));
        }

        // NOTE: the safety of `unsafe impl TrustedLen` depends on this being correct!
        match Step::steps_between(&self.start, &self.end) {
            Some(hint) => (hint.saturating_add(1), hint.checked_add(1)),
            None => (usize::MAX, None),
        }
    }

    #[inline]
    fn nth(&mut self, n: usize) -> Option<A> {
        if let Some(plus_n) = self.start.forward(n) {
            use cmp::Ordering::*;

            match plus_n.partial_cmp(&self.end) {
                Some(Less) => {
                    // `plus_n + 1` should not overflow since `end` exists such that `plus_n < end`
                    self.start = plus_n.forward(1).expect("overflow in RangeInclusive::nth");
                    return Some(plus_n)
                }
                Some(Equal) => {
                    if let Some(end_plus_one) = self.end.forward(1) {
                        self.start = end_plus_one
                    } else {
                        // `start == end`, and `end + 1` underflowed.
                        // `start - 1` overflowing would imply a type with only one valid value?
                        self.end = self.start.backward(1).expect("overflow in RangeInclusive::nth")
                    }
                    return Some(plus_n)
                }
                _ => {}
            }
        }

        if let Some(end_plus_one) = self.end.forward(1) {
            self.start = end_plus_one
        } else {
            // `start == end`, and `end + 1` underflowed.
            // `start - 1` overflowing would imply a type with only one valid value?
            self.end = self.start.backward(1).expect("overflow in RangeInclusive::nth")
        }
        None
    }
}

#[unstable(feature = "inclusive_range", reason = "recently added, follows RFC", issue = "28237")]
impl<A: Step> DoubleEndedIterator for ops::RangeInclusive<A> {
    #[inline]
    fn next_back(&mut self) -> Option<A> {
        use cmp::Ordering::*;

        match self.start.partial_cmp(&self.end) {
            Some(Less) => {
                // `end - 1` should not overflow since `start` exists such that `start < end`
                let n = self.end.backward(1).expect("overflow in RangeInclusive::next_back");
                Some(mem::replace(&mut self.end, n))
            },
            Some(Equal) => {
                let last;
                if let Some(start_minus_one) = self.start.backward(1) {
                    last = mem::replace(&mut self.end, start_minus_one);
                } else {
                    last = self.end.clone();
                    // `start == end`, and `start - 1` underflowed.
                    // `end + 1` overflowing would imply a type with only one valid value?
                    self.start =
                        self.start.forward(1).expect("overflow in RangeInclusive::next_back");
                }
                Some(last)
            },
            _ => None,
        }
    }
}

#[unstable(feature = "inclusive_range",
           reason = "recently added, follows RFC",
           issue = "28237")]
unsafe impl<T: Step> TrustedLen for ops::RangeInclusive<T> { }

#[unstable(feature = "fused", issue = "35602")]
impl<A: Step> FusedIterator for ops::RangeInclusive<A> {}
