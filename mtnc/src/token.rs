//! Token types for the Mountain lexer.
//! Spec: Document 2 (Lexical Structure & Tokens), keyword table sourced
//! from Document 3 (Full Keyword List & Definitions), operator set from
//! Document 4 (Operators & Precedence Table).

use std::fmt;

/// Source location of a token: 1-indexed line and column, matching the
/// diagnostic format required by Document 22 (`file:line:column`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// Reserved words. This list is deliberately the *complete* set from
/// Document 3, Categories A-L (excluding the @-prefixed attribute names,
/// which are lexed as plain identifiers following a separate `@` token —
/// see Category L). `self`/`Self`, `true`/`false`/`null`, and `as` are
/// included even though they sit in slightly different places across
/// Documents 2/3/4/5, since lexically they are all word-shaped reserved
/// tokens. Document 3 §12 states more domain-specific keywords will be
/// introduced by Documents 6, 11, 12, 16, 18, 19, 20 — this enum is the
/// deliberate extension point for those additions in later phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Keyword {
    // Category A — Declarations & Bindings
    Let, Mut, Const, Static, Fn, Return, Struct, Enum, Trait, Impl, Type,
    Mod, Use, Pub, Export, Import,
    // Category B — Control Flow
    If, Else, Match, Case, Default, Loop, While, For, In, Break, Continue,
    Yield, Do,
    // Category C — Ownership, Borrowing & Memory
    Own, Borrow, Ref, Move, Copy, Clone, Drop, Box, Weak, Unsafe, Alloc,
    Dealloc, Ptr,
    // Category D — Concurrency
    Async, Await, Spawn, Thread, Channel, Send, Recv, Select, Mutex,
    RwLock, Atomic, Spin, Join, Sync, Actor, Message,
    // Category E — Error Handling
    Result, Ok, Err, Option, Some, None, Try, Catch, Throw, Panic, Assert,
    Ensure, Recover,
    // Category F — Types & Generics
    Where, Dyn, Sized, From, Into, As,
    // Category G — UI / Frontend
    Ui, Component, State, Prop, Render, On, Bind, Style, Layout, Mount,
    Unmount,
    // Category H — Database / Query
    // NOTE: `select` is deliberately not re-declared here — it's the same
    // token as Category D's concurrency `select`. See the comment on the
    // "select" arm in `from_str` below.
    Table, Query, Index, Insert, Update, Delete, Schema,
    Transaction, Commit, Rollback,
    // Category I — Networking / IPC
    Server, Client, Listen, Connect, Socket, Stream, Packet, Broadcast,
    Subscribe, Publish,
    // Category J — AI / Numeric / Statistics
    Tensor, Matrix, Vector, Dataset, Model, Layer, Train, Predict,
    Gradient, Epoch, Batch,
    // Category K — Systems / Low-Level Interop
    Asm, Syscall, Volatile, Align, Pack, Extern, Abi,
    // Self-reference (Document 7 §4.1 / §2.3)
    SelfValue, // `self`
    SelfType,  // `Self`
}

