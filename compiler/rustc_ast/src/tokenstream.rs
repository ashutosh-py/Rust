//! # Token Streams
//!
//! `TokenStream`s represent syntactic objects before they are converted into ASTs.
//! A `TokenStream` is, roughly speaking, a sequence of [`TokenTree`]s,
//! which are themselves a single [`Token`] or a `Delimited` subsequence of tokens.
//!
//! ## Ownership
//!
//! `TokenStream`s are persistent data structures constructed as ropes with reference
//! counted-children. In general, this means that calling an operation on a `TokenStream`
//! (such as `slice`) produces an entirely new `TokenStream` from the borrowed reference to
//! the original. This essentially coerces `TokenStream`s into "views" of their subparts,
//! and a borrowed `TokenStream` is sufficient to build an owned `TokenStream` without taking
//! ownership of the original.

use crate::ast::{AttrStyle, StmtKind};
use crate::ast_traits::{HasAttrs, HasTokens};
use crate::token::{self, Delimiter, Nonterminal, Token, TokenKind};
use crate::AttrVec;

use rustc_data_structures::stable_hasher::{HashStable, StableHasher};
use rustc_data_structures::sync::{self, Lrc};
use rustc_macros::{Decodable, Encodable, HashStable_Generic};
use rustc_serialize::{Decodable, Encodable};
use rustc_span::{sym, Span, SpanDecoder, SpanEncoder, Symbol, DUMMY_SP};

use std::borrow::Cow;
use std::ops::Range;
use std::{cmp, fmt, iter, mem};

/// Part of a `TokenStream`.
#[derive(Debug, Clone, PartialEq, Encodable, Decodable, HashStable_Generic)]
pub enum TokenTree {
    /// A single token. Should never be `OpenDelim` or `CloseDelim`, because
    /// delimiters are implicitly represented by `Delimited`.
    Token(Token, Spacing),
    /// A delimited sequence of token trees.
    Delimited(DelimSpan, DelimSpacing, Delimiter, TokenStream),
}

// Ensure all fields of `TokenTree` are `DynSend` and `DynSync`.
#[cfg(parallel_compiler)]
fn _dummy()
where
    Token: sync::DynSend + sync::DynSync,
    Spacing: sync::DynSend + sync::DynSync,
    DelimSpan: sync::DynSend + sync::DynSync,
    Delimiter: sync::DynSend + sync::DynSync,
    TokenStream: sync::DynSend + sync::DynSync,
{
}

impl TokenTree {
    /// Checks if this `TokenTree` is equal to the other, regardless of span/spacing information.
    pub fn eq_unspanned(&self, other: &TokenTree) -> bool {
        match (self, other) {
            (TokenTree::Token(token, _), TokenTree::Token(token2, _)) => token.kind == token2.kind,
            (TokenTree::Delimited(.., delim, tts), TokenTree::Delimited(.., delim2, tts2)) => {
                delim == delim2 && tts.eq_unspanned(tts2)
            }
            _ => false,
        }
    }

    /// Retrieves the `TokenTree`'s span.
    pub fn span(&self) -> Span {
        match self {
            TokenTree::Token(token, _) => token.span,
            TokenTree::Delimited(sp, ..) => sp.entire(),
        }
    }

    /// Create a `TokenTree::Token` with alone spacing.
    pub fn token_alone(kind: TokenKind, span: Span) -> TokenTree {
        TokenTree::Token(Token::new(kind, span), Spacing::Alone)
    }

    /// Create a `TokenTree::Token` with joint spacing.
    pub fn token_joint(kind: TokenKind, span: Span) -> TokenTree {
        TokenTree::Token(Token::new(kind, span), Spacing::Joint)
    }

    /// Create a `TokenTree::Token` with joint-hidden spacing.
    pub fn token_joint_hidden(kind: TokenKind, span: Span) -> TokenTree {
        TokenTree::Token(Token::new(kind, span), Spacing::JointHidden)
    }

    pub fn uninterpolate(&self) -> Cow<'_, TokenTree> {
        match self {
            TokenTree::Token(token, spacing) => match token.uninterpolate() {
                Cow::Owned(token) => Cow::Owned(TokenTree::Token(token, *spacing)),
                Cow::Borrowed(_) => Cow::Borrowed(self),
            },
            _ => Cow::Borrowed(self),
        }
    }
}

impl<CTX> HashStable<CTX> for TokenStream
where
    CTX: crate::HashStableContext,
{
    fn hash_stable(&self, hcx: &mut CTX, hasher: &mut StableHasher) {
        for sub_tt in self.trees() {
            sub_tt.hash_stable(hcx, hasher);
        }
    }
}

pub trait ToAttrTokenStream: sync::DynSend + sync::DynSync {
    fn to_attr_token_stream(&self) -> AttrTokenStream;
}

impl ToAttrTokenStream for AttrTokenStream {
    fn to_attr_token_stream(&self) -> AttrTokenStream {
        self.clone()
    }
}

/// A lazy version of [`TokenStream`], which defers creation
/// of an actual `TokenStream` until it is needed.
/// `Box` is here only to reduce the structure size.
#[derive(Clone)]
pub struct LazyAttrTokenStream(Lrc<Box<dyn ToAttrTokenStream>>);

