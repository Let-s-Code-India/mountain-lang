//! Trait/impl tests, per Document 25 §2.3's Phase 4 exit criteria: the
//! orphan-rule violation case (Document 7 §8) must be rejected, and
//! trait method resolution must be correct for both static and `dyn`
//! dispatch. Tests both directions throughout: valid usage that should
//! resolve, and invalid usage (orphan violations, missing methods,
//! out-of-scope method calls) that must be rejected with a real
//! diagnostic.

use mtnc::lexer;
use mtnc::parser::parse_program;
use mtnc::types::TypeChecker;

fn check(src: &str) -> Vec<String> {
    let (tokens, lex_errs) = lexer::tokenize(src);
    assert!(lex_errs.is_empty(), "lex errors: {:?}", lex_errs);
    let (program, parse_errs) = parse_program(tokens);
    assert!(parse_errs.is_empty(), "parse errors: {:?}", parse_errs);
    let mut tc = TypeChecker::new();
    tc.check_program(&program);
    tc.errors.iter().map(|e| e.to_string()).collect()
}

fn assert_ok(src: &str) {
    let errs = check(src);
    assert!(errs.is_empty(), "expected no type errors, got: {:#?}", errs);
}

fn assert_rejected(src: &str) {
    let errs = check(src);
    assert!(!errs.is_empty(), "expected type errors, got none");
}

// ============================================================
// Document 7 §5/§8 — orphan rule
// ============================================================

