// Copyright 2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use ast;
use syntax_pos::{Span, DUMMY_SP};
use ext::base::{DummyResult, ExtCtxt, MacResult, SyntaxExtension};
use ext::base::{NormalTT, TTMacroExpander};
use ext::tt::macro_parser::{Success, Error, Failure};
use ext::tt::macro_parser::{MatchedSeq, MatchedNonterminal};
use ext::tt::macro_parser::parse;
use parse::lexer::new_tt_reader;
use parse::parser::{Parser, Restrictions};
use parse::token::{self, gensym_ident, NtTT, Token};
use parse::token::Token::*;
use print;
use ptr::P;
use tokenstream::{self, TokenTree};

use util::small_vector::SmallVector;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::collections::hash_map::{Entry};
use std::rc::Rc;

struct ParserAnyMacro<'a> {
    parser: RefCell<Parser<'a>>,

    /// Span of the expansion site of the macro this parser is for
    site_span: Span,
    /// The ident of the macro we're parsing
    macro_ident: ast::Ident
}

impl<'a> ParserAnyMacro<'a> {
    /// Make sure we don't have any tokens left to parse, so we don't
    /// silently drop anything. `allow_semi` is so that "optional"
    /// semicolons at the end of normal expressions aren't complained
    /// about e.g. the semicolon in `macro_rules! kapow { () => {
    /// panic!(); } }` doesn't get picked up by .parse_expr(), but it's
    /// allowed to be there.
    fn ensure_complete_parse(&self, allow_semi: bool, context: &str) {
        let mut parser = self.parser.borrow_mut();
        if allow_semi && parser.token == token::Semi {
            parser.bump();
        }
        if parser.token != token::Eof {
            let token_str = parser.this_token_to_string();
            let msg = format!("macro expansion ignores token `{}` and any \
                               following",
                              token_str);
            let span = parser.span;
            let mut err = parser.diagnostic().struct_span_err(span, &msg[..]);
            let msg = format!("caused by the macro expansion here; the usage \
                               of `{}!` is likely invalid in {} context",
                               self.macro_ident, context);
            err.span_note(self.site_span, &msg[..])
               .emit();
        }
    }
}

impl<'a> MacResult for ParserAnyMacro<'a> {
    fn make_expr(self: Box<ParserAnyMacro<'a>>) -> Option<P<ast::Expr>> {
        let ret = panictry!(self.parser.borrow_mut().parse_expr());
        self.ensure_complete_parse(true, "expression");
        Some(ret)
    }
    fn make_pat(self: Box<ParserAnyMacro<'a>>) -> Option<P<ast::Pat>> {
        let ret = panictry!(self.parser.borrow_mut().parse_pat());
        self.ensure_complete_parse(false, "pattern");
        Some(ret)
    }
    fn make_items(self: Box<ParserAnyMacro<'a>>) -> Option<SmallVector<P<ast::Item>>> {
        let mut ret = SmallVector::zero();
        while let Some(item) = panictry!(self.parser.borrow_mut().parse_item()) {
            ret.push(item);
        }
        self.ensure_complete_parse(false, "item");
        Some(ret)
    }

    fn make_impl_items(self: Box<ParserAnyMacro<'a>>)
                       -> Option<SmallVector<ast::ImplItem>> {
        let mut ret = SmallVector::zero();
        loop {
            let mut parser = self.parser.borrow_mut();
            match parser.token {
                token::Eof => break,
                _ => ret.push(panictry!(parser.parse_impl_item()))
            }
        }
        self.ensure_complete_parse(false, "item");
        Some(ret)
    }

    fn make_trait_items(self: Box<ParserAnyMacro<'a>>)
                       -> Option<SmallVector<ast::TraitItem>> {
        let mut ret = SmallVector::zero();
        loop {
            let mut parser = self.parser.borrow_mut();
            match parser.token {
                token::Eof => break,
                _ => ret.push(panictry!(parser.parse_trait_item()))
            }
        }
        self.ensure_complete_parse(false, "item");
        Some(ret)
    }


    fn make_stmts(self: Box<ParserAnyMacro<'a>>)
                  -> Option<SmallVector<ast::Stmt>> {
        let mut ret = SmallVector::zero();
        loop {
            let mut parser = self.parser.borrow_mut();
            match parser.token {
                token::Eof => break,
                _ => match parser.parse_full_stmt(true) {
                    Ok(maybe_stmt) => match maybe_stmt {
                        Some(stmt) => ret.push(stmt),
                        None => (),
                    },
                    Err(mut e) => {
                        e.emit();
                        break;
                    }
                }
            }
        }
        self.ensure_complete_parse(false, "statement");
        Some(ret)
    }

    fn make_ty(self: Box<ParserAnyMacro<'a>>) -> Option<P<ast::Ty>> {
        let ret = panictry!(self.parser.borrow_mut().parse_ty());
        self.ensure_complete_parse(false, "type");
        Some(ret)
    }
}

struct MacroRulesMacroExpander {
    name: ast::Ident,
    imported_from: Option<ast::Ident>,
    lhses: Vec<TokenTree>,
    rhses: Vec<TokenTree>,
    valid: bool,
}

impl TTMacroExpander for MacroRulesMacroExpander {
    fn expand<'cx>(&self,
                   cx: &'cx mut ExtCtxt,
                   sp: Span,
                   arg: &[TokenTree])
                   -> Box<MacResult+'cx> {
        if !self.valid {
            return DummyResult::any(sp);
        }
        generic_extension(cx,
                          sp,
                          self.name,
                          self.imported_from,
                          arg,
                          &self.lhses,
                          &self.rhses)
    }
}

/// Given `lhses` and `rhses`, this is the new macro we create
fn generic_extension<'cx>(cx: &'cx ExtCtxt,
                          sp: Span,
                          name: ast::Ident,
                          imported_from: Option<ast::Ident>,
                          arg: &[TokenTree],
                          lhses: &[TokenTree],
                          rhses: &[TokenTree])
                          -> Box<MacResult+'cx> {
    if cx.trace_macros() {
        println!("{}! {{ {} }}",
                 name,
                 print::pprust::tts_to_string(arg));
    }

    // Which arm's failure should we report? (the one furthest along)
    let mut best_fail_spot = DUMMY_SP;
    let mut best_fail_msg = "internal error: ran no matchers".to_string();

    for (i, lhs) in lhses.iter().enumerate() { // try each arm's matchers
        let lhs_tt = match *lhs {
            TokenTree::Delimited(_, ref delim) => &delim.tts[..],
            _ => cx.span_bug(sp, "malformed macro lhs")
        };

        match TokenTree::parse(cx, lhs_tt, arg) {
            Success(named_matches) => {
                let rhs = match rhses[i] {
                    // ignore delimiters
                    TokenTree::Delimited(_, ref delimed) => delimed.tts.clone(),
                    _ => cx.span_bug(sp, "malformed macro rhs"),
                };
                // rhs has holes ( `$id` and `$(...)` that need filled)
                let trncbr = new_tt_reader(&cx.parse_sess().span_diagnostic,
                                           Some(named_matches),
                                           imported_from,
                                           rhs);
                let mut p = Parser::new(cx.parse_sess(), cx.cfg(), Box::new(trncbr));
                p.filename = cx.filename.clone();
                p.mod_path_stack = cx.mod_path_stack.clone();
                p.restrictions = match cx.in_block {
                    true => Restrictions::NO_NONINLINE_MOD,
                    false => Restrictions::empty(),
                };
                p.check_unknown_macro_variable();
                // Let the context choose how to interpret the result.
                // Weird, but useful for X-macros.
                return Box::new(ParserAnyMacro {
                    parser: RefCell::new(p),

                    // Pass along the original expansion site and the name of the macro
                    // so we can print a useful error message if the parse of the expanded
                    // macro leaves unparsed tokens.
                    site_span: sp,
                    macro_ident: name
                })
            }
            Failure(sp, ref msg) => if sp.lo >= best_fail_spot.lo {
                best_fail_spot = sp;
                best_fail_msg = (*msg).clone();
            },
            Error(err_sp, ref msg) => {
                cx.span_fatal(err_sp.substitute_dummy(sp), &msg[..])
            }
        }
    }

     cx.span_fatal(best_fail_spot.substitute_dummy(sp), &best_fail_msg[..]);
}