impl LazyAttrTokenStream {
    pub fn new(inner: impl ToAttrTokenStream + 'static) -> LazyAttrTokenStream {
        LazyAttrTokenStream(Lrc::new(Box::new(inner)))
    }

    pub fn to_attr_token_stream(&self) -> AttrTokenStream {
        self.0.to_attr_token_stream()
    }
}

impl fmt::Debug for LazyAttrTokenStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LazyAttrTokenStream({:?})", self.to_attr_token_stream())
    }
}

impl<S: SpanEncoder> Encodable<S> for LazyAttrTokenStream {
    fn encode(&self, _s: &mut S) {
        panic!("Attempted to encode LazyAttrTokenStream");
    }
}

impl<D: SpanDecoder> Decodable<D> for LazyAttrTokenStream {
    fn decode(_d: &mut D) -> Self {
        panic!("Attempted to decode LazyAttrTokenStream");
    }
}

impl<CTX> HashStable<CTX> for LazyAttrTokenStream {
    fn hash_stable(&self, _hcx: &mut CTX, _hasher: &mut StableHasher) {
        panic!("Attempted to compute stable hash for LazyAttrTokenStream");
    }
}

/// Indicates a range of tokens that should be replaced by the tokens in the
/// provided `AttrsTarget`. This is used in two places during token collection:
///
/// 1. During the parsing of an AST node that may have a `#[derive]` attribute,
///    we parse a nested AST node that has `#[cfg]` or `#[cfg_attr]` In this
///    case, we use a `ReplaceRange` to replace the entire inner AST node with
///    `FlatToken::AttrsTarget`, allowing us to perform eager cfg-expansion on
///    an `AttrTokenStream`.
///
/// 2. When we parse an inner attribute while collecting tokens. We remove
///    inner attributes from the token stream entirely, and instead track them
///    through the `attrs` field on the AST node. This allows us to easily
///    manipulate them (for example, removing the first macro inner attribute
///    to invoke a proc-macro). When create a `TokenStream`, the inner
///    attributes get inserted into the proper place in the token stream.
pub type ReplaceRange = (Range<u32>, Option<AttrsTarget>);

// Produces a `TokenStream` on-demand. Using `cursor_snapshot` and `num_calls`,
// we can reconstruct the `TokenStream` seen by the callback. This allows us to
// avoid producing a `TokenStream` if it is never needed - for example, a
// captured `macro_rules!` argument that is never passed to a proc macro. In
// practice token stream creation happens rarely compared to calls to
// `collect_tokens` (see some statistics in #78736), so we are doing as little
// up-front work as possible.
//
// This also makes `Parser` very cheap to clone, since there is no intermediate
// collection buffer to clone.
pub struct LazyAttrTokenStreamImpl {
    pub start_token: (Token, Spacing),
    pub cursor_snapshot: TokenCursor,
    pub num_calls: u32,
    pub break_last_token: bool,
    pub replace_ranges: Box<[ReplaceRange]>,
}

impl ToAttrTokenStream for LazyAttrTokenStreamImpl {
    fn to_attr_token_stream(&self) -> AttrTokenStream {
        // The token produced by the final call to `{,inlined_}next` was not
        // actually consumed by the callback. The combination of chaining the
        // initial token and using `take` produces the desired result - we
        // produce an empty `TokenStream` if no calls were made, and omit the
        // final token otherwise.
        let mut cursor_snapshot = self.cursor_snapshot.clone();
        let tokens = iter::once(FlatToken::Token(self.start_token.clone()))
            .chain(iter::repeat_with(|| FlatToken::Token(cursor_snapshot.next())))
            .take(self.num_calls as usize);

        if self.replace_ranges.is_empty() {
            make_attr_token_stream(tokens, self.break_last_token)
        } else {
            let mut tokens: Vec<_> = tokens.collect();
            let mut replace_ranges = self.replace_ranges.to_vec();
            replace_ranges.sort_by_key(|(range, _)| range.start);

            #[cfg(debug_assertions)]
            {
                for [(range, tokens), (next_range, next_tokens)] in replace_ranges.array_windows() {
                    assert!(
                        range.end <= next_range.start || range.end >= next_range.end,
                        "Replace ranges should either be disjoint or nested: \
                         ({:?}, {:?}) ({:?}, {:?})",
                        range,
                        tokens,
                        next_range,
                        next_tokens,
                    );
                }
            }

            // Process the replace ranges, starting from the highest start
            // position and working our way back. If have tokens like:
            //
            // `#[cfg(FALSE)] struct Foo { #[cfg(FALSE)] field: bool }`
            //
            // Then we will generate replace ranges for both the `#[cfg(FALSE)]
            // field: bool` and the entire `#[cfg(FALSE)] struct Foo {
            // #[cfg(FALSE)] field: bool }`
            //
            // By starting processing from the replace range with the greatest
            // start position, we ensure that any replace range which encloses
            // another replace range will capture the *replaced* tokens for the
            // inner range, not the original tokens.
            for (range, target) in replace_ranges.into_iter().rev() {
                assert!(!range.is_empty(), "Cannot replace an empty range: {range:?}");

                // Replace the tokens in range with zero or one
                // `FlatToken::AttrsTarget`s, plus enough `FlatToken::Empty`s
                // to fill up the rest of the range. This keeps the total
                // length of `tokens` constant throughout the replacement
                // process, allowing us to use all of the `ReplaceRanges`
                // entries without adjusting indices.
                let target_len = target.is_some() as usize;
                tokens.splice(
                    (range.start as usize)..(range.end as usize),
                    target
                        .into_iter()
                        .map(|target| FlatToken::AttrsTarget(target))
                        .chain(iter::repeat(FlatToken::Empty).take(range.len() - target_len)),
                );
            }
            make_attr_token_stream(tokens.into_iter(), self.break_last_token)
        }
    }
}

