use super::{Capturing, ForceCollect, Parser, TrailingToken};
use rustc_ast::token;
use rustc_ast::tokenstream::{AttrsTarget, LazyAttrTokenStream, ReplaceRange};
use rustc_ast::{self as ast};
use rustc_ast::{AttrVec, Attribute, HasAttrs, HasTokens};
use rustc_errors::PResult;
use rustc_session::parse::ParseSess;
use rustc_span::{sym, DUMMY_SP};

use std::mem;

/// A wrapper type to ensure that the parser handles outer attributes correctly.
/// When we parse outer attributes, we need to ensure that we capture tokens
/// for the attribute target. This allows us to perform cfg-expansion on
/// a token stream before we invoke a derive proc-macro.
///
/// This wrapper prevents direct access to the underlying `ast::AttrVec`.
/// Parsing code can only get access to the underlying attributes
/// by passing an `AttrWrapper` to `collect_tokens_trailing_tokens`.
/// This makes it difficult to accidentally construct an AST node
/// (which stores an `ast::AttrVec`) without first collecting tokens.
///
/// This struct has its own module, to ensure that the parser code
/// cannot directly access the `attrs` field
#[derive(Debug, Clone)]
pub struct AttrWrapper {
    attrs: AttrVec,
    // The start of the outer attributes in the token cursor.
    // This allows us to create a `ReplaceRange` for the entire attribute
    // target, including outer attributes.
    start_pos: u32,
}

impl AttrWrapper {
    pub(super) fn new(attrs: AttrVec, start_pos: u32) -> AttrWrapper {
        AttrWrapper { attrs, start_pos }
    }
    pub fn empty() -> AttrWrapper {
        AttrWrapper { attrs: AttrVec::new(), start_pos: u32::MAX }
    }

    pub(crate) fn take_for_recovery(self, psess: &ParseSess) -> AttrVec {
        psess.dcx().span_delayed_bug(
            self.attrs.get(0).map(|attr| attr.span).unwrap_or(DUMMY_SP),
            "AttrVec is taken for recovery but no error is produced",
        );

        self.attrs
    }

    /// Prepend `self.attrs` to `attrs`.
    // FIXME: require passing an NT to prevent misuse of this method
    pub(crate) fn prepend_to_nt_inner(self, attrs: &mut AttrVec) {
        let mut self_attrs = self.attrs;
        mem::swap(attrs, &mut self_attrs);
        attrs.extend(self_attrs);
    }

    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }

    pub fn is_complete(&self) -> bool {
        crate::parser::attr::is_complete(&self.attrs)
    }
}

/// Returns `true` if `attrs` contains a `cfg` or `cfg_attr` attribute
fn has_cfg_or_cfg_attr(attrs: &[Attribute]) -> bool {
    // NOTE: Builtin attributes like `cfg` and `cfg_attr` cannot be renamed via imports.
    // Therefore, the absence of a literal `cfg` or `cfg_attr` guarantees that
    // we don't need to do any eager expansion.
    attrs.iter().any(|attr| {
        attr.ident().is_some_and(|ident| ident.name == sym::cfg || ident.name == sym::cfg_attr)
    })
}

