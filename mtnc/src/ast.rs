//! Untyped AST for Mountain, per Document 17 §3 ("Produces an untyped
//! AST... no type information resolved yet, only syntactic structure")
//! and Document 23 (the authoritative EBNF — ground truth over any
//! inline grammar snippet in an earlier document per this phase's
//! instructions).
//!
//! Naming-collision policy carried over from Phase 1 (see token.rs):
//! several AST enums here have variants that share a name with another
//! type in this module (e.g. `Stmt::Item(Item)`, `ItemKind::Fn(FnDecl)`).
//! That's fine on its own — the actual danger (proven in Phase 1's CI
//! run #2) is glob-importing both a type and a same-named wrapping
//! variant into one scope at once. Nothing in this file or `parser.rs`
//! does that; every reference is written as `EnumName::Variant`.

use crate::token::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub attrs: Vec<Attribute>,
    pub visibility: Visibility,
    pub kind: ItemKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Visibility {
    Private,
    Pub,
    PubPackage,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    Fn(FnDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Trait(TraitDecl),
    Impl(ImplDecl),
    Mod(ModDecl),
    Use(UsePath),
    Import(Vec<String>),
    Const(ConstDecl),
    Static(StaticDecl),
    TypeAlias(TypeAliasDecl),
    Table(TableDecl),
    Index(IndexDecl),
    Ui(UiDecl),
    Component(UiDecl),
    Server(ServerDecl),
    Schema(SchemaDecl),
    Actor(ActorDecl),
    /// A `#target(...) { items }` block used at item level (Document 2
    /// §8's examples show this both at top level and inside function
    /// bodies; see `Stmt::TargetBlock` for the statement-position form).
    TargetBlock(TargetKind, Vec<Item>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TargetKind {
    Native,
    Wasm,
    All,
}

// ---------- Generics ----------

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GenericParams(pub Vec<GenericParam>);

#[derive(Debug, Clone, PartialEq)]
pub enum GenericParam {
    Type { name: String, bounds: Vec<TraitBound> },
    Const { name: String, ty: Type },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitBound {
    pub name: String,
    pub args: Vec<Type>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WhereClause(pub Vec<WhereBound>);

#[derive(Debug, Clone, PartialEq)]
pub struct WhereBound {
    pub name: String,
    pub bounds: Vec<TraitBound>,
}

// ---------- Declarations ----------

#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    pub is_async: bool,
    pub name: String,
    pub generics: GenericParams,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub where_clause: WhereClause,
    /// `None` for a trait method signature with no default body
    /// (Document 23 §3's `trait_item ::= fn_sig ";"` alternative).
    pub body: Option<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OwnershipMod {
    None,
    Borrow,
    BorrowMut,
    Move,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub ownership: OwnershipMod,
    pub name: String,
    pub is_variadic: bool,
    pub ty: Type,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub name: String,
    pub generics: GenericParams,
    pub body: StructBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructBody {
    Named(Vec<FieldDecl>),
    Tuple(Vec<Type>),
    Unit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    pub visibility: Visibility,
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub name: String,
    pub generics: GenericParams,
    pub variants: Vec<EnumVariant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub data: Vec<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitDecl {
    pub name: String,
    pub generics: GenericParams,
    pub items: Vec<TraitItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TraitItem {
    AssocType(String),
    Fn(FnDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeRef {
    pub name: String,
    pub args: Vec<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImplDecl {
    pub generics: GenericParams,
    /// `Some` for `impl Trait for Type { }`, `None` for inherent `impl Type { }`.
    pub trait_ref: Option<TypeRef>,
    pub target: TypeRef,
    pub where_clause: WhereClause,
    pub items: Vec<ImplItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImplItem {
    Fn(FnDecl),
    AssocType(String, Type),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModDecl {
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsePath {
    pub path: Vec<String>,
    /// `use a::b::{X, Y};` — empty when not present (plain `use a::b::X;`,
    /// modeled as `path = [a,b,X]`, `items = []`).
    pub items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstDecl {
    pub name: String,
    pub ty: Type,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StaticDecl {
    pub name: String,
    pub ty: Type,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeAliasDecl {
    pub name: String,
    pub generics: GenericParams,
    pub ty: Type,
}

// ---------- Database (Document 20 / Doc23 §11) ----------

#[derive(Debug, Clone, PartialEq)]
pub struct TableDecl {
    pub name: String,
    pub fields: Vec<TableField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableField {
    pub name: String,
    pub ty: Type,
    pub constraints: Vec<FieldConstraint>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldConstraint {
    PrimaryKey,
    AutoIncrement,
    Unique,
    NotNull,
    Default(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexDecl {
    pub name: String,
    pub table: String,
    pub column: String,
    pub using_hash: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaDecl {
    pub name: String,
    pub versions: Vec<(u64, Vec<Item>)>,
}

// ---------- Networking (Document 19 / Doc23 §12) ----------

#[derive(Debug, Clone, PartialEq)]
pub struct ServerDecl {
    pub name: String,
    pub items: Vec<ServerItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ServerItem {
    Listen(Vec<Arg>),
    On(OnHandler),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OnHandler {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Block,
}

// ---------- Concurrency (Document 12 / Doc23 §9) ----------

#[derive(Debug, Clone, PartialEq)]
pub struct ActorDecl {
    pub name: String,
    pub items: Vec<ActorItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActorItem {
    State(StateDecl),
    On(OnHandler),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectArm {
    pub pattern: Pattern,
    pub channel_expr: Expr,
    pub body: Expr,
}

// ---------- UI (Document 18 / Doc23 §10) ----------

#[derive(Debug, Clone, PartialEq)]
pub struct UiDecl {
    pub name: String,
    pub items: Vec<UiItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiItem {
    State(StateDecl),
    Prop(PropDecl),
    Render(Block),
    Mount(Block),
    Unmount(Block),
    /// FLAGGED DEVIATION FROM DOCUMENT 23 §10 — see PROGRESS.md's Phase 2
    /// "flagged spec gaps" section. Document 23's `ui_item` production
    /// only lists `state_decl | prop_decl | render_block | mount_block |
    /// unmount_block`; it does not include `fn_decl`. But Document 24 §2
    /// (the Todo App example, which Phase 2's own exit criteria requires
    /// to round-trip) declares `fn addTask(borrow mut self) { ... }` and
    /// `fn removeTask(...)` directly inside a `ui TodoApp { }` block.
    /// Since Doc 24's example is both a required exit-criteria target and
    /// the only concrete evidence of intent here, this variant was added
    /// so the example parses, rather than silently dropping the example
    /// or silently rewriting it. Needs explicit sign-off — see report.
    Fn(FnDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateDecl {
    pub name: String,
    pub ty: Type,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropDecl {
    pub name: String,
    pub ty: Type,
}

// ---------- Types (Doc23 §4) ----------

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// Primitive type names (`i32`, `f64`, `bool`, `String`, `str`, ...).
    /// Consistent with the Phase 1 lexer decision: these are lexed as
    /// plain identifiers, not keywords, so the *parser* is what
    /// recognizes them by text. See `parse_type` in parser.rs.
    Primitive(String),
    Named(String, Vec<Type>),
    Array(Box<Type>, Option<Box<Expr>>),
    Tuple(Vec<Type>),
    Ref { lifetime: Option<String>, mutable: bool, inner: Box<Type> },
    Dyn(String, Vec<Type>),
    Fn(Vec<Type>, Box<Type>),
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),
    Unit,
    Never,
    /// A const-generic argument value used in generic-argument position
    /// (Document 8 §8's `Matrix<f64, 2, 3>` — the `2`/`3` are const
    /// values, not types). FLAGGED DEVIATION FROM DOCUMENT 23: its
    /// `generic_args` production only allows `type`, not const
    /// expressions; needed for Document 8/24's const-generic examples
    /// to parse at all. Needs explicit sign-off — see report.
    ConstArg(Box<Expr>),
}

// ---------- Patterns (Doc23 §7) ----------

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Literal(Literal),
    Ident(String),
    Wildcard,
    Mut(String),
    TupleStruct(String, Vec<Pattern>),
    Tuple(Vec<Pattern>),
    Array(Vec<Pattern>, Option<Option<String>>),
    Or(Vec<Pattern>),
}

// ---------- Statements & blocks (Doc23 §5) ----------

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub tail: Option<Box<Expr>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let { mutable: bool, pattern: Pattern, ty: Option<Type>, value: Option<Expr> },
    Expr(Expr),
    Item(Box<Item>),
    Return(Option<Expr>),
    Break { label: Option<String>, value: Option<Expr> },
    Continue { label: Option<String> },
    Yield(Expr),
    TargetBlock(TargetKind, Block),
}

// ---------- Expressions (Doc23 §6) ----------

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
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
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Not,
    BitNot,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOp {
    Add, Sub, Mul, Div, Mod, Pow,
    EqEq, NotEq, Lt, Gt, LtEq, GtEq,
    AndAnd, OrOr,
    BitAnd, BitOr, BitXor, Shl, Shr,
    Coalesce,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AssignOp {
    Eq, PlusEq, MinusEq, StarEq, SlashEq, PercentEq,
    AmpEq, PipeEq, CaretEq, ShlEq, ShrEq,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub name: Option<String>,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IfExpr {
    pub cond: Expr,
    pub then_block: Block,
    pub else_branch: Option<ElseBranch>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ElseBranch {
    If(Box<IfExpr>),
    Block(Block),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchExpr {
    pub scrutinee: Expr,
    pub arms: Vec<MatchArm>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub patterns: Vec<Pattern>,
    pub guard: Option<Expr>,
    pub body: MatchArmBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchArmBody {
    Expr(Box<Expr>),
    Block(Block),
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoopExpr {
    Loop { label: Option<String>, body: Block },
    While { label: Option<String>, cond: Box<Expr>, body: Block },
    For { label: Option<String>, pattern: Pattern, iter: Box<Expr>, body: Block },
    DoWhile { body: Block, cond: Box<Expr> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClosureExpr {
    pub is_move: bool,
    /// FLAGGED DEVIATION FROM DOCUMENT 23 — `closure_expr`'s grammar
    /// (§6) only allows `IDENT (":" type)?` per parameter, unlike
    /// function parameters, `let` bindings, and `match` arms, which all
    /// use the general `pattern` production (§7). Document 24 §3 uses
    /// tuple-destructuring closure parameters (`|(input, _)| ...`),
    /// which only fits the grammar if closures accept the same
    /// `pattern` production everywhere else does. This is a genuine
    /// EBNF gap/clarification, not a differing snippet — widened here
    /// to `Pattern` (reusing the same pattern parser as `fn`/`let`/
    /// `match`, not a second one) rather than inventing a
    /// closure-specific destructuring syntax. Needs explicit sign-off.
    pub params: Vec<(Pattern, Option<Type>)>,
    pub return_type: Option<Type>,
    pub body: ClosureBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClosureBody {
    Expr(Box<Expr>),
    Block(Block),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpawnBody {
    Block(Block),
    Closure(Box<ClosureExpr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal),
    Ident(String),
    Path(Vec<String>),
    Paren(Box<Expr>),
    Tuple(Vec<Expr>),
    Array(Vec<Expr>),
    StructLit { name: String, fields: Vec<(String, Expr)>, spread: Option<Box<Expr>> },
    If(Box<IfExpr>),
    Match(Box<MatchExpr>),
    Loop(Box<LoopExpr>),
    Closure(Box<ClosureExpr>),
    Block(Box<Block>),
    Await(Box<Expr>),
    Borrow { mutable: bool, expr: Box<Expr> },
    Unary { op: UnaryOp, expr: Box<Expr> },
    Binary { op: BinaryOp, lhs: Box<Expr>, rhs: Box<Expr> },
    Assign { op: AssignOp, lhs: Box<Expr>, rhs: Box<Expr> },
    Cast { expr: Box<Expr>, ty: Box<Type> },
    Range { lo: Box<Expr>, hi: Box<Expr>, inclusive: bool },
    Propagate(Box<Expr>),
    Field { expr: Box<Expr>, name: String },
    Index { expr: Box<Expr>, index: Box<Expr> },
    Call { callee: Box<Expr>, args: Vec<Arg> },
    MethodCall { receiver: Box<Expr>, name: String, args: Vec<Arg> },
    Spawn { is_move: bool, body: SpawnBody },
    Select(Vec<SelectArm>),
    Query(QueryExpr),
    Yield(Box<Expr>),
    /// FLAGGED DEVIATION FROM DOCUMENT 23 §8 — see PROGRESS.md. Doc23's
    /// `try_stmt` is statement-shaped only (`"try" block "catch" "("
    /// IDENT ")" block`) and isn't listed among `primary_expr`'s
    /// alternatives. But Document 24 §1 uses `try { ... } catch (e) {
    /// ... }` as the right-hand side of `let body = try { ... } catch
    /// (e) { ... };` — an expression position. Modeled as an expression
    /// variant (consistent with how if/match/loop are already
    /// expressions elsewhere in the grammar) so Doc24 §1 round-trips.
    /// Needs explicit sign-off — see report.
    TryCatch { try_block: Block, catch_var: String, catch_block: Block },
    /// FLAGGED DEVIATION FROM DOCUMENT 23 — Document 23's `return_stmt`
    /// is statement-only, but Document 24 §1 uses bare `return expr`
    /// directly as a match-arm body with no enclosing block (e.g.
    /// `_ => return HttpResponse::notFound(),`), which only fits
    /// Doc23's `match_arm ::= pattern ... "=>" (expr | block)` if
    /// `return` is itself a valid `expr`. Needs explicit sign-off.
    Return(Option<Box<Expr>>),
    Throw(Box<Expr>),
    /// FLAGGED DEVIATION FROM DOCUMENT 23 — the `style { ... }` postfix
    /// modifier (Document 18 §7: `Button(...) style { background: ...,
    /// padding: 12, ... }`) has no corresponding production anywhere in
    /// Document 23's EBNF at all (not a differing snippet — a missing
    /// one). Grounded in Document 18 §7's concrete example rather than
    /// invented from nothing, but still needs explicit sign-off — see
    /// report.
    Styled { expr: Box<Expr>, props: Vec<(String, Expr)> },
    /// FLAGGED DEVIATION FROM DOCUMENT 23 — same situation as `Styled`,
    /// for the `layout { props } { children }` postfix modifier
    /// (Document 18 §7.1). Needs explicit sign-off — see report.
    Layout { expr: Box<Expr>, props: Vec<(String, Expr)>, children: Vec<Expr> },
    /// FLAGGED DEVIATION FROM DOCUMENT 23 — `Name { child1, child2, ... }`
    /// (Document 18's `Column { Text("Left"), Text("Right") }` pattern,
    /// used constantly in Document 24 §2/§6). Syntactically ambiguous
    /// with a struct literal (`Name { field: expr, ... }`) at the token
    /// level; disambiguated by lookahead in the parser (does the first
    /// item look like `word :`, i.e. a field label). Not in Document 23
    /// at all — same situation as `Styled`/`Layout`. Needs sign-off.
    ComponentChildren { name: String, children: Vec<Expr> },
    /// Document 18 §5: `on: <eventName> => <expression or closure>`
    /// (e.g. `Button("Add", on: click => addTask())`). Not a general
    /// expression production — Document 18 §5 explicitly scopes this
    /// syntax to the `on:` argument-value position specifically; `click`
    /// here is a literal event-name marker, not a variable or pattern,
    /// and there's no general `IDENT "=>" expr` production anywhere in
    /// Document 23's core expression grammar (only `match_arm` and
    /// `closure_expr` use `=>`, both with different left-hand shapes).
    /// Special-cased in `parse_arg` for the `on:` label only, not
    /// wired into general expression parsing. `OnHandler` (used by
    /// `server`/`actor` blocks: `on IDENT(params) { block }`) doesn't
    /// fit — that's a declaration-shaped construct with a name and
    /// parameter list, not an argument value.
    EventHandler { event: String, body: Box<Expr> },
    /// `unsafe { ... }` (Document 3 Category C, Document 6 SS7, Document
    /// 13 SS5). Needed for Phase 3's null/unsafe interaction (Document 5
    /// SS2.6: `null` is only usable in unsafe/FFI-typed contexts) — this
    /// was a real gap in Phase 2 (no unsafe-block parsing existed at
    /// all), closed here since Phase 3's exit criteria needs it.
    Unsafe(Box<Block>),
}

// ---------- Database query expression (Document 20 / Doc23 §11) ----------

#[derive(Debug, Clone, PartialEq)]
pub struct QueryExpr {
    pub table: String,
    pub clauses: Vec<QueryClause>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueryClause {
    Where(Box<Expr>),
    OrderBy(String),
    Join { table: String, on: Box<Expr> },
    Insert(Box<Expr>),
    Update(Vec<(String, Expr)>),
    Delete,
    First,
    Count,
}