impl Keyword {
    /// Looks up a reserved word by its exact source text. Returns `None`
    /// for anything that isn't a keyword (including primitive type names
    /// like `i32`/`f64`/`bool`/`String` — see the design note below).
    ///
    /// Design decision (Phase 1): primitive type names are deliberately
    /// *not* lexer-level keywords, matching Rust's own lexer design
    /// (rustc lexes `i32` as a plain identifier and resolves it to a
    /// builtin type during name resolution, not during lexing). Document
    /// 2/3 do not list primitive types under "Keywords" — only structural
    /// words like `let`/`fn`/`struct` — so this is a defensible reading,
    /// not an invented rule. Flagged here explicitly per the "no silent
    /// assumptions" instruction; revisit in Phase 3 (Type System) if this
    /// turns out to be wrong.
    pub fn from_str(s: &str) -> std::option::Option<Keyword> {
        // Deliberately NOT `use Keyword::*;` here. Several Mountain
        // keywords are spelled identically to items the Rust prelude
        // auto-imports (Result, Ok, Err, Option, Some, None, Copy, Clone,
        // Drop, Send, Sync, Sized, Default, From, Into, Box, Fn — all of
        // these are both Document 3 keywords *and* std prelude traits/
        // types/constructors). A glob import of Keyword's own variants
        // shadows those prelude names within this function and silently
        // breaks unqualified `Some(...)`/`None`/etc. below — this is
        // exactly the bug a real `cargo test` run caught here (E0308 on
        // `return None` resolving to the unit variant `Keyword::None`
        // instead of `std::option::Option::None`, and E0618 on `Some(x)`
        // resolving to the non-callable unit variant `Keyword::Some`).
        // Fix, applied generally rather than as a one-off: every
        // `Keyword` variant below is fully qualified, and the return
        // type itself is written as `std::option::Option` for the same
        // reason. This same policy is applied to the `Op`/`Delim`
        // `Display` impls further down, even though those two weren't
        // actually broken (pattern-position matching resolves only
        // through the value namespace, so it doesn't hit this ambiguity
        // the way constructor-call syntax does) — kept consistent so the
        // "never glob-import a local enum here" rule has no exceptions
        // for a future reader to trip over.
        std::option::Option::Some(match s {
            "let" => Keyword::Let, "mut" => Keyword::Mut, "const" => Keyword::Const,
            "static" => Keyword::Static, "fn" => Keyword::Fn, "return" => Keyword::Return,
            "struct" => Keyword::Struct, "enum" => Keyword::Enum, "trait" => Keyword::Trait,
            "impl" => Keyword::Impl, "type" => Keyword::Type, "mod" => Keyword::Mod,
            "use" => Keyword::Use, "pub" => Keyword::Pub, "export" => Keyword::Export,
            "import" => Keyword::Import,

            "if" => Keyword::If, "else" => Keyword::Else, "match" => Keyword::Match,
            "case" => Keyword::Case, "default" => Keyword::Default, "loop" => Keyword::Loop,
            "while" => Keyword::While, "for" => Keyword::For, "in" => Keyword::In,
            "break" => Keyword::Break, "continue" => Keyword::Continue,
            "yield" => Keyword::Yield, "do" => Keyword::Do,

            "own" => Keyword::Own, "borrow" => Keyword::Borrow, "ref" => Keyword::Ref,
            "move" => Keyword::Move, "copy" => Keyword::Copy, "clone" => Keyword::Clone,
            "drop" => Keyword::Drop, "box" => Keyword::Box, "weak" => Keyword::Weak,
            "unsafe" => Keyword::Unsafe, "alloc" => Keyword::Alloc,
            "dealloc" => Keyword::Dealloc, "ptr" => Keyword::Ptr,

            "async" => Keyword::Async, "await" => Keyword::Await, "spawn" => Keyword::Spawn,
            "thread" => Keyword::Thread, "channel" => Keyword::Channel, "send" => Keyword::Send,
            "recv" => Keyword::Recv, "select" => Keyword::Select, "mutex" => Keyword::Mutex,
            "rwlock" => Keyword::RwLock, "atomic" => Keyword::Atomic, "spin" => Keyword::Spin,
            "join" => Keyword::Join, "sync" => Keyword::Sync, "actor" => Keyword::Actor,
            "message" => Keyword::Message,

            "Result" => Keyword::Result, "Ok" => Keyword::Ok, "Err" => Keyword::Err,
            "Option" => Keyword::Option, "Some" => Keyword::Some, "None" => Keyword::None,
            "try" => Keyword::Try, "catch" => Keyword::Catch, "throw" => Keyword::Throw,
            "panic" => Keyword::Panic, "assert" => Keyword::Assert,
            "ensure" => Keyword::Ensure, "recover" => Keyword::Recover,

            "where" => Keyword::Where, "dyn" => Keyword::Dyn, "sized" => Keyword::Sized,
            "from" => Keyword::From, "into" => Keyword::Into, "as" => Keyword::As,

            "ui" => Keyword::Ui, "component" => Keyword::Component, "state" => Keyword::State,
            "prop" => Keyword::Prop, "render" => Keyword::Render, "on" => Keyword::On,
            "bind" => Keyword::Bind, "style" => Keyword::Style, "layout" => Keyword::Layout,
            "mount" => Keyword::Mount, "unmount" => Keyword::Unmount,

            "table" => Keyword::Table, "query" => Keyword::Query, "index" => Keyword::Index,
            // NOTE: "select" is intentionally handled once, above, for the
            // concurrency `select { case ... }` construct (Doc 3 Category
            // D). Document 3 also lists `select`/`insert`/`update`/`delete`
            // as query-context CRUD keywords (Category H) — but it is the
            // *same lexical token* reused in a different grammatical
            // context, not a second reserved word, so it gets exactly one
            // `Keyword::Select` variant. A duplicate `"select" => ...`
            // arm here was the E0428 "defined multiple times" compile
            // error from the first CI run. Disambiguating "am I inside a
            // `select {}` block or a `query` expression" is the Parser's
            // job (Phase 2+), not the Lexer's — the lexer only needs to
            // agree that both spellings produce one token kind.
            "insert" => Keyword::Insert, "update" => Keyword::Update,
            "delete" => Keyword::Delete, "schema" => Keyword::Schema,
            "transaction" => Keyword::Transaction, "commit" => Keyword::Commit,
            "rollback" => Keyword::Rollback,

            "server" => Keyword::Server, "client" => Keyword::Client,
            "listen" => Keyword::Listen, "connect" => Keyword::Connect,
            "socket" => Keyword::Socket, "stream" => Keyword::Stream,
            "packet" => Keyword::Packet, "broadcast" => Keyword::Broadcast,
            "subscribe" => Keyword::Subscribe, "publish" => Keyword::Publish,

            "tensor" => Keyword::Tensor, "matrix" => Keyword::Matrix,
            "vector" => Keyword::Vector, "dataset" => Keyword::Dataset,
            "model" => Keyword::Model, "layer" => Keyword::Layer,
            "train" => Keyword::Train, "predict" => Keyword::Predict,
            "gradient" => Keyword::Gradient, "epoch" => Keyword::Epoch,
            "batch" => Keyword::Batch,

            "asm" => Keyword::Asm, "syscall" => Keyword::Syscall,
            "volatile" => Keyword::Volatile, "align" => Keyword::Align,
            "pack" => Keyword::Pack, "extern" => Keyword::Extern, "abi" => Keyword::Abi,

            "self" => Keyword::SelfValue, "Self" => Keyword::SelfType,

            _ => return std::option::Option::None,
        })
    }
}

