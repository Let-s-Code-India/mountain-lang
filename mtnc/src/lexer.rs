//! Hand-written finite-state-machine lexer for Mountain source files.
//! Spec: Document 2 (Lexical Structure & Tokens). Implementation approach
//! (hand-written FSM, not a regex/generator-based lexer) per Document 17
//! §2, chosen for precise control over multi-character operator
//! disambiguation and error recovery.
//!
//! This is a direct port of a Python prototype (`proto/lexer_proto.py`,
//! not part of this deliverable) that was executed and verified against
//! 33 test cases before translation, per the agreed Phase 1 process.

use crate::diagnostics::Diagnostic;
use crate::token::{Delim, Keyword, Op, Span, Token, TokenKind};

pub struct Lexer {
    /// We decode identifiers/strings/all structural tokens via
    /// UTF-8-aware char boundaries, matching Document 2 §1's "UTF-8
    /// only" mandate. For Phase 1/2 scope we accept full UTF-8 in
    /// identifiers/strings and restrict structural tokens to ASCII,
    /// which covers every construct in Documents 2-4; broader
    /// Unicode-identifier normalization rules are not specified
    /// anywhere in Docs 1-25, so none are invented here.
    ///
    /// (Earlier draft note, resolved: this struct originally also held
    /// a raw `&[u8]` byte-slice field alongside `chars`, intended for a
    /// byte-level fast path over ASCII structural characters that was
    /// never actually implemented — every scan in this file goes
    /// through `chars`. Confirmed via grep that the byte-slice field
    /// was never read anywhere, only written once in `new()`, so it was
    /// genuinely dead code, not a sign of some other part of the lexer
    /// wrongly relying on a different field — removed outright, which
    /// also removed the struct's now-unnecessary lifetime parameter.)
    chars: Vec<char>,
    i: usize,
    line: u32,
    col: u32,
    pub tokens: Vec<Token>,
    pub errors: Vec<Diagnostic>,
}