/// A helper struct used when building an `AttrTokenStream` from a
/// `LazyAttrTokenStream`. Both delimiter and non-delimited tokens are stored
/// as `FlatToken::Token`. A vector of `FlatToken`s is then 'parsed' to build
/// up an `AttrTokenStream` with nested `AttrTokenTree::Delimited` tokens.
#[derive(Debug, Clone)]
enum FlatToken {
    /// A token. This holds both delimiter (e.g. '{' and '}') and non-delimiter
    /// tokens.
    Token((Token, Spacing)),
    /// Holds the `AttrsTarget` for an AST node. The `AttrsTarget` is inserted
    /// directly into the constructed `AttrTokenStream` as an
    /// `AttrTokenTree::AttrsTarget`.
    AttrsTarget(AttrsTarget),
    /// A special 'empty' token that is ignored during the conversion to an
    /// `AttrTokenStream`. This is used to simplify the handling of replace
    /// ranges.
    Empty,
}

/// Converts a flattened iterator of tokens (including open and close delimiter
/// tokens) into an `AttrTokenStream`, creating an `AttrTokenTree::Delimited`
/// for each matching pair of open and close delims.
fn make_attr_token_stream(
    iter: impl Iterator<Item = FlatToken>,
    break_last_token: bool,
) -> AttrTokenStream {
    #[derive(Debug)]
    struct FrameData {
        // This is `None` for the first frame, `Some` for all others.
        open_delim_sp: Option<(Delimiter, Span, Spacing)>,
        inner: Vec<AttrTokenTree>,
    }
    // The stack always has at least one element. Storing it separately makes for shorter code.
    let mut stack_top = FrameData { open_delim_sp: None, inner: vec![] };
    let mut stack_rest = vec![];
    for flat_token in iter {
        match flat_token {
            FlatToken::Token((Token { kind: TokenKind::OpenDelim(delim), span }, spacing)) => {
                stack_rest.push(mem::replace(
                    &mut stack_top,
                    FrameData { open_delim_sp: Some((delim, span, spacing)), inner: vec![] },
                ));
            }
            FlatToken::Token((Token { kind: TokenKind::CloseDelim(delim), span }, spacing)) => {
                let frame_data = mem::replace(&mut stack_top, stack_rest.pop().unwrap());
                let (open_delim, open_sp, open_spacing) = frame_data.open_delim_sp.unwrap();
                assert_eq!(
                    open_delim, delim,
                    "Mismatched open/close delims: open={open_delim:?} close={span:?}"
                );
                let dspan = DelimSpan::from_pair(open_sp, span);
                let dspacing = DelimSpacing::new(open_spacing, spacing);
                let stream = AttrTokenStream::new(frame_data.inner);
                let delimited = AttrTokenTree::Delimited(dspan, dspacing, delim, stream);
                stack_top.inner.push(delimited);
            }
            FlatToken::Token((token, spacing)) => {
                stack_top.inner.push(AttrTokenTree::Token(token, spacing))
            }
            FlatToken::AttrsTarget(target) => {
                stack_top.inner.push(AttrTokenTree::AttrsTarget(target))
            }
            FlatToken::Empty => {}
        }
    }

    if break_last_token {
        let last_token = stack_top.inner.pop().unwrap();
        if let AttrTokenTree::Token(last_token, spacing) = last_token {
            let unglued_first = last_token.kind.break_two_token_op().unwrap().0;

            // An 'unglued' token is always two ASCII characters.
            let mut first_span = last_token.span.shrink_to_lo();
            first_span = first_span.with_hi(first_span.lo() + rustc_span::BytePos(1));

            stack_top
                .inner
                .push(AttrTokenTree::Token(Token::new(unglued_first, first_span), spacing));
        } else {
            panic!("Unexpected last token {last_token:?}")
        }
    }
    AttrTokenStream::new(stack_top.inner)
}

/// Like `TokenTree`, but for `AttrTokenStream`.
#[derive(Clone, Debug, Encodable, Decodable)]
pub enum AttrTokenTree {
    Token(Token, Spacing),
    Delimited(DelimSpan, DelimSpacing, Delimiter, AttrTokenStream),
    /// Stores the attributes for an attribute target,
    /// along with the tokens for that attribute target.
    /// See `AttrsTarget` for more information
    AttrsTarget(AttrsTarget),
}