// Note that macro-by-example's input is also matched against a token tree:
//                   $( $lhs:tt => $rhs:tt );+
//
// Holy self-referential!

/// Converts a `macro_rules!` invocation into a syntax extension.
pub fn compile<'cx>(cx: &'cx mut ExtCtxt,
                    def: &ast::MacroDef,
                    check_macro: bool) -> SyntaxExtension {

    let lhs_nm =  gensym_ident("lhs");
    let rhs_nm =  gensym_ident("rhs");

    // The pattern that macro_rules matches.
    // The grammar for macro_rules! is:
    // $( $lhs:tt => $rhs:tt );+
    // ...quasiquoting this would be nice.
    // These spans won't matter, anyways
    let match_lhs_tok = MatchNt(lhs_nm, token::str_to_ident("tt"));
    let match_rhs_tok = MatchNt(rhs_nm, token::str_to_ident("tt"));
    let argument_gram = vec![
        TokenTree::Sequence(DUMMY_SP, Rc::new(tokenstream::SequenceRepetition {
            tts: vec![
                TokenTree::Token(DUMMY_SP, match_lhs_tok),
                TokenTree::Token(DUMMY_SP, token::FatArrow),
                TokenTree::Token(DUMMY_SP, match_rhs_tok),
            ],
            separator: Some(token::Semi),
            op: tokenstream::KleeneOp::OneOrMore,
            num_captures: 2,
        })),
        // to phase into semicolon-termination instead of semicolon-separation
        TokenTree::Sequence(DUMMY_SP, Rc::new(tokenstream::SequenceRepetition {
            tts: vec![TokenTree::Token(DUMMY_SP, token::Semi)],
            separator: None,
            op: tokenstream::KleeneOp::ZeroOrMore,
            num_captures: 0
        })),
    ];

    // Parse the macro_rules! invocation (`none` is for no interpolations):
    let arg_reader = new_tt_reader(&cx.parse_sess().span_diagnostic,
                                   None,
                                   None,
                                   def.body.clone());

    let argument_map = match parse(cx.parse_sess(),
                                   cx.cfg(),
                                   arg_reader,
                                   &argument_gram) {
        Success(m) => m,
        Failure(sp, str) | Error(sp, str) => {
            panic!(cx.parse_sess().span_diagnostic
                     .span_fatal(sp.substitute_dummy(def.span), &str[..]));
        }
    };

    let mut valid = true;

    // Extract the arguments:
    let lhses = match **argument_map.get(&lhs_nm.name).unwrap() {
        MatchedSeq(ref s, _) => {
            s.iter().map(|m| match **m {
                MatchedNonterminal(NtTT(ref tt)) => {
                    if check_macro {
                        valid &= check_lhs_nt_follows(cx, tt);
                    }
                    (**tt).clone()
                }
                _ => cx.span_bug(def.span, "wrong-structured lhs")
            }).collect::<Vec<_>>()
        }
        _ => cx.span_bug(def.span, "wrong-structured lhs")
    };

    if check_macro {
        'a: for (i, lhs) in lhses.iter().enumerate() {
            for lhs_ in lhses[i + 1 ..].iter() {
                match check_lhs_firsts(cx, lhs, lhs_) {
                    AnalysisResult::Error => {
                        cx.struct_span_err(def.span, "macro is not future-proof")
                            .span_help(lhs.get_span(), "parsing of this arm is ambiguous...")
                            .span_help(lhs_.get_span(), "with the parsing of this arm.")
                            .help("the behaviour of this macro might change in the future")
                            .emit();
                        //valid = false;
                        break 'a;
                    }
                    _ => ()
                }
            }
        }
    }

    let rhses = match **argument_map.get(&rhs_nm.name).unwrap() {
        MatchedSeq(ref s, _) => {
            s.iter().map(|m| match **m {
                MatchedNonterminal(NtTT(ref tt)) => (**tt).clone(),
                _ => cx.span_bug(def.span, "wrong-structured rhs")
            }).collect()
        }
        _ => cx.span_bug(def.span, "wrong-structured rhs")
    };

    for rhs in &rhses {
        valid &= check_rhs(cx, rhs);
    }

    let exp: Box<_> = Box::new(MacroRulesMacroExpander {
        name: def.ident,
        imported_from: def.imported_from,
        lhses: lhses,
        rhses: rhses,
        valid: valid,
    });

    NormalTT(exp, Some(def.span), def.allow_internal_unstable)
}

