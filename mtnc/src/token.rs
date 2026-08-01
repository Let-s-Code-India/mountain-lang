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
    Table, Query, Index, Select, Insert, Update, Delete, Schema,
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
    pub fn from_str(s: &str) -> Option<Keyword> {
        use Keyword::*;
        Some(match s {
            "let" => Let, "mut" => Mut, "const" => Const, "static" => Static,
            "fn" => Fn, "return" => Return, "struct" => Struct, "enum" => Enum,
            "trait" => Trait, "impl" => Impl, "type" => Type, "mod" => Mod,
            "use" => Use, "pub" => Pub, "export" => Export, "import" => Import,

            "if" => If, "else" => Else, "match" => Match, "case" => Case,
            "default" => Default, "loop" => Loop, "while" => While, "for" => For,
            "in" => In, "break" => Break, "continue" => Continue, "yield" => Yield,
            "do" => Do,

            "own" => Own, "borrow" => Borrow, "ref" => Ref, "move" => Move,
            "copy" => Copy, "clone" => Clone, "drop" => Drop, "box" => Box,
            "weak" => Weak, "unsafe" => Unsafe, "alloc" => Alloc,
            "dealloc" => Dealloc, "ptr" => Ptr,

            "async" => Async, "await" => Await, "spawn" => Spawn,
            "thread" => Thread, "channel" => Channel, "send" => Send,
            "recv" => Recv, "select" => Select, "mutex" => Mutex,
            "rwlock" => RwLock, "atomic" => Atomic, "spin" => Spin,
            "join" => Join, "sync" => Sync, "actor" => Actor, "message" => Message,

            "Result" => Result, "Ok" => Ok, "Err" => Err, "Option" => Option,
            "Some" => Some, "None" => None, "try" => Try, "catch" => Catch,
            "throw" => Throw, "panic" => Panic, "assert" => Assert,
            "ensure" => Ensure, "recover" => Recover,

            "where" => Where, "dyn" => Dyn, "sized" => Sized, "from" => From,
            "into" => Into, "as" => As,

            "ui" => Ui, "component" => Component, "state" => State,
            "prop" => Prop, "render" => Render, "on" => On, "bind" => Bind,
            "style" => Style, "layout" => Layout, "mount" => Mount,
            "unmount" => Unmount,

            "table" => Table, "query" => Query, "index" => Index,
            "insert" => Insert, "update" => Update, "delete" => Delete,
            "schema" => Schema, "transaction" => Transaction,
            "commit" => Commit, "rollback" => Rollback,

            "server" => Server, "client" => Client, "listen" => Listen,
            "connect" => Connect, "socket" => Socket, "stream" => Stream,
            "packet" => Packet, "broadcast" => Broadcast,
            "subscribe" => Subscribe, "publish" => Publish,

            "tensor" => Tensor, "matrix" => Matrix, "vector" => Vector,
            "dataset" => Dataset, "model" => Model, "layer" => Layer,
            "train" => Train, "predict" => Predict, "gradient" => Gradient,
            "epoch" => Epoch, "batch" => Batch,

            "asm" => Asm, "syscall" => Syscall, "volatile" => Volatile,
            "align" => Align, "pack" => Pack, "extern" => Extern, "abi" => Abi,

            "self" => SelfValue, "Self" => SelfType,

            _ => return None,
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
        use Op::*;
        let s = match self {
            Plus => "+", Minus => "-", Star => "*", Slash => "/", Percent => "%",
            StarStar => "**", EqEq => "==", NotEq => "!=", Lt => "<", Gt => ">",
            LtEq => "<=", GtEq => ">=", AndAnd => "&&", OrOr => "||", Not => "!",
            Amp => "&", Pipe => "|", Caret => "^", Tilde => "~", Shl => "<<",
            Shr => ">>", Eq => "=", PlusEq => "+=", MinusEq => "-=",
            StarEq => "*=", SlashEq => "/=", PercentEq => "%=", AmpEq => "&=",
            PipeEq => "|=", CaretEq => "^=", ShlEq => "<<=", ShrEq => ">>=",
            DotDot => "..", DotDotEq => "..=", Question => "?",
            QuestionQuestion => "??", Dot => ".", ColonColon => "::",
            Arrow => "->", FatArrow => "=>", At => "@", Hash => "#",
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
        use Delim::*;
        let s = match self {
            LBrace => "{", RBrace => "}", LParen => "(", RParen => ")",
            LBracket => "[", RBracket => "]", Comma => ",", Semi => ";",
            Colon => ":",
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