#[test]
fn orphan_rule_violation_neither_trait_nor_type_local() {
    // Document 7 §5: "An `impl TraitX for TypeY` is only allowed if
    // either `TraitX` or `TypeY` is defined in the current module/
    // package." Phase 4 doesn't have the real package system yet
    // (Document 15 is Phase 14) -- grounded via Document 15 §3.2's
    // `use`/`import` distinction: neither `ForeignTrait` nor
    // `ForeignType` is declared anywhere in this Program, so both are
    // treated as belonging to some other (hypothetical) package.
    let errs = check(r#"
impl ForeignTrait for ForeignType {
    fn doThing(borrow self) {
    }
}
"#);
    assert!(!errs.is_empty(), "expected an orphan-rule violation");
    assert!(
        errs.iter().any(|e| e.contains("orphan rule violation")),
        "expected an orphan-rule-specific message, got: {:#?}", errs
    );
}

#[test]
fn orphan_rule_allowed_when_trait_is_local() {
    // Trait is local (declared here), type is foreign -- allowed,
    // matching Document 7 §5's "either" (not "both").
    assert_ok(r#"
trait MyTrait {
    fn doThing(borrow self);
}
impl MyTrait for ForeignType {
    fn doThing(borrow self) {
    }
}
"#);
}

#[test]
fn orphan_rule_allowed_when_type_is_local() {
    // Type is local, trait is foreign -- also allowed.
    assert_ok(r#"
struct MyType {
    value: i32,
}
impl ForeignTrait for MyType {
    fn doThing(borrow self) {
    }
}
"#);
}

#[test]
fn orphan_rule_does_not_apply_to_inherent_impls() {
    // Document 7 §5 specifically scopes the orphan rule to `impl Trait
    // for Type`; a plain inherent `impl Type { ... }` (no trait) isn't
    // subject to it, even for a struct declared right here (trivially
    // local anyway, but confirms inherent impls take a different code
    // path entirely, not just "trait happens to be satisfied").
    assert_ok(r#"
struct MyType {
    value: i32,
}
impl MyType {
    fn getValue(borrow self) -> i32 {
        return self.value;
    }
}
"#);
}

// ============================================================
// Document 7 §4.2/§4.3 — required vs. default trait methods
// ============================================================

#[test]
fn missing_required_trait_method_rejected() {
    let errs = check(r#"
trait Greetable {
    fn name(borrow self) -> String;
    fn greet(borrow self) -> String {
        return "hi";
    }
}
struct User {
    n: String,
}
impl Greetable for User {
}
"#);
    assert!(!errs.is_empty(), "expected a missing-required-method error");
    assert!(
        errs.iter().any(|e| e.contains("missing required method") && e.contains("name")),
        "got: {:#?}", errs
    );
    // `greet` has a default body -- must NOT be reported as missing.
    assert!(
        !errs.iter().any(|e| e.contains("greet")),
        "default-bodied method should not be required, got: {:#?}", errs
    );
}

#[test]
fn all_required_methods_provided_accepted() {
    assert_ok(r#"
trait Greetable {
    fn name(borrow self) -> String;
    fn greet(borrow self) -> String {
        return "hi";
    }
}
struct User {
    n: String,
}
impl Greetable for User {
    fn name(borrow self) -> String {
        return self.n;
    }
}
"#);
}

// ============================================================
// Static dispatch method resolution
// ============================================================

#[test]
fn static_dispatch_method_call_resolves_and_checks_args() {
    assert_ok(r#"
struct Counter {
    value: i32,
}
impl Counter {
    fn add(borrow mut self, amount: i32) -> i32 {
        return self.value + amount;
    }
}
fn main() {
    let c = Counter { value: 0 };
    let total: i32 = c.add(5);
}
"#);
}

#[test]
fn static_dispatch_wrong_arg_type_rejected() {
    assert_rejected(r#"
struct Counter {
    value: i32,
}
impl Counter {
    fn add(borrow mut self, amount: i32) -> i32 {
        return self.value + amount;
    }
}
fn main() {
    let c = Counter { value: 0 };
    let x: f64 = 1.5;
    let total = c.add(x);
}
"#);
}

#[test]
fn static_dispatch_via_trait_impl_resolves() {
    assert_ok(r#"
trait Serializable {
    fn serialize(borrow self) -> String;
}
struct User {
    name: String,
}
impl Serializable for User {
    fn serialize(borrow self) -> String {
        return self.name;
    }
}
fn main() {
    let u = User { name: "Alice" };
    let s: String = u.serialize();
}
"#);
}

#[test]
fn calling_undefined_method_rejected() {
    let errs = check(r#"
struct User {
    name: String,
}
impl User {
    fn getName(borrow self) -> String {
        return self.name;
    }
}
fn main() {
    let u = User { name: "Alice" };
    let x = u.nonexistentMethod();
}
"#);
    assert!(!errs.is_empty(), "expected a no-such-method error");
    assert!(
        errs.iter().any(|e| e.contains("no method named") && e.contains("nonexistentMethod")),
        "got: {:#?}", errs
    );
}

#[test]
fn inherent_method_takes_precedence_over_trait_method_of_same_name() {
    // Exercises ImplRecord::trait_name's actual purpose: an inherent
    // impl's method shadows a trait-provided method of the same name.
    // Both are structurally valid to register; resolution must pick the
    // inherent one (checked indirectly here via a return-type
    // difference between the two candidates).
    assert_ok(r#"
trait Describable {
    fn describe(borrow self) -> String;
}
struct Widget {
    id: i32,
}
impl Describable for Widget {
    fn describe(borrow self) -> String {
        return "trait version";
    }
}
impl Widget {
    fn describe(borrow self) -> i32 {
        return self.id;
    }
}
fn main() {
    let w = Widget { id: 1 };
    let n: i32 = w.describe();
}
"#);
}

// ============================================================
// Document 7 §4.5 — dyn dispatch
// ============================================================

#[test]
fn dyn_dispatch_method_call_resolves_via_trait_signature() {
    assert_ok(r#"
trait Drawable {
    fn draw(borrow self);
}
fn useShape(shape: dyn Drawable) {
    shape.draw();
}
"#);
}

#[test]
fn dyn_dispatch_undefined_method_rejected() {
    let errs = check(r#"
trait Drawable {
    fn draw(borrow self);
}
fn useShape(shape: dyn Drawable) {
    shape.resize();
}
"#);
    assert!(!errs.is_empty(), "expected a no-such-method error on the trait");
    assert!(
        errs.iter().any(|e| e.contains("no method named") && e.contains("resize")),
        "got: {:#?}", errs
    );
}

#[test]
fn dyn_dispatch_wrong_arg_type_rejected() {
    let errs = check(r#"
trait Scalable {
    fn scale(borrow self, factor: f64);
}
fn useShape(shape: dyn Scalable) {
    shape.scale(true);
}
"#);
    assert!(!errs.is_empty(), "expected an arg-type mismatch error");
}

#[test]
fn dyn_and_static_dispatch_are_distinguished() {
    // A `dyn Trait` receiver and a concrete struct receiver with the
    // same trait implemented must resolve through different paths
    // (trait-signature lookup vs. concrete-impl lookup) but both
    // succeed for a correctly-implemented method.
    assert_ok(r#"
trait Drawable {
    fn draw(borrow self);
}
struct Circle {
    radius: f64,
}
impl Drawable for Circle {
    fn draw(borrow self) {
    }
}
fn useConcrete(c: Circle) {
    c.draw();
}
fn useDyn(shape: dyn Drawable) {
    shape.draw();
}
"#);
}