fn check_lhs_firsts(cx: &ExtCtxt, lhs: &TokenTree, lhs_: &TokenTree)
                    -> AnalysisResult {
    match (lhs, lhs_) {
        (&TokenTree::Delimited(_, ref tta),
         &TokenTree::Delimited(_, ref ttb)) =>
            check_matcher_firsts(cx, &tta.tts, &ttb.tts, &mut HashSet::new()),
        _ => cx.span_bug(lhs.get_span(), "malformed macro lhs")
    }
}

fn match_same_input(ma: &TokenTree, mb: &TokenTree) -> bool {
    match (ma, mb) {
        (&TokenTree::Token(_, MatchNt(_, nta)),
         &TokenTree::Token(_, MatchNt(_, ntb))) =>
            nta == ntb,
        // FIXME: must we descend into Interpolated TTs here?
        (&TokenTree::Token(_, ref toka),
         &TokenTree::Token(_, ref tokb)) =>
            toka == tokb,
        (&TokenTree::Delimited(_, ref delima),
         &TokenTree::Delimited(_, ref delimb)) => {
            delima.delim == delimb.delim &&
            delima.tts.iter().zip(delimb.tts.iter())
                .all(|(ref t1, ref t2)| match_same_input(t1, t2))
        }
        // we cannot consider that sequences match the same input
        // they need to be checked specially.
        // (&TokenTree::Sequence(_, ref seqa),
        //  &TokenTree::Sequence(_, ref seqb)) => {
        //     seqa.separator == seqb.separator &&
        //     seqa.op == seqb.op &&
        //     seqa.tts.iter().zip(seqb.tts.iter())
        //         .all(|(ref t1, ref t2)| match_same_input(t1, t2))
        // }
        _ => false
    }
}

// assumes that tok != MatchNt
fn nt_first_set_contains(nt: ast::Ident, tok: &Token) -> bool {
    use parse::token::BinOpToken::*;
    use parse::token::DelimToken::*;
    match &nt.name.as_str() as &str {
        "tt" => true,
        "ident" => match *tok {
            Ident(_) => true,
            _ => false
        },
        "meta" => match *tok {
            Ident(_) => true,
            _ => false
        },
        "path" => match *tok {
            ModSep |
            Ident(_) => true,
            _ => false
        },
        "ty" => match *tok {
            AndAnd |
            BinOp(And) |
            OpenDelim(Paren) |
            OpenDelim(Bracket) |
            BinOp(Star) |
            ModSep |
            BinOp(Shl) |
            Lt |
            Underscore |
            Ident(_) => true,
            _ => false
        },
        "expr" => match *tok {
            BinOp(And) |
            AndAnd |
            Not |
            BinOp(Star) |
            BinOp(Minus) |
            OpenDelim(_) |
            DotDot |
            ModSep |
            BinOp(Shl) |
            Lt |
            Lifetime(_) |
            BinOp(Or) |
            OrOr |
            Ident(_) |
            Literal(..) => true,
            _ => false
        },
        "pat" => match *tok {
            AndAnd |
            BinOp(And) |
            OpenDelim(Paren) |
            OpenDelim(Bracket) |
            BinOp(Minus) |
            ModSep |
            BinOp(Shl) |
            Lt|
            Underscore |
            Ident(_) |
            Literal(..) => true,
            _ => false
        },
        "stmt" => match *tok {
            BinOp(And) |
            AndAnd |
            Not |
            BinOp(Star) |
            BinOp(Minus) |
            Pound |
            OpenDelim(_) |
            DotDot |
            ModSep |
            Semi |
            BinOp(Shl) |
            Lt |
            Lifetime(_) |
            BinOp(Or) |
            OrOr |
            Ident(_) |
            Literal(..) => true,
            _ => false
        },
        "block" => match *tok {
            OpenDelim(Brace) => true,
            _ => false
        },
        "item" => match *tok {
            ModSep |
            Ident(_) => true,
            _ => false
        },
        _ => panic!("unknown NT")
    }
}

fn nt_first_disjoints(nt1: ast::Ident, nt2: ast::Ident) -> bool {
    use parse::token::DelimToken::*;
    match (&nt1.name.as_str() as &str, &nt2.name.as_str() as &str) {
        ("block", _) => !nt_first_set_contains(nt2, &OpenDelim(Brace)),
        (_, "block") => !nt_first_set_contains(nt1, &OpenDelim(Brace)),
        // all the others can contain Ident
        _ => false
    }
}

fn first_set_contains(set: &TokenSet, tok: &Token) -> bool {
    for &(_, ref t) in set.tokens.iter() {
        match (t, tok) {
            (&MatchNt(_, nt1), &MatchNt(_, nt2)) =>
                if !nt_first_disjoints(nt1, nt2) { return true },
            (&MatchNt(_, nt), tok) | (tok, &MatchNt(_, nt)) =>
                if nt_first_set_contains(nt, tok) { return true },
            (t1, t2) => if t1 == t2 { return true }
        }
    }
    return false
}

fn token_of(tt: &TokenTree) -> Token {
    use tokenstream::TokenTree::*;
    match tt {
        &Delimited(_, ref delim) => OpenDelim(delim.delim.clone()),
        &Token(_, ref tok) => tok.clone(),
        &Sequence(..) => panic!("unexpected seq")
    }
}