/// Operator tokens (Document 4). Word-shaped operators (`as`) are
/// `Keyword`s, not `Op`s — this enum is exclusively symbolic operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Plus, Minus, Star, Slash, Percent, StarStar,          // arithmetic
    EqEq, NotEq, Lt, Gt, LtEq, GtEq,                       // comparison
    AndAnd, OrOr, Not,                                     // logical
    Amp, Pipe, Caret, Tilde, Shl, Shr,                     // bitwise
    Eq, PlusEq, MinusEq, StarEq, SlashEq, PercentEq,       // assignment
    AmpEq, PipeEq, CaretEq, ShlEq, ShrEq,
    DotDot, DotDotEq,                                      // range
    Question, QuestionQuestion,                            // error/optional
    Dot, ColonColon, Arrow, FatArrow,                      // path/member
    At, Hash,                                              // attribute/directive
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // No `use Op::*;` here — see the policy note in Keyword::from_str.
        let s = match self {
            Op::Plus => "+", Op::Minus => "-", Op::Star => "*", Op::Slash => "/",
            Op::Percent => "%", Op::StarStar => "**", Op::EqEq => "==",
            Op::NotEq => "!=", Op::Lt => "<", Op::Gt => ">", Op::LtEq => "<=",
            Op::GtEq => ">=", Op::AndAnd => "&&", Op::OrOr => "||", Op::Not => "!",
            Op::Amp => "&", Op::Pipe => "|", Op::Caret => "^", Op::Tilde => "~",
            Op::Shl => "<<", Op::Shr => ">>", Op::Eq => "=", Op::PlusEq => "+=",
            Op::MinusEq => "-=", Op::StarEq => "*=", Op::SlashEq => "/=",
            Op::PercentEq => "%=", Op::AmpEq => "&=", Op::PipeEq => "|=",
            Op::CaretEq => "^=", Op::ShlEq => "<<=", Op::ShrEq => ">>=",
            Op::DotDot => "..", Op::DotDotEq => "..=", Op::Question => "?",
            Op::QuestionQuestion => "??", Op::Dot => ".", Op::ColonColon => "::",
            Op::Arrow => "->", Op::FatArrow => "=>", Op::At => "@", Op::Hash => "#",
        };
        write!(f, "{}", s)
    }
}

/// Delimiter tokens (Document 2 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delim {
    LBrace, RBrace, LParen, RParen, LBracket, RBracket,
    Comma, Semi, Colon,
}

impl fmt::Display for Delim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Delim::LBrace => "{", Delim::RBrace => "}", Delim::LParen => "(",
            Delim::RParen => ")", Delim::LBracket => "[", Delim::RBracket => "]",
            Delim::Comma => ",", Delim::Semi => ";", Delim::Colon => ":",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Keyword(Keyword),
    Ident(String),
    Int(String),
    IntHex(String),
    IntOct(String),
    IntBin(String),
    Float(String),
    Str(String),
    RawStr(String),
    Char(String),
    Bool(bool),
    Null,
    DocComment(String),
    Op(Op),
    Delim(Delim),
    /// A recoverable lexical error: the offending text is preserved so the
    /// parser (a later phase) can still make forward progress instead of
    /// aborting the whole compilation (Document 17 §2's error-recovery
    /// requirement; Document 25 Phase 1 exit criteria).
    Error(String),
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Token { kind, span }
    }
}
