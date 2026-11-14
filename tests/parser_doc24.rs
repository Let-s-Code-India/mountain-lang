//! Round-trips Document 24's example programs through the full
//! Lexer -> Parser pipeline, per Document 25 Phase 2's exit criteria.
//! Each example is reproduced verbatim from the spec document.
//!
//! Known gap (documented, not silently worked around): Document 24 §3
//! (AI/ML example) uses `tensor<f32>[784]` as a type — a generic type
//! immediately followed by a `[N]` shape suffix. Document 23's type
//! grammar has no production for this at all (its only `[...]` type
//! form is `[T; N]` array-of-T, a structurally different thing), and
//! unlike the UI style/layout gap, there's no other spec document with
//! a concrete grammar example to ground an extension against — Document
//! 16 §1.9 and Document 8 §8 both use tensor/matrix types but never
//! show this exact suffix-shape spelling being parsed from scratch. This
//! is flagged for explicit user guidance rather than guessed at; see
//! PROGRESS.md and the Phase 2 report. The test below parses Document
//! 24 §3 up to that construct and documents the expected failure point
//! rather than skipping the example entirely.

use mtnc::lexer;
use mtnc::parser::parse_program;

fn assert_parses_cleanly(name: &str, src: &str) {
    let (tokens, lex_errs) = lexer::tokenize(src);
    assert!(lex_errs.is_empty(), "[{}] lex errors: {:?}", name, lex_errs);
    let (_prog, parse_errs) = parse_program(tokens);
    assert!(
        parse_errs.is_empty(),
        "[{}] parse errors: {:?}",
        name,
        parse_errs
    );
}

#[test]
fn doc24_example1_backend_http_server_with_db() {
    let src = r#"
use models::User;
use std::net::server;
use std::db::query;

table Users {
    id: u64 primary_key auto_increment,
    name: String,
    email: String unique,
}

index UsersByEmail on Users(email) using hash;

async fn handleRequest(req: HttpRequest) -> HttpResponse {
    match (req.method, req.path.as_str()) {
        (HttpMethod::Get, "/users") => {
            let users: [User] = query Users orderBy name;
            return HttpResponse::json(users.serialize());
        },
        (HttpMethod::Post, "/users") => {
            let body = try { parseJson(req.body)? } catch (e) {
                return HttpResponse::badRequest("invalid JSON: " + e.message());
            };
            query Users insert User { name: body.name, email: body.email };
            return HttpResponse::created();
        },
        (HttpMethod::Get, path) if path.startsWith("/users/") => {
            let id = path.substring(7).parse::<u64>()?;
            let user = query Users where id == id first;
            match user {
                Some(u) => return HttpResponse::json(u.serialize()),
                None => return HttpResponse::notFound(),
            }
        },
        _ => return HttpResponse::notFound(),
    }
}

async fn main() -> Result<(), NetError> {
    let srv = server::Http::bind("0.0.0.0:8080")?;
    srv.onRequest(handleRequest);
    print("Server running on port 8080");
    await srv.listen();
    return Ok(());
}
"#;
    assert_parses_cleanly("doc24_example1", src);
}

#[test]
fn doc24_example2_frontend_todo_app() {
    let src = r#"
struct TodoItem {
    id: u64,
    title: String,
    done: bool,
}

ui TodoApp {
    state items: [TodoItem] = [];
    state newTitle: String = "";

    render {
        Column layout { gap: 8, align: Align::Center } {
            Text("My Tasks") style { fontSize: 24, fontWeight: FontWeight::Bold },
            Row {
                TextInput(bind: newTitle, placeholder: "New task..."),
                Button("Add", on: click => addTask()),
            },
            List {
                for item in items {
                    Row {
                        Checkbox(bind: item.done),
                        Text(item.title) style {
                            textDecoration: if item.done { TextDecoration::Strikethrough } else { TextDecoration::None },
                        },
                        Button("Delete", on: click => removeTask(item.id)),
                    }
                }
            },
        }
    }

    fn addTask(borrow mut self) {
        if newTitle.len() == 0 { return; }
        self.items.push(TodoItem { id: generateId(), title: newTitle, done: false });
        newTitle = "";
    }

    fn removeTask(borrow mut self, id: u64) {
        self.items.retain(|item| item.id != id);
    }
}
"#;
    assert_parses_cleanly("doc24_example2", src);
}