#[allow(unused_variables)]
fn first_sets_disjoints(ma: &TokenTree, mb: &TokenTree,
                        first_a: &FirstSets, first_b: &FirstSets) -> bool {
    use tokenstream::TokenTree::*;
    match (ma, mb) {
        (&Token(_, MatchNt(_, nta)),
         &Token(_, MatchNt(_, ntb))) => nt_first_disjoints(nta, ntb),

        (&Token(_, MatchNt(_, nt)), &Token(_, ref tok)) |
        (&Token(_, ref tok), &Token(_, MatchNt(_, nt))) =>
            !nt_first_set_contains(nt, tok),

        (&Token(_, MatchNt(_, nt)), &Delimited(_, ref delim)) |
        (&Delimited(_, ref delim), &Token(_, MatchNt(_, nt))) =>
            !nt_first_set_contains(nt, &OpenDelim(delim.delim.clone())),

        (&Sequence(ref spa, _), &Sequence(ref spb, _)) => {
            match (first_a.first.get(spa), first_b.first.get(spb)) {
                (Some(&Some(ref seta)), Some(&Some(ref setb))) => {
                    for &(_, ref tok) in setb.tokens.iter() {
                        if first_set_contains(seta, tok) {
                            return false
                        }
                    }
                    true
                }
                _ => panic!("no FIRST set for sequence")
            }
        }

        (&Sequence(ref sp, _), ref tok) => {
            match first_a.first.get(sp) {
                Some(&Some(ref set)) => !first_set_contains(set, &token_of(tok)),
                _ => panic!("no FIRST set for sequence")
            }
        }

        (ref tok, &Sequence(ref sp, _)) => {
            match first_b.first.get(sp) {
                Some(&Some(ref set)) => !first_set_contains(set, &token_of(tok)),
                _ => panic!("no FIRST set for sequence")
            }
        }

        (&Token(_, ref t1), &Token(_, ref t2)) =>
            t1 != t2,

        (&Token(_, ref t), &Delimited(_, ref delim)) |
        (&Delimited(_, ref delim), &Token(_, ref t)) =>
            t != &OpenDelim(delim.delim.clone()),

        (&Delimited(_, ref d1), &Delimited(_, ref d2)) =>
            d1.delim != d2.delim
    }
}

// the result of the FIRST set analysis.
// * Ok -> an obvious disambiguation has been found
// * Unsure -> no problem between those matchers but analysis should continue
// * Error -> maybe a problem. should be accepted only if an obvious
//   disambiguation is found later
enum AnalysisResult {
    Ok,
    Unsure,
    Error
}

impl AnalysisResult {
    fn chain<F: FnMut() -> AnalysisResult>(self, mut next: F) -> AnalysisResult {
        if let AnalysisResult::Error = self { return self };
        match next() {
            AnalysisResult::Ok => self,
            ret => ret
        }
    }
}

fn unroll_sequence<'a>(sp: Span, seq: &tokenstream::SequenceRepetition,
                       next: &[TokenTree]) -> Vec<TokenTree> {
    let mut ret = seq.tts.to_vec();
    seq.separator.clone().map(|sep| ret.push(TokenTree::Token(sp, sep)));
    ret.push(TokenTree::Sequence(
        // clone a sequence. change $(...)+ into $(...)*
        sp, tokenstream::SequenceRepetition {
            op: tokenstream::KleeneOp::ZeroOrMore,
            .. seq.clone()
        }
    ));
    ret.extend_from_slice(next);
    ret
}

fn check_sequence<F>(sp: Span, seq: &tokenstream::SequenceRepetition,
                     next: &[TokenTree], against: &[TokenTree], mut callback: F)
                     -> AnalysisResult
    where F: FnMut(&[TokenTree], &[TokenTree]) -> AnalysisResult {
    let unrolled = unroll_sequence(sp, seq, next);
    let ret = callback(&unrolled, against);

    if seq.op == tokenstream::KleeneOp::ZeroOrMore {
        ret.chain(|| callback(next, against))
    } else { ret }
}