impl<'a> Parser<'a> {
    /// Records all tokens consumed by the provided callback,
    /// including the current token. These tokens are collected
    /// into a `LazyAttrTokenStream`, and returned along with the result
    /// of the callback.
    ///
    /// The `attrs` passed in are in `AttrWrapper` form, which is opaque. The
    /// `AttrVec` within is passed to `f`. See the comment on `AttrWrapper` for
    /// details.
    ///
    /// Note: If your callback consumes an opening delimiter
    /// (including the case where you call `collect_tokens`
    /// when the current token is an opening delimiter),
    /// you must also consume the corresponding closing delimiter.
    ///
    /// That is, you can consume
    /// `something ([{ }])` or `([{}])`, but not `([{}]`
    ///
    /// This restriction shouldn't be an issue in practice,
    /// since this function is used to record the tokens for
    /// a parsed AST item, which always has matching delimiters.
    pub fn collect_tokens_trailing_token<R: HasAttrs + HasTokens>(
        &mut self,
        attrs: AttrWrapper,
        force_collect: ForceCollect,
        f: impl FnOnce(&mut Self, ast::AttrVec) -> PResult<'a, (R, TrailingToken)>,
    ) -> PResult<'a, R> {
        // We only bail out when nothing could possibly observe the collected tokens:
        // 1. We cannot be force collecting tokens (since force-collecting requires tokens
        //    by definition
        if matches!(force_collect, ForceCollect::No)
            // None of our outer attributes can require tokens (e.g. a proc-macro)
            && attrs.is_complete()
            // If our target supports custom inner attributes, then we cannot bail
            // out early, since we may need to capture tokens for a custom inner attribute
            // invocation.
            && !R::SUPPORTS_CUSTOM_INNER_ATTRS
            // Never bail out early in `capture_cfg` mode, since there might be `#[cfg]`
            // or `#[cfg_attr]` attributes.
            && !self.capture_cfg
        {
            return Ok(f(self, attrs.attrs)?.0);
        }

        let start_token = (self.token.clone(), self.token_spacing);
        let cursor_snapshot = self.token_cursor.clone();
        let start_pos = self.num_bump_calls;
        let has_outer_attrs = !attrs.attrs.is_empty();
        let replace_ranges_start = self.capture_state.replace_ranges.len();

        let (mut ret, trailing) = {
            let prev_capturing = mem::replace(&mut self.capture_state.capturing, Capturing::Yes);
            let ret_and_trailing = f(self, attrs.attrs);
            self.capture_state.capturing = prev_capturing;
            ret_and_trailing?
        };

        // When we're not in `capture-cfg` mode, then bail out early if:
        // 1. Our target doesn't support tokens at all (e.g we're parsing an `NtIdent`)
        //    so there's nothing for us to do.
        // 2. Our target already has tokens set (e.g. we've parsed something
        //    like `#[my_attr] $item`). The actual parsing code takes care of
        //    prepending any attributes to the nonterminal, so we don't need to
        //    modify the already captured tokens.
        // Note that this check is independent of `force_collect`- if we already
        // have tokens, or can't even store them, then there's never a need to
        // force collection of new tokens.
        if !self.capture_cfg && matches!(ret.tokens_mut(), None | Some(Some(_))) {
            return Ok(ret);
        }

        // This is very similar to the bail out check at the start of this function.
        // Now that we've parsed an AST node, we have more information available.
        if matches!(force_collect, ForceCollect::No)
            // We now have inner attributes available, so this check is more precise
            // than `attrs.is_complete()` at the start of the function.
            // As a result, we don't need to check `R::SUPPORTS_CUSTOM_INNER_ATTRS`
            && crate::parser::attr::is_complete(ret.attrs())
            // Subtle: We call `has_cfg_or_cfg_attr` with the attrs from `ret`.
            // This ensures that we consider inner attributes (e.g. `#![cfg]`),
            // which require us to have tokens available
            // We also call `has_cfg_or_cfg_attr` at the beginning of this function,
            // but we only bail out if there's no possibility of inner attributes
            // (!R::SUPPORTS_CUSTOM_INNER_ATTRS)
            // We only capture about `#[cfg]` or `#[cfg_attr]` in `capture_cfg`
            // mode - during normal parsing, we don't need any special capturing
            // for those attributes, since they're builtin.
            && !(self.capture_cfg && has_cfg_or_cfg_attr(ret.attrs()))
        {
            return Ok(ret);
        }

        let mut inner_attr_replace_ranges = Vec::new();
        // Take the captured ranges for any inner attributes that we parsed.
        for inner_attr in ret.attrs().iter().filter(|a| a.style == ast::AttrStyle::Inner) {
            if let Some(attr_range) = self.capture_state.inner_attr_ranges.remove(&inner_attr.id) {
                inner_attr_replace_ranges.push(attr_range);
            } else {
                self.dcx().span_delayed_bug(inner_attr.span, "Missing token range for attribute");
            }
        }