#[test]
fn doc24_example3_ai_training_loop_up_to_known_gap() {
    // The struct/impl/model-setup portion (everything NOT using the
    // `tensor<f32>[784]` shape-suffix type spelling) is expected to
    // parse cleanly -- this isolates the known, documented gap rather
    // than letting one unrelated construct hide whether the rest of the
    // domain's syntax (named args, closures over tuples, `gradient(...,
    // respectTo: ...)`, range-based `for epoch in 0..10`) works.
    let src_without_tensor_shape_type = r#"
use std::ai::{tensor, layers, optimizers, loss, dataset};

struct SimpleClassifier {
    layer1: layers::Dense,
    layer2: layers::Dense,
}

impl SimpleClassifier {
    fn newModel() -> SimpleClassifier {
        return SimpleClassifier {
            layer1: layers::Dense::new(784, 128, activation: Activation::ReLU),
            layer2: layers::Dense::new(128, 10, activation: Activation::Softmax),
        };
    }
}

fn trainModel() {
    let mut model = SimpleClassifier::newModel();
    let optimizer = optimizers::Adam::new(learningRate: 0.001);

    for epoch in 0..10 {
        let mut totalLoss = 0.0;
        for batch in trainData.batches(size: 64) {
            let predictions = batch.map(|(input, _)| model.forward(input));
            let targets = batch.map(|(_, label)| label);
            let batchLoss = loss::crossEntropy(predictions, targets);

            let gradients = gradient(batchLoss, respectTo: model);
            optimizer.step(borrow mut model, gradients);

            totalLoss += batchLoss.value();
        }
        print("Epoch " + epoch as String + ": loss = " + totalLoss as String);
    }
}
"#;
    assert_parses_cleanly("doc24_example3_minus_tensor_shape_type", src_without_tensor_shape_type);

    // Documents the actual gap: this fragment, taken verbatim from
    // Document 24 §3, is EXPECTED to fail to parse right now.
    let tensor_shape_fragment = r#"
fn forward(borrow self, input: tensor<f32>[784]) -> tensor<f32>[10] {
    return input;
}
"#;
    let (tokens, lex_errs) = lexer::tokenize(tensor_shape_fragment);
    assert!(lex_errs.is_empty());
    let (_prog, parse_errs) = parse_program(tokens);
    assert!(
        !parse_errs.is_empty(),
        "expected the tensor<f32>[784] shape-suffix construct to still be \
         an open gap -- if this now passes, the gap was fixed and this \
         assertion (and the PROGRESS.md note about it) should be updated"
    );
}

#[test]
fn doc24_example4_hft_actor_and_threads() {
    let src = r#"
use std::finance::orderbook;
use std::concurrency::channel;

struct Order {
    id: u64,
    side: OrderSide,
    price: f64,
    quantity: u64,
}

enum OrderSide { Buy, Sell }

actor MatchingEngine {
    state book: orderbook::OrderBook = orderbook::OrderBook::new();

    on submitOrder(order: Order) {
        let matches = self.book.match(order);
        for m in matches {
            broadcastFill(m);
        }
        if order.quantity > 0 {
            self.book.insert(order);
        }
    }

    on cancelOrder(id: u64) {
        self.book.remove(id);
    }
}

fn launchEngine() -> ActorHandle<MatchingEngine> {
    let engine = MatchingEngine::spawn();

    thread::spawnOS(move || {
        pinToCore(0);
        loop {
            let feedUpdate = await marketDataFeed.receive();
            engine.send(convertToOrder(feedUpdate));
        }
    });

    return engine;
}
"#;
    assert_parses_cleanly("doc24_example4", src);
}

#[test]
fn doc24_example5_producer_consumer_pipeline() {
    let src = r#"
use std::concurrency::channel;

fn runPipeline() {
    let (tx, rx) = channel::<i32>();

    spawn {
        for i in 0..1_000_000 {
            tx.send(i);
        }
    };

    let mut results: [i32] = [];
    spawn move {
        loop {
            match rx.recv() {
                Ok(value) => results.push(value * value),
                Err(_) => break,
            }
        }
        print("Processed " + results.len() as String + " items");
    };
}
"#;
    assert_parses_cleanly("doc24_example5", src);
}

#[test]
fn doc24_example6_cross_domain_one_struct_every_layer() {
    let src = r#"
struct Product {
    id: u64,
    name: String,
    price: f64,
    stock: u32,
}

table Products {
    id: u64 primary_key,
    name: String,
    price: f64,
    stock: u32,
}

async fn getProduct(id: u64) -> Result<Product, ApiError> {
    let product = query Products where id == id first;
    match product {
        Some(p) => return Ok(p),
        None => return Err(ApiError::NotFound),
    }
}

component ProductCard {
    prop product: Product;
    render {
        Column {
            Text(product.name),
            Text("$" + product.price as String),
            Text(if product.stock > 0 { "In Stock" } else { "Sold Out" }),
        }
    }
}

fn calculateInventoryValue(products: [Product]) -> f64 {
    return products.reduce(0.0, |acc, p| acc + (p.price * p.stock as f64));
}
"#;
    assert_parses_cleanly("doc24_example6", src);
}