fn check_matcher_firsts(cx: &ExtCtxt, ma: &[TokenTree], mb: &[TokenTree],
                        visited_spans: &mut HashSet<(Span, Span)>)
                        -> AnalysisResult {
    use self::AnalysisResult::*;
    let mut need_disambiguation = false;

    //println!("running on {:?} <-> {:?}", ma, mb);

    // first compute the FIRST sets. FIRST sets for tokens, delimited TTs and NT
    // matchers are fixed, this will compute the FIRST sets for all sequence TTs
    // that appear in the matcher. Note that if a sequence starts with a matcher,
    // for ex. $e:expr, its FIRST set will be the singleton { MatchNt(expr) }.
    // This is okay because none of our matchable NTs can be empty.
    let firsts_a = FirstSets::new(ma);
    let firsts_b = FirstSets::new(mb);

    // analyse until one of the cases happen:
    // * we find an obvious disambiguation, that is a proof that all inputs that
    //   matches A will never match B or vice-versa
    // * we find a case that is too complex to handle and reject it
    // * we reach the end of the macro
    let iter_a = ma.iter().enumerate();
    let iter_b = mb.iter().enumerate();
    let mut iter = iter_a.clone().zip(iter_b.clone());
    while let Some(((idx_a, ta), (idx_b, tb))) = iter.next() {
        if visited_spans.contains(&(ta.get_span(), tb.get_span())) {
            return if need_disambiguation { Error } else { Unsure };
        }

        visited_spans.insert((ta.get_span(), tb.get_span()));

        // sequence analysis

        match (ta, tb) {
            (&TokenTree::Sequence(sp_a, ref seq_a),
             &TokenTree::Sequence(sp_b, ref seq_b)) => {
                let mut ret = check_sequence(sp_a, seq_a, &ma[idx_a + 1 ..], &mb[idx_b ..], |u, a| {
                    check_matcher_firsts(cx, u, a, visited_spans)
                });

                ret = ret.chain(|| {
                    check_sequence(sp_b, seq_b, &mb[idx_b + 1 ..], &ma[idx_a ..], |u, a| {
                        check_matcher_firsts(cx, a, u, visited_spans)
                    })
                });

                return match ret {
                    Unsure => if need_disambiguation { Error } else { Unsure },
                    _ => ret
                };
            }

            (&TokenTree::Sequence(sp, ref seq), _) => {
                let ret = check_sequence(sp, seq, &ma[idx_a + 1 ..], &mb[idx_b ..], |u, a| {
                    check_matcher_firsts(cx, u, a, visited_spans)
                });

                return match ret {
                    Unsure => if need_disambiguation { Error } else { Unsure },
                    _ => ret
                };
            }

            (_, &TokenTree::Sequence(sp, ref seq)) => {
                let ret = check_sequence(sp, seq, &mb[idx_b + 1 ..], &ma[idx_a ..], |u, a| {
                    check_matcher_firsts(cx, a, u, visited_spans)
                });

                return match ret {
                    Unsure => if need_disambiguation { Error } else { Unsure },
                    _ => ret
                };
            }

            _ => ()
        }

        if match_same_input(ta, tb) {
            continue;
        }

        if first_sets_disjoints(&ta, &tb, &firsts_a, &firsts_b) {
            // accept the macro
            return Ok
        }

        // now we cannot say anything in the general case but we can
        // still look if we are in a particular case we know how to handle...
        match (ta, tb) {
            (&TokenTree::Sequence(_, _), _) |
            (_, &TokenTree::Sequence(_, _)) =>
                // cannot happen since we treated sequences earlier
                cx.bug("unexpeceted seq"),

            (_ ,&TokenTree::Token(_, MatchNt(_, nt))) if !nt_is_single_tt(nt) =>
                return if only_simple_tokens(&ma[idx_a..]) && !need_disambiguation {
                    Unsure
                } else { Error },

            // first case: NT vs _.
            // invariant: B is always a single-TT

            (&TokenTree::Token(_, MatchNt(_, nt)), _)
                // ident or tt will never start matching more input
                if nt.name.as_str() == "ident" ||
                   nt.name.as_str() == "tt" => continue,

            (&TokenTree::Token(_, MatchNt(_, nt)), _)
                if nt.name.as_str() == "block" => {
                match tb {
                    &TokenTree::Delimited(_, ref delim)
                        if delim.delim == token::DelimToken::Brace => {
                        // we cannot say much here. we cannot look inside. we
                        // can just hope we will find an obvious disambiguation later
                        need_disambiguation = true;
                        continue;
                    }
                    &TokenTree::Token(_, MatchNt(_, nt))
                        if nt.name.as_str() == "tt" => {
                        // same
                        need_disambiguation = true;
                        continue;
                    }
                    // should be the only possibility.
                    _ => cx.bug("unexpected matcher against block")
                }
            }

            (&TokenTree::Token(_, MatchNt(_, _)), _) =>
                // A is a NT matcher that is not tt, ident, or block (that is, A
                // could match several token trees), we cannot know where we
                // should continue the analysis.
                return Error,

            // second case: T vs _.
            // both A and B are always a single-TT

            (&TokenTree::Token(..), &TokenTree::Token(_, MatchNt(_, nt))) => {
                assert!(nt.name.as_str() == "ident" || nt.name.as_str() == "tt");
                // the token will never match new input
                continue;
            }

            (&TokenTree::Delimited(_, ref delim),
             &TokenTree::Token(_, MatchNt(_, _))) => {
                // either block vs { } or tt vs any delim.
                // as with several-TTs NTs, if the above is only
                // made of simple tokens this is ok...
                need_disambiguation |= !only_simple_tokens(&delim.tts);
                continue;
            }

            (&TokenTree::Delimited(_, ref d1),
             &TokenTree::Delimited(_, ref d2)) => {
                // they have the same delim. as above.
                assert!(d1.delim == d2.delim);
                // descend into delimiters.
                match check_matcher_firsts(cx, &d1.tts, &d2.tts, visited_spans) {
                    Ok => return Ok,
                    Unsure => continue,
                    Error => {
                        need_disambiguation = true;
                        continue
                    }
                }
            }

            // cannot happen. either they're the same
            // token or their FIRST sets are disjoint.
            (&TokenTree::Token(..), &TokenTree::Token(..)) |
            (&TokenTree::Token(..), &TokenTree::Delimited(..)) |
            (&TokenTree::Delimited(..), &TokenTree::Token(..)) =>
                cx.bug("unexpected Token vs. Token")
        }
    }

    // now we are at the end of one arm:
    // we know that the last element on this arm was not a sequence (we would
    // have returned earlier), so it cannot accept new input at this point.
    // if the other arm always accept new input, that is, if it cannot accept
    // the end of stream, then this is a disambiguation.
    let (ma, mb): (Vec<_>, Vec<_>) = iter.unzip();
    for &(_, tt) in if ma.len() == 0 { mb.iter() } else { ma.iter() } {
        match tt {
            &TokenTree::Sequence(_, ref seq)
                if seq.op == tokenstream::KleeneOp::ZeroOrMore => continue,
            _ =>
                // this arm still expects input, while the other can't.
                // use this as a disambiguation
                return Ok
        }
    }

    if need_disambiguation {
        // we couldn't find any. we cannot say anything about those arms.
        // reject conservatively.
        Error
    } else {
        // either A is strictly included in B and the other inputs that match B
        // will never match A, or B is included in or equal to A, which means
        // it's unreachable. this is not our problem. accept.
        Unsure
    }
}

// checks that a matcher does not contain any NT except ident or TT.
// that is, that it will never start matching new input
fn only_simple_tokens(m: &[TokenTree]) -> bool {
    m.iter().all(|tt| match *tt {
        TokenTree::Token(_, MatchNt(_, nt)) =>
            nt.name.as_str() == "ident" ||
            nt.name.as_str() == "tt",
        TokenTree::Token(..) => true,
        TokenTree::Delimited(_, ref delim) => only_simple_tokens(&delim.tts),
        TokenTree::Sequence(_, ref seq) => only_simple_tokens(&seq.tts)
    })
}

fn nt_is_single_tt(nt: ast::Ident) -> bool {
    match &nt.name.as_str() as &str {
        "block" | "ident" | "tt" => true,
        _ => false
    }
}