/// An `AttrTokenStream` is similar to a `TokenStream`, but with extra
/// information about the tokens for attribute targets. This is used
/// during expansion to perform early cfg-expansion, and to process attributes
/// during proc-macro invocations.
#[derive(Clone, Debug, Default, Encodable, Decodable)]
pub struct AttrTokenStream(pub Lrc<Vec<AttrTokenTree>>);

impl AttrTokenStream {
    pub fn new(tokens: Vec<AttrTokenTree>) -> AttrTokenStream {
        AttrTokenStream(Lrc::new(tokens))
    }

    /// Converts this `AttrTokenStream` to a plain `Vec<TokenTree>`.
    /// During conversion, `AttrTokenTree::AttrsTarget` get 'flattened'
    /// back to a `TokenStream` of the form `outer_attr attr_target`.
    /// If there are inner attributes, they are inserted into the proper
    /// place in the attribute target tokens.
    pub fn to_token_trees(&self) -> Vec<TokenTree> {
        let mut res = Vec::with_capacity(self.0.len());
        for tree in self.0.iter() {
            match tree {
                AttrTokenTree::Token(inner, spacing) => {
                    res.push(TokenTree::Token(inner.clone(), *spacing));
                }
                AttrTokenTree::Delimited(span, spacing, delim, stream) => {
                    res.push(TokenTree::Delimited(
                        *span,
                        *spacing,
                        *delim,
                        TokenStream::new(stream.to_token_trees()),
                    ))
                }
                AttrTokenTree::AttrsTarget(target) => {
                    let idx = target
                        .attrs
                        .partition_point(|attr| matches!(attr.style, crate::AttrStyle::Outer));
                    let (outer_attrs, inner_attrs) = target.attrs.split_at(idx);

                    let mut target_tokens = target.tokens.to_attr_token_stream().to_token_trees();
                    if !inner_attrs.is_empty() {
                        let mut found = false;
                        // Check the last two trees (to account for a trailing semi)
                        for tree in target_tokens.iter_mut().rev().take(2) {
                            if let TokenTree::Delimited(span, spacing, delim, delim_tokens) = tree {
                                // Inner attributes are only supported on extern blocks, functions,
                                // impls, and modules. All of these have their inner attributes
                                // placed at the beginning of the rightmost outermost braced group:
                                // e.g. fn foo() { #![my_attr] }
                                //
                                // Therefore, we can insert them back into the right location
                                // without needing to do any extra position tracking.
                                //
                                // Note: Outline modules are an exception - they can
                                // have attributes like `#![my_attr]` at the start of a file.
                                // Support for custom attributes in this position is not
                                // properly implemented - we always synthesize fake tokens,
                                // so we never reach this code.

                                let mut stream = TokenStream::default();
                                for inner_attr in inner_attrs {
                                    stream.push_stream(inner_attr.get_tokens());
                                }
                                stream.push_stream(delim_tokens.clone());
                                *tree = TokenTree::Delimited(*span, *spacing, *delim, stream);
                                found = true;
                                break;
                            }
                        }

                        assert!(
                            found,
                            "Failed to find trailing delimited group in: {target_tokens:?}"
                        );
                    }
                    for attr in outer_attrs {
                        res.extend(attr.get_tokens().0.iter().cloned());
                    }
                    res.extend(target_tokens);
                }
            }
        }
        res
    }
}

/// Stores the tokens for an attribute target, along
/// with its attributes.
///
/// This is constructed during parsing when we need to capture
/// tokens.
///
/// For example, `#[cfg(FALSE)] struct Foo {}` would
/// have an `attrs` field containing the `#[cfg(FALSE)]` attr,
/// and a `tokens` field storing the (unparsed) tokens `struct Foo {}`
#[derive(Clone, Debug, Encodable, Decodable)]
pub struct AttrsTarget {
    /// Attributes, both outer and inner.
    /// These are stored in the original order that they were parsed in.
    pub attrs: AttrVec,
    /// The underlying tokens for the attribute target that `attrs`
    /// are applied to
    pub tokens: LazyAttrTokenStream,
}

/// A `TokenStream` is an abstract sequence of tokens, organized into [`TokenTree`]s.
///
/// The goal is for procedural macros to work with `TokenStream`s and `TokenTree`s
/// instead of a representation of the abstract syntax tree.
/// Today's `TokenTree`s can still contain AST via `token::Interpolated` for
/// backwards compatibility.
#[derive(Clone, Debug, Default, Encodable, Decodable)]
pub struct TokenStream(pub(crate) Lrc<Vec<TokenTree>>);

/// Indicates whether a token can join with the following token to form a
/// compound token. Used for conversions to `proc_macro::Spacing`. Also used to
/// guide pretty-printing, which is where the `JointHidden` value (which isn't
/// part of `proc_macro::Spacing`) comes in useful.
#[derive(Clone, Copy, Debug, PartialEq, Encodable, Decodable, HashStable_Generic)]
pub enum Spacing {
    /// The token cannot join with the following token to form a compound
    /// token.
    ///
    /// In token streams parsed from source code, the compiler will use `Alone`
    /// for any token immediately followed by whitespace, a non-doc comment, or
    /// EOF.
    ///
    /// When constructing token streams within the compiler, use this for each
    /// token that (a) should be pretty-printed with a space after it, or (b)
    /// is the last token in the stream. (In the latter case the choice of
    /// spacing doesn't matter because it is never used for the last token. We
    /// arbitrarily use `Alone`.)
    ///
    /// Converts to `proc_macro::Spacing::Alone`, and
    /// `proc_macro::Spacing::Alone` converts back to this.
    Alone,