        let replace_ranges_end = self.capture_state.replace_ranges.len();

        // Capture a trailing token if requested by the callback 'f'
        let captured_trailing = match trailing {
            TrailingToken::None => false,
            TrailingToken::Gt => {
                assert_eq!(self.token.kind, token::Gt);
                false
            }
            TrailingToken::Semi => {
                assert_eq!(self.token.kind, token::Semi);
                true
            }
            TrailingToken::MaybeComma => self.token.kind == token::Comma,
        };

        assert!(
            !(self.break_last_token && captured_trailing),
            "Cannot set break_last_token and have trailing token"
        );

        let end_pos = self.num_bump_calls
            + captured_trailing as u32
            // If we 'broke' the last token (e.g. breaking a '>>' token to two '>' tokens), then
            // extend the range of captured tokens to include it, since the parser was not actually
            // bumped past it. When the `LazyAttrTokenStream` gets converted into an
            // `AttrTokenStream`, we will create the proper token.
            + self.break_last_token as u32;

        let num_calls = end_pos - start_pos;

        // If we have no attributes, then we will never need to
        // use any replace ranges.
        let replace_ranges: Box<[ReplaceRange]> = if ret.attrs().is_empty() && !self.capture_cfg {
            Box::new([])
        } else {
            // Grab any replace ranges that occur *inside* the current AST node.
            // We will perform the actual replacement when we convert the `LazyAttrTokenStream`
            // to an `AttrTokenStream`.
            self.capture_state.replace_ranges[replace_ranges_start..replace_ranges_end]
                .iter()
                .cloned()
                .chain(inner_attr_replace_ranges.iter().cloned())
                .map(|(range, data)| ((range.start - start_pos)..(range.end - start_pos), data))
                .collect()
        };

        let tokens = LazyAttrTokenStream::new_pending(
            start_token,
            cursor_snapshot,
            num_calls,
            self.break_last_token,
            replace_ranges,
        );

        // If we support tokens and don't already have them, store the newly captured tokens.
        if let Some(target_tokens @ None) = ret.tokens_mut() {
            *target_tokens = Some(tokens.clone());
        }

        let final_attrs = ret.attrs();

        // If `capture_cfg` is set and we're inside a recursive call to
        // `collect_tokens_trailing_token`, then we need to register a replace range
        // if we have `#[cfg]` or `#[cfg_attr]`. This allows us to run eager cfg-expansion
        // on the captured token stream.
        if self.capture_cfg
            && matches!(self.capture_state.capturing, Capturing::Yes)
            && has_cfg_or_cfg_attr(final_attrs)
        {
            assert!(!self.break_last_token, "Should not have unglued last token with cfg attr");

            // Replace the entire AST node that we just parsed, including attributes, with
            // `target`. If this AST node is inside an item that has `#[derive]`, then this will
            // allow us to cfg-expand this AST node.
            let start_pos = if has_outer_attrs { attrs.start_pos } else { start_pos };
            let target = AttrsTarget { attrs: final_attrs.iter().cloned().collect(), tokens };
            self.capture_state.replace_ranges.push((start_pos..end_pos, Some(target)));
            self.capture_state.replace_ranges.extend(inner_attr_replace_ranges);
        }

        // Only clear our `replace_ranges` when we're finished capturing entirely.
        if matches!(self.capture_state.capturing, Capturing::No) {
            self.capture_state.replace_ranges.clear();
            // We don't clear `inner_attr_ranges`, as doing so repeatedly
            // had a measurable performance impact. Most inner attributes that
            // we insert will get removed - when we drop the parser, we'll free
            // up the memory used by any attributes that we didn't remove from the map.
        }
        Ok(ret)
    }
}

// Some types are used a lot. Make sure they don't unintentionally get bigger.
#[cfg(target_pointer_width = "64")]
mod size_asserts {
    use super::*;
    use rustc_data_structures::static_assert_size;
    // tidy-alphabetical-start
    static_assert_size!(AttrWrapper, 16);
    // tidy-alphabetical-end
}