fn check_lhs_nt_follows(cx: &mut ExtCtxt, lhs: &TokenTree) -> bool {
    // lhs is going to be like TokenTree::Delimited(...), where the
    // entire lhs is those tts. Or, it can be a "bare sequence", not wrapped in parens.
    match lhs {
        &TokenTree::Delimited(_, ref tts) => check_matcher(cx, &tts.tts),
        _ => {
            cx.span_err(lhs.get_span(), "invalid macro matcher; matchers must \
                                         be contained in balanced delimiters");
            false
        }
    }
    // we don't abort on errors on rejection, the driver will do that for us
    // after parsing/expansion. we can report every error in every macro this way.
}

fn check_rhs(cx: &mut ExtCtxt, rhs: &TokenTree) -> bool {
    match *rhs {
        TokenTree::Delimited(..) => return true,
        _ => cx.span_err(rhs.get_span(), "macro rhs must be delimited")
    }
    false
}

fn check_matcher(cx: &mut ExtCtxt, matcher: &[TokenTree]) -> bool {
    let first_sets = FirstSets::new(matcher);
    let empty_suffix = TokenSet::empty();
    let err = cx.parse_sess.span_diagnostic.err_count();
    check_matcher_core(cx, &first_sets, matcher, &empty_suffix);
    err == cx.parse_sess.span_diagnostic.err_count()
}

// The FirstSets for a matcher is a mapping from subsequences in the
// matcher to the FIRST set for that subsequence.
//
// This mapping is partially precomputed via a backwards scan over the
// token trees of the matcher, which provides a mapping from each
// repetition sequence to its FIRST set.
//
// (Hypothetically sequences should be uniquely identifiable via their
// spans, though perhaps that is false e.g. for macro-generated macros
// that do not try to inject artificial span information. My plan is
// to try to catch such cases ahead of time and not include them in
// the precomputed mapping.)
struct FirstSets {
    // this maps each TokenTree::Sequence `$(tt ...) SEP OP` that is uniquely identified by its
    // span in the original matcher to the First set for the inner sequence `tt ...`.
    //
    // If two sequences have the same span in a matcher, then map that
    // span to None (invalidating the mapping here and forcing the code to
    // use a slow path).
    first: HashMap<Span, Option<TokenSet>>,
}

impl FirstSets {
    fn new(tts: &[TokenTree]) -> FirstSets {
        let mut sets = FirstSets { first: HashMap::new() };
        build_recur(&mut sets, tts);
        return sets;

        // walks backward over `tts`, returning the FIRST for `tts`
        // and updating `sets` at the same time for all sequence
        // substructure we find within `tts`.
        fn build_recur(sets: &mut FirstSets, tts: &[TokenTree]) -> TokenSet {
            let mut first = TokenSet::empty();
            for tt in tts.iter().rev() {
                match *tt {
                    TokenTree::Token(sp, ref tok) => {
                        first.replace_with((sp, tok.clone()));
                    }
                    TokenTree::Delimited(_, ref delimited) => {
                        build_recur(sets, &delimited.tts[..]);
                        first.replace_with((delimited.open_span,
                                            Token::OpenDelim(delimited.delim)));
                    }
                    TokenTree::Sequence(sp, ref seq_rep) => {
                        let subfirst = build_recur(sets, &seq_rep.tts[..]);

                        match sets.first.entry(sp) {
                            Entry::Vacant(vac) => {
                                vac.insert(Some(subfirst.clone()));
                            }
                            Entry::Occupied(mut occ) => {
                                // if there is already an entry, then a span must have collided.
                                // This should not happen with typical macro_rules macros,
                                // but syntax extensions need not maintain distinct spans,
                                // so distinct syntax trees can be assigned the same span.
                                // In such a case, the map cannot be trusted; so mark this
                                // entry as unusable.
                                occ.insert(None);
                            }
                        }

                        // If the sequence contents can be empty, then the first
                        // token could be the separator token itself.

                        if let (Some(ref sep), true) = (seq_rep.separator.clone(),
                                                        subfirst.maybe_empty) {
                            first.add_one_maybe((sp, sep.clone()));
                        }

                        // Reverse scan: Sequence comes before `first`.
                        if subfirst.maybe_empty || seq_rep.op == tokenstream::KleeneOp::ZeroOrMore {
                            // If sequence is potentially empty, then
                            // union them (preserving first emptiness).
                            first.add_all(&TokenSet { maybe_empty: true, ..subfirst });
                        } else {
                            // Otherwise, sequence guaranteed
                            // non-empty; replace first.
                            first = subfirst;
                        }
                    }
                }
            }

            return first;
        }
    }

    // walks forward over `tts` until all potential FIRST tokens are
    // identified.
    fn first(&self, tts: &[TokenTree]) -> TokenSet {
        let mut first = TokenSet::empty();
        for tt in tts.iter() {
            assert!(first.maybe_empty);
            match *tt {
                TokenTree::Token(sp, ref tok) => {
                    first.add_one((sp, tok.clone()));
                    return first;
                }
                TokenTree::Delimited(_, ref delimited) => {
                    first.add_one((delimited.open_span,
                                   Token::OpenDelim(delimited.delim)));
                    return first;
                }
                TokenTree::Sequence(sp, ref seq_rep) => {
                    match self.first.get(&sp) {
                        Some(&Some(ref subfirst)) => {

                            // If the sequence contents can be empty, then the first
                            // token could be the separator token itself.

                            if let (Some(ref sep), true) = (seq_rep.separator.clone(),
                                                            subfirst.maybe_empty) {
                                first.add_one_maybe((sp, sep.clone()));
                            }

                            assert!(first.maybe_empty);
                            first.add_all(subfirst);
                            if subfirst.maybe_empty ||
                               seq_rep.op == tokenstream::KleeneOp::ZeroOrMore {
                                // continue scanning for more first
                                // tokens, but also make sure we
                                // restore empty-tracking state
                                first.maybe_empty = true;
                                continue;
                            } else {
                                return first;
                            }
                        }

                        Some(&None) => {
                            panic!("assume all sequences have (unique) spans for now");
                        }

                        None => {
                            panic!("We missed a sequence during FirstSets construction");
                        }
                    }
                }
            }
        }

        // we only exit the loop if `tts` was empty or if every
        // element of `tts` matches the empty sequence.
        assert!(first.maybe_empty);
        return first;
    }
}