    /// The token can join with the following token to form a compound token.
    ///
    /// In token streams parsed from source code, the compiler will use `Joint`
    /// for any token immediately followed by punctuation (as determined by
    /// `Token::is_punct`).
    ///
    /// When constructing token streams within the compiler, use this for each
    /// token that (a) should be pretty-printed without a space after it, and
    /// (b) is followed by a punctuation token.
    ///
    /// Converts to `proc_macro::Spacing::Joint`, and
    /// `proc_macro::Spacing::Joint` converts back to this.
    Joint,

    /// The token can join with the following token to form a compound token,
    /// but this will not be visible at the proc macro level. (This is what the
    /// `Hidden` means; see below.)
    ///
    /// In token streams parsed from source code, the compiler will use
    /// `JointHidden` for any token immediately followed by anything not
    /// covered by the `Alone` and `Joint` cases: an identifier, lifetime,
    /// literal, delimiter, doc comment.
    ///
    /// When constructing token streams, use this for each token that (a)
    /// should be pretty-printed without a space after it, and (b) is followed
    /// by a non-punctuation token.
    ///
    /// Converts to `proc_macro::Spacing::Alone`, but
    /// `proc_macro::Spacing::Alone` converts back to `token::Spacing::Alone`.
    /// Because of that, pretty-printing of `TokenStream`s produced by proc
    /// macros is unavoidably uglier (with more whitespace between tokens) than
    /// pretty-printing of `TokenStream`'s produced by other means (i.e. parsed
    /// source code, internally constructed token streams, and token streams
    /// produced by declarative macros).
    JointHidden,
}

impl TokenStream {
    /// Given a `TokenStream` with a `Stream` of only two arguments, return a new `TokenStream`
    /// separating the two arguments with a comma for diagnostic suggestions.
    pub fn add_comma(&self) -> Option<(TokenStream, Span)> {
        // Used to suggest if a user writes `foo!(a b);`
        let mut suggestion = None;
        let mut iter = self.0.iter().enumerate().peekable();
        while let Some((pos, ts)) = iter.next() {
            if let Some((_, next)) = iter.peek() {
                let sp = match (&ts, &next) {
                    (_, TokenTree::Token(Token { kind: token::Comma, .. }, _)) => continue,
                    (
                        TokenTree::Token(token_left, Spacing::Alone),
                        TokenTree::Token(token_right, _),
                    ) if ((token_left.is_ident() && !token_left.is_reserved_ident())
                        || token_left.is_lit())
                        && ((token_right.is_ident() && !token_right.is_reserved_ident())
                            || token_right.is_lit()) =>
                    {
                        token_left.span
                    }
                    (TokenTree::Delimited(sp, ..), _) => sp.entire(),
                    _ => continue,
                };
                let sp = sp.shrink_to_hi();
                let comma = TokenTree::token_alone(token::Comma, sp);
                suggestion = Some((pos, comma, sp));
            }
        }
        if let Some((pos, comma, sp)) = suggestion {
            let mut new_stream = Vec::with_capacity(self.0.len() + 1);
            let parts = self.0.split_at(pos + 1);
            new_stream.extend_from_slice(parts.0);
            new_stream.push(comma);
            new_stream.extend_from_slice(parts.1);
            return Some((TokenStream::new(new_stream), sp));
        }
        None
    }
}

impl FromIterator<TokenTree> for TokenStream {
    fn from_iter<I: IntoIterator<Item = TokenTree>>(iter: I) -> Self {
        TokenStream::new(iter.into_iter().collect::<Vec<TokenTree>>())
    }
}

impl Eq for TokenStream {}

impl PartialEq<TokenStream> for TokenStream {
    fn eq(&self, other: &TokenStream) -> bool {
        self.trees().eq(other.trees())
    }
}

impl TokenStream {
    pub fn new(tts: Vec<TokenTree>) -> TokenStream {
        TokenStream(Lrc::new(tts))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn trees(&self) -> RefTokenTreeCursor<'_> {
        RefTokenTreeCursor::new(self)
    }

    pub fn into_trees(self) -> TokenTreeCursor {
        TokenTreeCursor::new(self)
    }

    /// Compares two `TokenStream`s, checking equality without regarding span information.
    pub fn eq_unspanned(&self, other: &TokenStream) -> bool {
        let mut t1 = self.trees();
        let mut t2 = other.trees();
        for (t1, t2) in iter::zip(&mut t1, &mut t2) {
            if !t1.eq_unspanned(t2) {
                return false;
            }
        }
        t1.next().is_none() && t2.next().is_none()
    }

    /// Create a token stream containing a single token with alone spacing. The
    /// spacing used for the final token in a constructed stream doesn't matter
    /// because it's never used. In practice we arbitrarily use
    /// `Spacing::Alone`.
    pub fn token_alone(kind: TokenKind, span: Span) -> TokenStream {
        TokenStream::new(vec![TokenTree::token_alone(kind, span)])
    }