impl Lexer {
    pub fn new(src: &str) -> Self {
        Lexer {
            chars: src.chars().collect(),
            i: 0,
            line: 1,
            col: 1,
            tokens: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn peek(&self, off: usize) -> char {
        *self.chars.get(self.i + off).unwrap_or(&'\0')
    }

    fn advance(&mut self) -> char {
        let c = self.chars[self.i];
        self.i += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        c
    }

    fn at_end(&self) -> bool {
        self.i >= self.chars.len()
    }

    fn emit(&mut self, kind: TokenKind, span: Span) {
        self.tokens.push(Token::new(kind, span));
    }

    pub fn tokenize(mut self) -> (Vec<Token>, Vec<Diagnostic>) {
        while !self.at_end() {
            let c = self.peek(0);
            if c == ' ' || c == '\t' || c == '\r' || c == '\n' {
                self.advance();
                continue;
            }
            if c == '/' && self.peek(1) == '/' {
                self.lex_line_comment();
                continue;
            }
            if c == '/' && self.peek(1) == '*' {
                self.lex_block_comment();
                continue;
            }
            // Raw string check MUST precede the generic identifier branch:
            // 'r' is alphabetic, so without this ordering "r\"...\"" would
            // wrongly lex as an identifier `r` followed by a string.
            // (This exact bug was caught by the Python prototype harness.)
            if c == 'r' && self.peek(1) == '"' {
                self.lex_raw_string();
                continue;
            }
            if c.is_alphabetic() || c == '_' {
                self.lex_ident_or_keyword();
                continue;
            }
            if c.is_ascii_digit() {
                self.lex_number();
                continue;
            }
            if c == '"' {
                self.lex_string();
                continue;
            }
            if c == '\'' {
                self.lex_char();
                continue;
            }
            if self.lex_operator_or_delim() {
                continue;
            }
            // Unrecognized character: recoverable error, synthesize an
            // Error token, and continue lexing (Document 17 §2).
            let span = self.here();
            let bad = self.advance();
            self.errors.push(Diagnostic::new(
                format!("unexpected character '{}'", bad),
                span,
            ));
            self.emit(TokenKind::Error(bad.to_string()), span);
        }
        let eof_span = self.here();
        self.emit(TokenKind::Eof, eof_span);
        (self.tokens, self.errors)
    }

    fn here(&self) -> Span {
        Span { line: self.line, col: self.col }
    }

    fn lex_line_comment(&mut self) {
        let span = self.here();
        let is_doc = self.peek(2) == '/'; // '///'
        let start = self.i;
        while !self.at_end() && self.peek(0) != '\n' {
            self.advance();
        }
        if is_doc {
            let text: String = self.chars[start..self.i].iter().collect();
            self.emit(TokenKind::DocComment(text), span);
        }
        // ordinary `//` comments are discarded, not emitted, per Doc 2 §3
    }

    fn lex_block_comment(&mut self) {
        let span = self.here();
        self.advance();
        self.advance(); // consume /*
        let mut depth = 1;
        while !self.at_end() && depth > 0 {
            if self.peek(0) == '/' && self.peek(1) == '*' {
                self.advance();
                self.advance();
                depth += 1;
            } else if self.peek(0) == '*' && self.peek(1) == '/' {
                self.advance();
                self.advance();
                depth -= 1;
            } else {
                self.advance();
            }
        }
        if depth > 0 {
            self.errors.push(Diagnostic::new("unterminated block comment", span));
        }
    }

    fn lex_ident_or_keyword(&mut self) {
        let span = self.here();
        let start = self.i;
        while !self.at_end() && (self.peek(0).is_alphanumeric() || self.peek(0) == '_') {
            self.advance();
        }
        let text: String = self.chars[start..self.i].iter().collect();
        match text.as_str() {
            "true" => self.emit(TokenKind::Bool(true), span),
            "false" => self.emit(TokenKind::Bool(false), span),
            "null" => self.emit(TokenKind::Null, span),
            _ => match Keyword::from_str(&text) {
                Some(kw) => self.emit(TokenKind::Keyword(kw), span),
                None => self.emit(TokenKind::Ident(text), span),
            },
        }
    }

    fn lex_number(&mut self) {
        let span = self.here();
        let start = self.i;

        if self.peek(0) == '0' && (self.peek(1) == 'x' || self.peek(1) == 'X') {
            self.advance();
            self.advance();
            while matches!(self.peek(0), '0'..='9' | 'a'..='f' | 'A'..='F' | '_') {
                self.advance();
            }
            let text: String = self.chars[start..self.i].iter().collect();
            self.emit(TokenKind::IntHex(text), span);
            return;
        }
        if self.peek(0) == '0' && (self.peek(1) == 'o' || self.peek(1) == 'O') {
            self.advance();
            self.advance();
            while matches!(self.peek(0), '0'..='7' | '_') {
                self.advance();
            }
            let text: String = self.chars[start..self.i].iter().collect();
            self.emit(TokenKind::IntOct(text), span);
            return;
        }
        if self.peek(0) == '0' && (self.peek(1) == 'b' || self.peek(1) == 'B') {
            self.advance();
            self.advance();
            while matches!(self.peek(0), '0' | '1' | '_') {
                self.advance();
            }
            let text: String = self.chars[start..self.i].iter().collect();
            self.emit(TokenKind::IntBin(text), span);
            return;
        }

        while self.peek(0).is_ascii_digit() || self.peek(0) == '_' {
            self.advance();
        }
        let mut is_float = false;
        if self.peek(0) == '.' && self.peek(1).is_ascii_digit() {
            is_float = true;
            self.advance();
            while self.peek(0).is_ascii_digit() || self.peek(0) == '_' {
                self.advance();
            }
        }
        if (self.peek(0) == 'e' || self.peek(0) == 'E')
            && (self.peek(1).is_ascii_digit()
                || ((self.peek(1) == '+' || self.peek(1) == '-') && self.peek(2).is_ascii_digit()))
        {
            is_float = true;
            self.advance();
            if self.peek(0) == '+' || self.peek(0) == '-' {
                self.advance();
            }
            while self.peek(0).is_ascii_digit() {
                self.advance();
            }
        }
        // explicit precision suffix f32/f64 (Doc 2 §6.2)
        if self.peek(0) == 'f' {
            let rest: String = [self.peek(0), self.peek(1), self.peek(2)].iter().collect();
            if rest == "f32" || rest == "f64" {
                is_float = true;
                self.advance();
                self.advance();
                self.advance();
            }
        }
        let text: String = self.chars[start..self.i].iter().collect();
        if is_float {
            self.emit(TokenKind::Float(text), span);
        } else {
            self.emit(TokenKind::Int(text), span);
        }
    }

    fn lex_string(&mut self) {
        let span = self.here();
        let start = self.i;
        self.advance(); // opening quote
        let mut closed = false;
        while !self.at_end() {
            let c = self.peek(0);
            if c == '\\' {
                self.advance();
                if !self.at_end() {
                    self.advance(); // escaped char; validated later by the parser/semantic stage
                }
                continue;
            }
            if c == '"' {
                self.advance();
                closed = true;
                break;
            }
            if c == '\n' {
                break; // unterminated on this line
            }
            self.advance();
        }
        let text: String = self.chars[start..self.i].iter().collect();
        if !closed {
            self.errors.push(Diagnostic::new("unterminated string literal", span));
            self.emit(TokenKind::Error(text), span);
        } else {
            self.emit(TokenKind::Str(text), span);
        }
    }

    fn lex_raw_string(&mut self) {
        let span = self.here();
        let start = self.i;
        self.advance(); // 'r'
        self.advance(); // opening quote
        let mut closed = false;
        while !self.at_end() {
            let c = self.peek(0);
            if c == '"' {
                self.advance();
                closed = true;
                break;
            }
            if c == '\n' {
                break;
            }
            self.advance();
        }
        let text: String = self.chars[start..self.i].iter().collect();
        if !closed {
            self.errors.push(Diagnostic::new("unterminated raw string literal", span));
            self.emit(TokenKind::Error(text), span);
        } else {
            self.emit(TokenKind::RawStr(text), span);
        }
    }

    fn lex_char(&mut self) {
        let span = self.here();
        let start = self.i;
        self.advance(); // opening '
        if self.peek(0) == '\\' {
            self.advance();
            if !self.at_end() {
                self.advance();
            }
        } else if self.peek(0) != '\'' {
            self.advance();
        }
        let mut closed = false;
        if self.peek(0) == '\'' {
            self.advance();
            closed = true;
        }
        let text: String = self.chars[start..self.i].iter().collect();
        if !closed {
            self.errors.push(Diagnostic::new("unterminated char literal", span));
            self.emit(TokenKind::Error(text), span);
        } else {
            self.emit(TokenKind::Char(text), span);
        }
    }

    /// Maximal-munch operator/delimiter lexing: 3-char operators are
    /// checked before 2-char, before 1-char, so e.g. `<<=` is never
    /// mis-tokenized as `<` `<` `=` or `<<` `=`.
    fn lex_operator_or_delim(&mut self) -> bool {
        let span = self.here();
        let c0 = self.peek(0);
        let c1 = self.peek(1);
        let c2 = self.peek(2);

        // 3-character operators
        let three: Option<Op> = match (c0, c1, c2) {
            ('<', '<', '=') => Some(Op::ShlEq),
            ('>', '>', '=') => Some(Op::ShrEq),
            ('.', '.', '=') => Some(Op::DotDotEq),
            _ => None,
        };
        if let Some(op) = three {
            self.advance();
            self.advance();
            self.advance();
            self.emit(TokenKind::Op(op), span);
            return true;
        }

        // 2-character operators
        let two: Option<Op> = match (c0, c1) {
            ('=', '=') => Some(Op::EqEq),
            ('!', '=') => Some(Op::NotEq),
            ('<', '=') => Some(Op::LtEq),
            ('>', '=') => Some(Op::GtEq),
            ('&', '&') => Some(Op::AndAnd),
            ('|', '|') => Some(Op::OrOr),
            ('-', '>') => Some(Op::Arrow),
            ('=', '>') => Some(Op::FatArrow),
            (':', ':') => Some(Op::ColonColon),
            ('?', '?') => Some(Op::QuestionQuestion),
            ('+', '=') => Some(Op::PlusEq),
            ('-', '=') => Some(Op::MinusEq),
            ('*', '=') => Some(Op::StarEq),
            ('/', '=') => Some(Op::SlashEq),
            ('%', '=') => Some(Op::PercentEq),
            ('&', '=') => Some(Op::AmpEq),
            ('|', '=') => Some(Op::PipeEq),
            ('^', '=') => Some(Op::CaretEq),
            ('<', '<') => Some(Op::Shl),
            ('>', '>') => Some(Op::Shr),
            ('.', '.') => Some(Op::DotDot),
            ('*', '*') => Some(Op::StarStar),
            _ => None,
        };
        if let Some(op) = two {
            self.advance();
            self.advance();
            self.emit(TokenKind::Op(op), span);
            return true;
        }

        // 1-character operators / delimiters
        let single: Option<TokenKind> = match c0 {
            '+' => Some(TokenKind::Op(Op::Plus)),
            '-' => Some(TokenKind::Op(Op::Minus)),
            '*' => Some(TokenKind::Op(Op::Star)),
            '/' => Some(TokenKind::Op(Op::Slash)),
            '%' => Some(TokenKind::Op(Op::Percent)),
            '<' => Some(TokenKind::Op(Op::Lt)),
            '>' => Some(TokenKind::Op(Op::Gt)),
            '=' => Some(TokenKind::Op(Op::Eq)),
            '!' => Some(TokenKind::Op(Op::Not)),
            '&' => Some(TokenKind::Op(Op::Amp)),
            '|' => Some(TokenKind::Op(Op::Pipe)),
            '^' => Some(TokenKind::Op(Op::Caret)),
            '~' => Some(TokenKind::Op(Op::Tilde)),
            '.' => Some(TokenKind::Op(Op::Dot)),
            '?' => Some(TokenKind::Op(Op::Question)),
            '@' => Some(TokenKind::Op(Op::At)),
            '#' => Some(TokenKind::Op(Op::Hash)),
            '{' => Some(TokenKind::Delim(Delim::LBrace)),
            '}' => Some(TokenKind::Delim(Delim::RBrace)),
            '(' => Some(TokenKind::Delim(Delim::LParen)),
            ')' => Some(TokenKind::Delim(Delim::RParen)),
            '[' => Some(TokenKind::Delim(Delim::LBracket)),
            ']' => Some(TokenKind::Delim(Delim::RBracket)),
            ',' => Some(TokenKind::Delim(Delim::Comma)),
            ';' => Some(TokenKind::Delim(Delim::Semi)),
            ':' => Some(TokenKind::Delim(Delim::Colon)),
            _ => None,
        };
        if let Some(kind) = single {
            self.advance();
            self.emit(kind, span);
            return true;
        }

        false
    }
}

/// Convenience entry point.
pub fn tokenize(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    Lexer::new(src).tokenize()
}

#[cfg(test)]
mod tests {
    use super::*;
    // No `use crate::token::TokenKind::*;` here — see the policy note in
    // token.rs's Keyword::from_str. TokenKind's own variants are named
    // `Keyword`, `Op`, and `Delim`, which are *also* the names of the
    // wrapped types brought in by `use super::*` above (Keyword, Op,
    // Delim are re-exported from token.rs through lexer.rs's own
    // `use crate::token::{...}`). Glob-importing both into this module
    // at once made every `Op(Op::Dot)`/`Keyword(Keyword::Let)`/
    // `Delim(Delim::Semi)` constructor call ambiguous (E0659) — Rust
    // couldn't tell whether `Op` meant the type or the `TokenKind::Op`
    // variant. Every variant below is fully qualified as `TokenKind::X`
    // instead.

    /// Strips the trailing Eof token for easier comparison against
    /// expected-kind lists in tests below.
    fn kinds(src: &str) -> Vec<TokenKind> {
        let (toks, _errs) = tokenize(src);
        toks.into_iter()
            .map(|t| t.kind)
            .filter(|k| !matches!(k, TokenKind::Eof))
            .collect()
    }

    fn texts(src: &str) -> Vec<String> {
        let (toks, _errs) = tokenize(src);
        toks.into_iter()
            .filter(|t| !matches!(t.kind, TokenKind::Eof))
            .map(|t| match t.kind {
                TokenKind::Keyword(k) => format!("{:?}", k).to_lowercase(),
                TokenKind::Ident(s) | TokenKind::Int(s) | TokenKind::IntHex(s)
                | TokenKind::IntOct(s) | TokenKind::IntBin(s) | TokenKind::Float(s)
                | TokenKind::Str(s) | TokenKind::RawStr(s) | TokenKind::Char(s)
                | TokenKind::DocComment(s) | TokenKind::Error(s) => s,
                TokenKind::Bool(b) => b.to_string(),
                TokenKind::Null => "null".to_string(),
                TokenKind::Op(o) => o.to_string(),
                TokenKind::Delim(d) => d.to_string(),
                TokenKind::Eof => unreachable!(),
            })
            .collect()
    }

    // --- Doc 2 §4/§5: identifiers & keywords ---

    #[test]
    fn primitive_type_name_is_plain_identifier() {
        assert_eq!(kinds("i32"), vec![TokenKind::Ident("i32".into())]);
    }

    #[test]
    fn keyword_recognition_doc2_examples() {
        let (toks, errs) = tokenize("let mut fn struct spawn async match ui table");
        assert!(errs.is_empty());
        assert_eq!(toks.len(), 10); // 9 keywords + Eof
        for t in &toks[..9] {
            assert!(matches!(t.kind, TokenKind::Keyword(_)), "expected keyword, got {:?}", t.kind);
        }
    }

    #[test]
    fn as_keyword_is_recognized() {
        // Regression test: 'as' (Doc 4 §6 cast operator) was initially
        // missing from the keyword table; caught during Python-harness
        // cross-check against Document 3's category list, before the
        // Rust port. See PROGRESS.md.
        assert_eq!(kinds("300 as i32"), vec![
            TokenKind::Int("300".into()),
            TokenKind::Keyword(Keyword::As),
            TokenKind::Ident("i32".into()),
        ]);
    }

    #[test]
    fn identifier_valid_forms() {
        assert_eq!(
            kinds("userName _cache HTTPRequest2 max_retries"),
            vec![
                TokenKind::Ident("userName".into()),
                TokenKind::Ident("_cache".into()),
                TokenKind::Ident("HTTPRequest2".into()),
                TokenKind::Ident("max_retries".into()),
            ]
        );
    }

    // --- Doc 2 §3: comments ---

    #[test]
    fn line_comment_discarded() {
        let (toks, errs) = tokenize("let x = 5; // trailing\nlet y = 6;");
        assert!(errs.is_empty());
        assert_eq!(toks.len(), 11); // 10 real tokens + Eof, comment gone
    }

    #[test]
    fn block_comment_nested_discarded() {
        let (toks, errs) = tokenize("let /* a /* nested */ b */ x = 1;");
        assert!(errs.is_empty());
        assert_eq!(toks.len(), 6); // let x = 1 ; + Eof
    }

    #[test]
    fn doc_comment_retained() {
        let (toks, errs) = tokenize("/// computes interest\nfn f() {}");
        assert!(errs.is_empty());
        assert!(matches!(&toks[0].kind, TokenKind::DocComment(s) if s == "/// computes interest"));
    }

    // --- Doc 2 §6.1: integer literals ---

    #[test]
    fn integer_literal_forms() {
        assert_eq!(
            kinds("42 0x2A 0o52 0b101010 1_000_000"),
            vec![
                TokenKind::Int("42".into()),
                TokenKind::IntHex("0x2A".into()),
                TokenKind::IntOct("0o52".into()),
                TokenKind::IntBin("0b101010".into()),
                TokenKind::Int("1_000_000".into()),
            ]
        );
    }

    // --- Doc 2 §6.2: float literals ---

    #[test]
    fn float_literal_forms() {
        assert_eq!(
            kinds("3.14 2.0e10 0.5f32 1.0f64"),
            vec![
                TokenKind::Float("3.14".into()),
                TokenKind::Float("2.0e10".into()),
                TokenKind::Float("0.5f32".into()),
                TokenKind::Float("1.0f64".into()),
            ]
        );
    }

    // --- Doc 2 §6.3: string/char literals ---

    #[test]
    fn string_with_escapes() {
        assert_eq!(kinds(r#""line1\nline2""#), vec![TokenKind::Str(r#""line1\nline2""#.into())]);
    }

    #[test]
    fn raw_string_no_escape_processing() {
        // Regression test: raw-string dispatch must precede the generic
        // identifier branch, or 'r' is consumed as a bare identifier
        // before the string is ever seen. Caught by the Python harness.
        assert_eq!(
            kinds(r#"r"C:\no\escapes\here""#),
            vec![TokenKind::RawStr(r#"r"C:\no\escapes\here""#.into())]
        );
    }

    #[test]
    fn char_literal() {
        assert_eq!(kinds("'a'"), vec![TokenKind::Char("'a'".into())]);
    }

    #[test]
    fn unterminated_string_is_recoverable_not_a_crash() {
        let (_toks, errs) = tokenize("\"unterminated");
        assert_eq!(errs.len(), 1);
    }

    // --- Doc 2 §6.4: bool/null ---

    #[test]
    fn bool_null_literals() {
        assert_eq!(
            kinds("true false null"),
            vec![TokenKind::Bool(true), TokenKind::Bool(false), TokenKind::Null]
        );
    }

    // --- Doc 4: full operator round-trip ---

    #[test]
    fn arithmetic_operators() {
        assert_eq!(
            kinds("+ - * / % **"),
            vec![
                TokenKind::Op(Op::Plus), TokenKind::Op(Op::Minus), TokenKind::Op(Op::Star),
                TokenKind::Op(Op::Slash), TokenKind::Op(Op::Percent), TokenKind::Op(Op::StarStar),
            ]
        );
    }

    #[test]
    fn compound_assignment_maximal_munch() {
        assert_eq!(
            texts("+= -= *= /= %= &= |= ^= <<= >>="),
            vec!["+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="]
        );
    }

    #[test]
    fn maximal_munch_shl_eq_not_three_tokens() {
        assert_eq!(
            kinds("x <<= 1;"),
            vec![
                TokenKind::Ident("x".into()),
                TokenKind::Op(Op::ShlEq),
                TokenKind::Int("1".into()),
                TokenKind::Delim(Delim::Semi),
            ]
        );
    }

    #[test]
    fn range_exclusive_vs_inclusive() {
        assert_eq!(texts("0..10 0..=10"), vec!["0", "..", "10", "0", "..=", "10"]);
    }

    #[test]
    fn question_question_is_one_token() {
        assert_eq!(kinds("a ?? b"), vec![
            TokenKind::Ident("a".into()),
            TokenKind::Op(Op::QuestionQuestion),
            TokenKind::Ident("b".into()),
        ]);
    }

    #[test]
    fn postfix_propagation_then_member_are_separate_tokens() {
        assert_eq!(
            kinds("result?.field"),
            vec![
                TokenKind::Ident("result".into()),
                TokenKind::Op(Op::Question),
                TokenKind::Op(Op::Dot),
                TokenKind::Ident("field".into()),
            ]
        );
    }

    // --- Doc 2 §8: compile-target directives ---

    #[test]
    fn compile_target_directive_tokens() {
        assert_eq!(
            texts("#target(native) { }"),
            vec!["#", "target", "(", "native", ")", "{", "}"]
        );
    }

    // --- Doc 17 §2: error recovery ---

    #[test]
    fn invalid_character_recoverable_lexing_continues() {
        let (toks, errs) = tokenize("let x = 5 $ let y = 6;");
        assert_eq!(errs.len(), 1);
        // lexing continued past the bad char: both `let` statements present
        let kw_count = toks.iter()
            .filter(|t| matches!(t.kind, TokenKind::Keyword(Keyword::Let)))
            .count();
        assert_eq!(kw_count, 2);
    }

    // --- Full realistic snippet, Doc 2 §7 ---

    #[test]
    fn realistic_snippet_round_trip() {
        assert_eq!(
            texts("let x: i32 = 5;\nlet y = x + 10;"),
            vec!["let", "x", ":", "i32", "=", "5", ";",
                 "let", "y", "=", "x", "+", "10", ";"]
        );
    }

    #[test]
    fn span_line_col_tracking() {
        let (toks, _errs) = tokenize("let x\n  = 5;");
        // 'x' on line 1
        let x_tok = &toks[1];
        assert_eq!(x_tok.span.line, 1);
        // '=' on line 2, col 3 (after two leading spaces)
        let eq_tok = &toks[2];
        assert_eq!(eq_tok.span.line, 2);
        assert_eq!(eq_tok.span.col, 3);
    }
}