// A set of Tokens, which may include MatchNt tokens (for
// macro-by-example syntactic variables). It also carries the
// `maybe_empty` flag; that is true if and only if the matcher can
// match an empty token sequence.
//
// The First set is computed on submatchers like `$($a:expr b),* $(c)* d`,
// which has corresponding FIRST = {$a:expr, c, d}.
// Likewise, `$($a:expr b),* $(c)+ d` has FIRST = {$a:expr, c}.
//
// (Notably, we must allow for *-op to occur zero times.)
#[derive(Clone, Debug)]
struct TokenSet {
    tokens: Vec<(Span, Token)>,
    maybe_empty: bool,
}

impl TokenSet {
    // Returns a set for the empty sequence.
    fn empty() -> Self { TokenSet { tokens: Vec::new(), maybe_empty: true } }

    // Returns the set `{ tok }` for the single-token (and thus
    // non-empty) sequence [tok].
    fn singleton(tok: (Span, Token)) -> Self {
        TokenSet { tokens: vec![tok], maybe_empty: false }
    }

    // Changes self to be the set `{ tok }`.
    // Since `tok` is always present, marks self as non-empty.
    fn replace_with(&mut self, tok: (Span, Token)) {
        self.tokens.clear();
        self.tokens.push(tok);
        self.maybe_empty = false;
    }

    // Changes self to be the empty set `{}`; meant for use when
    // the particular token does not matter, but we want to
    // record that it occurs.
    fn replace_with_irrelevant(&mut self) {
        self.tokens.clear();
        self.maybe_empty = false;
    }

    // Adds `tok` to the set for `self`, marking sequence as non-empy.
    fn add_one(&mut self, tok: (Span, Token)) {
        if !self.tokens.contains(&tok) {
            self.tokens.push(tok);
        }
        self.maybe_empty = false;
    }

    // Adds `tok` to the set for `self`. (Leaves `maybe_empty` flag alone.)
    fn add_one_maybe(&mut self, tok: (Span, Token)) {
        if !self.tokens.contains(&tok) {
            self.tokens.push(tok);
        }
    }

    // Adds all elements of `other` to this.
    //
    // (Since this is a set, we filter out duplicates.)
    //
    // If `other` is potentially empty, then preserves the previous
    // setting of the empty flag of `self`. If `other` is guaranteed
    // non-empty, then `self` is marked non-empty.
    fn add_all(&mut self, other: &Self) {
        for tok in &other.tokens {
            if !self.tokens.contains(tok) {
                self.tokens.push(tok.clone());
            }
        }
        if !other.maybe_empty {
            self.maybe_empty = false;
        }
    }
}

// Checks that `matcher` is internally consistent and that it
// can legally by followed by a token N, for all N in `follow`.
// (If `follow` is empty, then it imposes no constraint on
// the `matcher`.)
//
// Returns the set of NT tokens that could possibly come last in
// `matcher`. (If `matcher` matches the empty sequence, then
// `maybe_empty` will be set to true.)
//
// Requires that `first_sets` is pre-computed for `matcher`;
// see `FirstSets::new`.
fn check_matcher_core(cx: &mut ExtCtxt,
                      first_sets: &FirstSets,
                      matcher: &[TokenTree],
                      follow: &TokenSet) -> TokenSet {
    use print::pprust::token_to_string;

    let mut last = TokenSet::empty();

    // 2. For each token and suffix  [T, SUFFIX] in M:
    // ensure that T can be followed by SUFFIX, and if SUFFIX may be empty,
    // then ensure T can also be followed by any element of FOLLOW.
    'each_token: for i in 0..matcher.len() {
        let token = &matcher[i];
        let suffix = &matcher[i+1..];

        let build_suffix_first = || {
            let mut s = first_sets.first(suffix);
            if s.maybe_empty { s.add_all(follow); }
            return s;
        };

        // (we build `suffix_first` on demand below; you can tell
        // which cases are supposed to fall through by looking for the
        // initialization of this variable.)
        let suffix_first;

        // First, update `last` so that it corresponds to the set
        // of NT tokens that might end the sequence `... token`.
        match *token {
            TokenTree::Token(sp, ref tok) => {
                let can_be_followed_by_any;
                if let Err(bad_frag) = has_legal_fragment_specifier(tok) {
                    cx.struct_span_err(sp, &format!("invalid fragment specifier `{}`", bad_frag))
                        .help("valid fragment specifiers are `ident`, `block`, \
                               `stmt`, `expr`, `pat`, `ty`, `path`, `meta`, `tt` \
                               and `item`")
                        .emit();
                    // (This eliminates false positives and duplicates
                    // from error messages.)
                    can_be_followed_by_any = true;
                } else {
                    can_be_followed_by_any = token_can_be_followed_by_any(tok);
                }

                if can_be_followed_by_any {
                    // don't need to track tokens that work with any,
                    last.replace_with_irrelevant();
                    // ... and don't need to check tokens that can be
                    // followed by anything against SUFFIX.
                    continue 'each_token;
                } else {
                    last.replace_with((sp, tok.clone()));
                    suffix_first = build_suffix_first();
                }
            }
            TokenTree::Delimited(_, ref d) => {
                let my_suffix = TokenSet::singleton((d.close_span, Token::CloseDelim(d.delim)));
                check_matcher_core(cx, first_sets, &d.tts, &my_suffix);
                // don't track non NT tokens
                last.replace_with_irrelevant();

                // also, we don't need to check delimited sequences
                // against SUFFIX
                continue 'each_token;
            }
            TokenTree::Sequence(sp, ref seq_rep) => {
                suffix_first = build_suffix_first();
                // The trick here: when we check the interior, we want
                // to include the separator (if any) as a potential
                // (but not guaranteed) element of FOLLOW. So in that
                // case, we make a temp copy of suffix and stuff
                // delimiter in there.
                //
                // FIXME: Should I first scan suffix_first to see if
                // delimiter is already in it before I go through the
                // work of cloning it? But then again, this way I may
                // get a "tighter" span?
                let mut new;
                let my_suffix = if let Some(ref u) = seq_rep.separator {
                    new = suffix_first.clone();
                    new.add_one_maybe((sp, u.clone()));
                    &new
                } else {
                    &suffix_first
                };

                // At this point, `suffix_first` is built, and
                // `my_suffix` is some TokenSet that we can use
                // for checking the interior of `seq_rep`.
                let next = check_matcher_core(cx, first_sets, &seq_rep.tts, my_suffix);
                if next.maybe_empty {
                    last.add_all(&next);
                } else {
                    last = next;
                }

                // the recursive call to check_matcher_core already ran the 'each_last
                // check below, so we can just keep going forward here.
                continue 'each_token;
            }
        }

        // (`suffix_first` guaranteed initialized once reaching here.)

        // Now `last` holds the complete set of NT tokens that could
        // end the sequence before SUFFIX. Check that every one works with `suffix`.
        'each_last: for &(_sp, ref t) in &last.tokens {
            if let MatchNt(ref name, ref frag_spec) = *t {
                for &(sp, ref next_token) in &suffix_first.tokens {
                    match is_in_follow(cx, next_token, &frag_spec.name.as_str()) {
                        Err((msg, help)) => {
                            cx.struct_span_err(sp, &msg).help(help).emit();
                            // don't bother reporting every source of
                            // conflict for a particular element of `last`.
                            continue 'each_last;
                        }
                        Ok(true) => {}
                        Ok(false) => {
                            let may_be = if last.tokens.len() == 1 &&
                                suffix_first.tokens.len() == 1
                            {
                                "is"
                            } else {
                                "may be"
                            };

                            cx.span_err(
                                sp,
                                &format!("`${name}:{frag}` {may_be} followed by `{next}`, which \
                                          is not allowed for `{frag}` fragments",
                                         name=name,
                                         frag=frag_spec,
                                         next=token_to_string(next_token),
                                         may_be=may_be)
                            );
                        }
                    }
                }
            }
        }
    }
    last
}