    pub fn from_ast(node: &(impl HasAttrs + HasTokens + fmt::Debug)) -> TokenStream {
        let Some(tokens) = node.tokens() else {
            panic!("missing tokens for node: {:?}", node);
        };
        let attrs = node.attrs();
        let attr_stream = if attrs.is_empty() {
            tokens.to_attr_token_stream()
        } else {
            let target =
                AttrsTarget { attrs: attrs.iter().cloned().collect(), tokens: tokens.clone() };
            AttrTokenStream::new(vec![AttrTokenTree::AttrsTarget(target)])
        };
        TokenStream::new(attr_stream.to_token_trees())
    }

    pub fn from_nonterminal_ast(nt: &Nonterminal) -> TokenStream {
        match nt {
            Nonterminal::NtItem(item) => TokenStream::from_ast(item),
            Nonterminal::NtBlock(block) => TokenStream::from_ast(block),
            Nonterminal::NtStmt(stmt) if let StmtKind::Empty = stmt.kind => {
                // FIXME: Properly collect tokens for empty statements.
                TokenStream::token_alone(token::Semi, stmt.span)
            }
            Nonterminal::NtStmt(stmt) => TokenStream::from_ast(stmt),
            Nonterminal::NtPat(pat) => TokenStream::from_ast(pat),
            Nonterminal::NtTy(ty) => TokenStream::from_ast(ty),
            Nonterminal::NtMeta(attr) => TokenStream::from_ast(attr),
            Nonterminal::NtPath(path) => TokenStream::from_ast(path),
            Nonterminal::NtVis(vis) => TokenStream::from_ast(vis),
            Nonterminal::NtExpr(expr) | Nonterminal::NtLiteral(expr) => TokenStream::from_ast(expr),
        }
    }

    fn flatten_token(token: &Token, spacing: Spacing) -> TokenTree {
        match token.kind {
            token::NtIdent(ident, is_raw) => {
                TokenTree::Token(Token::new(token::Ident(ident.name, is_raw), ident.span), spacing)
            }
            token::NtLifetime(ident) => TokenTree::Delimited(
                DelimSpan::from_single(token.span),
                DelimSpacing::new(Spacing::JointHidden, spacing),
                Delimiter::Invisible,
                TokenStream::token_alone(token::Lifetime(ident.name), ident.span),
            ),
            token::Interpolated(ref nt) => TokenTree::Delimited(
                DelimSpan::from_single(token.span),
                DelimSpacing::new(Spacing::JointHidden, spacing),
                Delimiter::Invisible,
                TokenStream::from_nonterminal_ast(&nt).flattened(),
            ),
            _ => TokenTree::Token(token.clone(), spacing),
        }
    }

    fn flatten_token_tree(tree: &TokenTree) -> TokenTree {
        match tree {
            TokenTree::Token(token, spacing) => TokenStream::flatten_token(token, *spacing),
            TokenTree::Delimited(span, spacing, delim, tts) => {
                TokenTree::Delimited(*span, *spacing, *delim, tts.flattened())
            }
        }
    }

    #[must_use]
    pub fn flattened(&self) -> TokenStream {
        fn can_skip(stream: &TokenStream) -> bool {
            stream.trees().all(|tree| match tree {
                TokenTree::Token(token, _) => !matches!(
                    token.kind,
                    token::NtIdent(..) | token::NtLifetime(..) | token::Interpolated(..)
                ),
                TokenTree::Delimited(.., inner) => can_skip(inner),
            })
        }

        if can_skip(self) {
            return self.clone();
        }

        self.trees().map(|tree| TokenStream::flatten_token_tree(tree)).collect()
    }

    // If `vec` is not empty, try to glue `tt` onto its last token. The return
    // value indicates if gluing took place.
    fn try_glue_to_last(vec: &mut Vec<TokenTree>, tt: &TokenTree) -> bool {
        if let Some(TokenTree::Token(last_tok, Spacing::Joint | Spacing::JointHidden)) = vec.last()
            && let TokenTree::Token(tok, spacing) = tt
            && let Some(glued_tok) = last_tok.glue(tok)
        {
            // ...then overwrite the last token tree in `vec` with the
            // glued token, and skip the first token tree from `stream`.
            *vec.last_mut().unwrap() = TokenTree::Token(glued_tok, *spacing);
            true
        } else {
            false
        }
    }

    /// Push `tt` onto the end of the stream, possibly gluing it to the last
    /// token. Uses `make_mut` to maximize efficiency.
    pub fn push_tree(&mut self, tt: TokenTree) {
        let vec_mut = Lrc::make_mut(&mut self.0);

        if Self::try_glue_to_last(vec_mut, &tt) {
            // nothing else to do
        } else {
            vec_mut.push(tt);
        }
    }