fn token_can_be_followed_by_any(tok: &Token) -> bool {
    if let &MatchNt(_, ref frag_spec) = tok {
        frag_can_be_followed_by_any(&frag_spec.name.as_str())
    } else {
        // (Non NT's can always be followed by anthing in matchers.)
        true
    }
}

/// True if a fragment of type `frag` can be followed by any sort of
/// token.  We use this (among other things) as a useful approximation
/// for when `frag` can be followed by a repetition like `$(...)*` or
/// `$(...)+`. In general, these can be a bit tricky to reason about,
/// so we adopt a conservative position that says that any fragment
/// specifier which consumes at most one token tree can be followed by
/// a fragment specifier (indeed, these fragments can be followed by
/// ANYTHING without fear of future compatibility hazards).
fn frag_can_be_followed_by_any(frag: &str) -> bool {
    match frag {
        "item"  | // always terminated by `}` or `;`
        "block" | // exactly one token tree
        "ident" | // exactly one token tree
        "meta"  | // exactly one token tree
        "tt" =>   // exactly one token tree
            true,

        _ =>
            false,
    }
}

/// True if `frag` can legally be followed by the token `tok`. For
/// fragments that can consume an unbounded number of tokens, `tok`
/// must be within a well-defined follow set. This is intended to
/// guarantee future compatibility: for example, without this rule, if
/// we expanded `expr` to include a new binary operator, we might
/// break macros that were relying on that binary operator as a
/// separator.
// when changing this do not forget to update doc/book/macros.md!
fn is_in_follow(_: &ExtCtxt, tok: &Token, frag: &str) -> Result<bool, (String, &'static str)> {
    if let &CloseDelim(_) = tok {
        // closing a token tree can never be matched by any fragment;
        // iow, we always require that `(` and `)` match, etc.
        Ok(true)
    } else {
        match frag {
            "item" => {
                // since items *must* be followed by either a `;` or a `}`, we can
                // accept anything after them
                Ok(true)
            },
            "block" => {
                // anything can follow block, the braces provide an easy boundary to
                // maintain
                Ok(true)
            },
            "stmt" | "expr"  => {
                match *tok {
                    FatArrow | Comma | Semi => Ok(true),
                    _ => Ok(false)
                }
            },
            "pat" => {
                match *tok {
                    FatArrow | Comma | Eq | BinOp(token::Or) => Ok(true),
                    Ident(i) if (i.name.as_str() == "if" ||
                                 i.name.as_str() == "in") => Ok(true),
                    _ => Ok(false)
                }
            },
            "path" | "ty" => {
                match *tok {
                    OpenDelim(token::DelimToken::Brace) | OpenDelim(token::DelimToken::Bracket) |
                    Comma | FatArrow | Colon | Eq | Gt | Semi | BinOp(token::Or) => Ok(true),
                    MatchNt(_, ref frag) if frag.name.as_str() == "block" => Ok(true),
                    Ident(i) if i.name.as_str() == "as" || i.name.as_str() == "where" => Ok(true),
                    _ => Ok(false)
                }
            },
            "ident" => {
                // being a single token, idents are harmless
                Ok(true)
            },
            "meta" | "tt" => {
                // being either a single token or a delimited sequence, tt is
                // harmless
                Ok(true)
            },
            _ => Err((format!("invalid fragment specifier `{}`", frag),
                     "valid fragment specifiers are `ident`, `block`, \
                      `stmt`, `expr`, `pat`, `ty`, `path`, `meta`, `tt` \
                      and `item`"))
        }
    }
}

fn has_legal_fragment_specifier(tok: &Token) -> Result<(), String> {
    debug!("has_legal_fragment_specifier({:?})", tok);
    if let &MatchNt(_, ref frag_spec) = tok {
        let s = &frag_spec.name.as_str();
        if !is_legal_fragment_specifier(s) {
            return Err(s.to_string());
        }
    }
    Ok(())
}

fn is_legal_fragment_specifier(frag: &str) -> bool {
    match frag {
        "item" | "block" | "stmt" | "expr" | "pat" |
        "path" | "ty" | "ident" | "meta" | "tt" => true,
        _ => false,
    }
}