    /// Push `stream` onto the end of the stream, possibly gluing the first
    /// token tree to the last token. (No other token trees will be glued.)
    /// Uses `make_mut` to maximize efficiency.
    pub fn push_stream(&mut self, stream: TokenStream) {
        let vec_mut = Lrc::make_mut(&mut self.0);

        let stream_iter = stream.0.iter().cloned();

        if let Some(first) = stream.0.first()
            && Self::try_glue_to_last(vec_mut, first)
        {
            // Now skip the first token tree from `stream`.
            vec_mut.extend(stream_iter.skip(1));
        } else {
            // Append all of `stream`.
            vec_mut.extend(stream_iter);
        }
    }

    pub fn chunks(&self, chunk_size: usize) -> core::slice::Chunks<'_, TokenTree> {
        self.0.chunks(chunk_size)
    }

    /// Desugar doc comments like `/// foo` in the stream into `#[doc =
    /// r"foo"]`. Modifies the `TokenStream` via `Lrc::make_mut`, but as little
    /// as possible.
    pub fn desugar_doc_comments(&mut self) {
        if let Some(desugared_stream) = desugar_inner(self.clone()) {
            *self = desugared_stream;
        }

        // The return value is `None` if nothing in `stream` changed.
        fn desugar_inner(mut stream: TokenStream) -> Option<TokenStream> {
            let mut i = 0;
            let mut modified = false;
            while let Some(tt) = stream.0.get(i) {
                match tt {
                    &TokenTree::Token(
                        Token { kind: token::DocComment(_, attr_style, data), span },
                        _spacing,
                    ) => {
                        let desugared = desugared_tts(attr_style, data, span);
                        let desugared_len = desugared.len();
                        Lrc::make_mut(&mut stream.0).splice(i..i + 1, desugared);
                        modified = true;
                        i += desugared_len;
                    }

                    &TokenTree::Token(..) => i += 1,

                    &TokenTree::Delimited(sp, spacing, delim, ref delim_stream) => {
                        if let Some(desugared_delim_stream) = desugar_inner(delim_stream.clone()) {
                            let new_tt =
                                TokenTree::Delimited(sp, spacing, delim, desugared_delim_stream);
                            Lrc::make_mut(&mut stream.0)[i] = new_tt;
                            modified = true;
                        }
                        i += 1;
                    }
                }
            }
            if modified { Some(stream) } else { None }
        }

        fn desugared_tts(attr_style: AttrStyle, data: Symbol, span: Span) -> Vec<TokenTree> {
            // Searches for the occurrences of `"#*` and returns the minimum number of `#`s
            // required to wrap the text. E.g.
            // - `abc d` is wrapped as `r"abc d"` (num_of_hashes = 0)
            // - `abc "d"` is wrapped as `r#"abc "d""#` (num_of_hashes = 1)
            // - `abc "##d##"` is wrapped as `r###"abc ##"d"##"###` (num_of_hashes = 3)
            let mut num_of_hashes = 0;
            let mut count = 0;
            for ch in data.as_str().chars() {
                count = match ch {
                    '"' => 1,
                    '#' if count > 0 => count + 1,
                    _ => 0,
                };
                num_of_hashes = cmp::max(num_of_hashes, count);
            }

            // `/// foo` becomes `[doc = r"foo"]`.
            let delim_span = DelimSpan::from_single(span);
            let body = TokenTree::Delimited(
                delim_span,
                DelimSpacing::new(Spacing::JointHidden, Spacing::Alone),
                Delimiter::Bracket,
                [
                    TokenTree::token_alone(token::Ident(sym::doc, token::IdentIsRaw::No), span),
                    TokenTree::token_alone(token::Eq, span),
                    TokenTree::token_alone(
                        TokenKind::lit(token::StrRaw(num_of_hashes), data, None),
                        span,
                    ),
                ]
                .into_iter()
                .collect::<TokenStream>(),
            );

            if attr_style == AttrStyle::Inner {
                vec![
                    TokenTree::token_joint(token::Pound, span),
                    TokenTree::token_joint_hidden(token::Not, span),
                    body,
                ]
            } else {
                vec![TokenTree::token_joint_hidden(token::Pound, span), body]
            }
        }
    }
}

/// By-reference iterator over a [`TokenStream`], that produces `&TokenTree`
/// items.
#[derive(Clone)]
pub struct RefTokenTreeCursor<'t> {
    stream: &'t TokenStream,
    index: usize,
}

impl<'t> RefTokenTreeCursor<'t> {
    fn new(stream: &'t TokenStream) -> Self {
        RefTokenTreeCursor { stream, index: 0 }
    }

    pub fn look_ahead(&self, n: usize) -> Option<&TokenTree> {
        self.stream.0.get(self.index + n)
    }
}

impl<'t> Iterator for RefTokenTreeCursor<'t> {
    type Item = &'t TokenTree;

    fn next(&mut self) -> Option<&'t TokenTree> {
        self.stream.0.get(self.index).map(|tree| {
            self.index += 1;
            tree
        })
    }
}

/// Owning by-value iterator over a [`TokenStream`], that produces `&TokenTree`
/// items.
///
/// Doesn't impl `Iterator` because Rust doesn't permit an owning iterator to
/// return `&T` from `next`; the need for an explicit lifetime in the `Item`
/// associated type gets in the way. Instead, use `next_ref` (which doesn't
/// involve associated types) for getting individual elements, or
/// `RefTokenTreeCursor` if you really want an `Iterator`, e.g. in a `for`
/// loop.
#[derive(Clone, Debug)]
pub struct TokenTreeCursor {
    pub stream: TokenStream,
    index: usize,
}

impl TokenTreeCursor {
    fn new(stream: TokenStream) -> Self {
        TokenTreeCursor { stream, index: 0 }
    }

    #[inline]
    pub fn next_ref(&mut self) -> Option<&TokenTree> {
        self.stream.0.get(self.index).map(|tree| {
            self.index += 1;
            tree
        })
    }

    pub fn look_ahead(&self, n: usize) -> Option<&TokenTree> {
        self.stream.0.get(self.index + n)
    }
}

/// Iterator over a `TokenStream` that produces `Token`s. It's a bit odd that
/// we (a) lex tokens into a nice tree structure (`TokenStream`), and then (b)
/// use this type to emit them as a linear sequence. But a linear sequence is
/// what the parser expects, for the most part.
#[derive(Clone, Debug)]
pub struct TokenCursor {
    // Cursor for the current (innermost) token stream. The delimiters for this
    // token stream are found in `self.stack.last()`; when that is `None` then
    // we are in the outermost token stream which never has delimiters.
    pub tree_cursor: TokenTreeCursor,

    // Token streams surrounding the current one. The delimiters for stack[n]'s
    // tokens are in `stack[n-1]`. `stack[0]` (when present) has no delimiters
    // because it's the outermost token stream which never has delimiters.
    pub stack: Vec<(TokenTreeCursor, DelimSpan, DelimSpacing, Delimiter)>,
}

impl TokenCursor {
    pub fn next(&mut self) -> (Token, Spacing) {
        self.inlined_next()
    }

    /// This always-inlined version should only be used on hot code paths.
    #[inline(always)]
    pub fn inlined_next(&mut self) -> (Token, Spacing) {
        loop {
            // FIXME: we currently don't return `Delimiter::Invisible` open/close delims. To fix
            // #67062 we will need to, whereupon the `delim != Delimiter::Invisible` conditions
            // below can be removed.
            if let Some(tree) = self.tree_cursor.next_ref() {
                match tree {
                    &TokenTree::Token(ref token, spacing) => {
                        debug_assert!(!matches!(
                            token.kind,
                            token::OpenDelim(_) | token::CloseDelim(_)
                        ));
                        return (token.clone(), spacing);
                    }
                    &TokenTree::Delimited(sp, spacing, delim, ref tts) => {
                        let trees = tts.clone().into_trees();
                        self.stack.push((
                            mem::replace(&mut self.tree_cursor, trees),
                            sp,
                            spacing,
                            delim,
                        ));
                        if delim != Delimiter::Invisible {
                            return (Token::new(token::OpenDelim(delim), sp.open), spacing.open);
                        }
                        // No open delimiter to return; continue on to the next iteration.
                    }
                };
            } else if let Some((tree_cursor, span, spacing, delim)) = self.stack.pop() {
                // We have exhausted this token stream. Move back to its parent token stream.
                self.tree_cursor = tree_cursor;
                if delim != Delimiter::Invisible {
                    return (Token::new(token::CloseDelim(delim), span.close), spacing.close);
                }
                // No close delimiter to return; continue on to the next iteration.
            } else {
                // We have exhausted the outermost token stream. The use of
                // `Spacing::Alone` is arbitrary and immaterial, because the
                // `Eof` token's spacing is never used.
                return (Token::new(token::Eof, DUMMY_SP), Spacing::Alone);
            }
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Encodable, Decodable, HashStable_Generic)]
pub struct DelimSpan {
    pub open: Span,
    pub close: Span,
}

impl DelimSpan {
    pub fn from_single(sp: Span) -> Self {
        DelimSpan { open: sp, close: sp }
    }

    pub fn from_pair(open: Span, close: Span) -> Self {
        DelimSpan { open, close }
    }

    pub fn dummy() -> Self {
        Self::from_single(DUMMY_SP)
    }

    pub fn entire(self) -> Span {
        self.open.with_hi(self.close.hi())
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Encodable, Decodable, HashStable_Generic)]
pub struct DelimSpacing {
    pub open: Spacing,
    pub close: Spacing,
}

impl DelimSpacing {
    pub fn new(open: Spacing, close: Spacing) -> DelimSpacing {
        DelimSpacing { open, close }
    }
}

// Some types are used a lot. Make sure they don't unintentionally get bigger.
#[cfg(target_pointer_width = "64")]
mod size_asserts {
    use super::*;
    use rustc_data_structures::static_assert_size;
    // tidy-alphabetical-start
    static_assert_size!(AttrTokenStream, 8);
    static_assert_size!(AttrTokenTree, 32);
    static_assert_size!(LazyAttrTokenStream, 8);
    static_assert_size!(LazyAttrTokenStreamImpl, 96);
    static_assert_size!(Option<LazyAttrTokenStream>, 8); // must be small, used in many AST nodes
    static_assert_size!(TokenStream, 8);
    static_assert_size!(TokenTree, 32);
    // tidy-alphabetical-end
}
