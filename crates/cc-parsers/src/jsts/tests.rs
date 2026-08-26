//! Tests for the JS/TS parser (moved verbatim from `jsts/mod.rs`).

use super::extras::classify_literal;
use super::*;
use cc_model::dispatch_site::DispatchSiteKind;
use cc_model::edge::DispatchKind;

#[test]
fn parse_simple_js() {
    let p = JsTsParser::new();
    let code = r#"
function greet(name) {
    return "Hello " + name;
}

class Greeter {
    constructor(prefix) {
        this.prefix = prefix;
    }
    greet(name) {
        return this.prefix + name;
    }
}
"#;
    let outcome = p.parse("app.js", code, Language::JavaScript).unwrap();
    let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"greet"));
    assert!(names.contains(&"Greeter"));
    assert!(!outcome.chunks.is_empty());
}

#[test]
fn parse_typescript_arrow() {
    let p = JsTsParser::new();
    let code = "const add = (a: number, b: number): number => a + b;\n";
    let outcome = p.parse("math.ts", code, Language::TypeScript).unwrap();
    assert!(!outcome.symbols.is_empty());
    let add = outcome.symbols.iter().find(|s| s.name == "add");
    assert!(add.is_some());
}

#[test]
fn extract_express_route() {
    let p = JsTsParser::new();
    let code = r#"
const express = require("express");
const app = express();

app.get("/api/users", getUsers);
app.post("/api/users", createUser);
app.use("/api", authMiddleware);
"#;
    let outcome = p.parse("server.js", code, Language::JavaScript).unwrap();
    assert!(
        outcome.route_edges.len() >= 3,
        "expected >= 3 routes, got {}",
        outcome.route_edges.len()
    );

    let get_route = outcome
        .route_edges
        .iter()
        .find(|r| r.route_path == "/api/users" && r.method.as_deref() == Some("GET"));
    assert!(get_route.is_some(), "missing GET /api/users route");
    assert_eq!(get_route.unwrap().handler_name.as_deref(), Some("getUsers"));

    let post_route = outcome
        .route_edges
        .iter()
        .find(|r| r.route_path == "/api/users" && r.method.as_deref() == Some("POST"));
    assert!(post_route.is_some(), "missing POST /api/users route");

    let middleware = outcome
        .route_edges
        .iter()
        .find(|r| r.route_kind.as_deref() == Some("middleware"));
    assert!(middleware.is_some(), "missing middleware");
    assert_eq!(middleware.unwrap().method.as_deref(), Some("USE"));
}

#[test]
fn extract_framework_roles() {
    let p = JsTsParser::new();
    let code = r#"
function useAuth() { return {}; }
function UserProfile() { return null; }
function processData() { return null; }
class UserController {}
class AuthService {}
"#;
    let outcome = p.parse("app.tsx", code, Language::Tsx).unwrap();

    let hook = outcome.symbols.iter().find(|s| s.name == "useAuth");
    assert!(hook.is_some());
    assert_eq!(
        hook.unwrap().framework_role.as_deref(),
        Some("hook"),
        "useAuth should be classified as hook"
    );

    let component = outcome.symbols.iter().find(|s| s.name == "UserProfile");
    assert!(component.is_some());
    assert_eq!(
        component.unwrap().framework_role.as_deref(),
        Some("component"),
        "UserProfile should be classified as component"
    );

    let controller = outcome.symbols.iter().find(|s| s.name == "UserController");
    assert!(controller.is_some());
    assert_eq!(
        controller.unwrap().framework_role.as_deref(),
        Some("controller"),
        "UserController should be classified as controller"
    );

    let service = outcome.symbols.iter().find(|s| s.name == "AuthService");
    assert!(service.is_some());
    assert_eq!(
        service.unwrap().framework_role.as_deref(),
        Some("service"),
        "AuthService should be classified as service"
    );

    let plain = outcome.symbols.iter().find(|s| s.name == "processData");
    assert!(plain.is_some());
    assert_eq!(
        plain.unwrap().framework_role,
        None,
        "processData should have no framework_role"
    );
}

#[test]
fn detect_nextjs_route() {
    let p = JsTsParser::new();
    let code = r#"
export async function GET(request) {
    return Response.json({ ok: true });
}
"#;
    let outcome = p
        .parse("app/api/users/route.ts", code, Language::TypeScript)
        .unwrap();
    let route = outcome
        .route_edges
        .iter()
        .find(|r| r.framework.as_deref() == Some("nextjs"));
    assert!(route.is_some(), "missing Next.js route");
    assert_eq!(route.unwrap().route_path, "/api/users");
}

#[test]
fn extract_literals() {
    let p = JsTsParser::new();
    let code = r#"
const url = "https://api.example.com/v1/users";
const key = "DATABASE_URL";
const query = "SELECT * FROM users WHERE id = 1";
"#;
    let outcome = p.parse("config.ts", code, Language::TypeScript).unwrap();
    assert!(
        !outcome.literal_index.is_empty(),
        "expected some literals to be extracted"
    );
    let url_lit = outcome
        .literal_index
        .iter()
        .find(|l| l.literal_kind == "url");
    assert!(url_lit.is_some(), "missing url literal");
    let env_lit = outcome
        .literal_index
        .iter()
        .find(|l| l.literal_kind == "env_key");
    assert!(env_lit.is_some(), "missing env_key literal");
}

#[test]
fn extract_call_edges_with_dispatch_kind() {
    let p = JsTsParser::new();
    let code = r#"
function main() {
    doSomething();
    obj.method();
    const x = new MyClass();
}
"#;
    let outcome = p.parse("main.ts", code, Language::TypeScript).unwrap();

    let direct = outcome
        .call_edges
        .iter()
        .find(|c| c.callee_symbol == "doSomething" && c.dispatch_kind == DispatchKind::Direct);
    assert!(direct.is_some(), "missing direct call edge");

    let member = outcome
        .call_edges
        .iter()
        .find(|c| c.callee_symbol == "obj.method" && c.call_kind == "member");
    assert!(member.is_some(), "missing member call edge");

    let constructor = outcome
        .call_edges
        .iter()
        .find(|c| c.callee_symbol == "MyClass" && c.is_constructor);
    assert!(constructor.is_some(), "missing constructor call edge");
}

#[test]
fn extract_http_call_fetch() {
    // 代码片段包含 fetch 调用
    let code = r#"
async function loadUsers() {
    const response = await fetch("/api/users");
    return response.json();
}
"#;
    let p = JsTsParser::new();
    let outcome = p.parse("app.js", code, Language::JavaScript).unwrap();

    assert!(
        !outcome.http_call_edges.is_empty(),
        "should extract HTTP call from fetch"
    );
    let hce = &outcome.http_call_edges[0];
    assert_eq!(hce.url_or_path, "/api/users");
    assert_eq!(hce.method, Some("GET".to_string())); // fetch defaults to GET
    assert_eq!(hce.call_kind, "http");
    assert!(hce.normalized_path.is_some());
    assert_eq!(hce.normalized_path.as_deref(), Some("/api/users"));
}

#[test]
fn extract_http_call_axios() {
    let code = r#"
import axios from 'axios';
async function createOrder(data) {
    const res = await axios.post("/api/orders", data);
    return res.data;
}
"#;
    let p = JsTsParser::new();
    let outcome = p.parse("orders.ts", code, Language::TypeScript).unwrap();

    assert!(
        !outcome.http_call_edges.is_empty(),
        "should extract HTTP call from axios.post"
    );
    let hce = &outcome.http_call_edges[0];
    assert_eq!(hce.url_or_path, "/api/orders");
    assert_eq!(hce.method, Some("POST".to_string()));
}

#[test]
fn extract_http_call_template_string() {
    let code = r#"
async function getUser(id) {
    return fetch(`/api/users/${id}`);
}
"#;
    let p = JsTsParser::new();
    let outcome = p.parse("users.js", code, Language::JavaScript).unwrap();

    assert!(
        !outcome.http_call_edges.is_empty(),
        "should extract HTTP call from fetch with template string"
    );
    let hce = &outcome.http_call_edges[0];
    // Template variables should be normalized to *
    assert!(
        hce.normalized_path.as_deref().unwrap().contains('*'),
        "normalized_path should contain * for template variable, got: {:?}",
        hce.normalized_path
    );
}

#[test]
fn no_false_positive_console_log() {
    // console.log should NOT trigger HTTP call detection
    let code = r#"console.log("/api/test");"#;
    let p = JsTsParser::new();
    let outcome = p.parse("test.js", code, Language::JavaScript).unwrap();
    assert!(
        outcome.http_call_edges.is_empty(),
        "console.log should not be detected as HTTP call"
    );
}

#[test]
fn pending_exports_applied() {
    let p = JsTsParser::new();
    let code = r#"
function foo() { return 1; }
function bar() { return 2; }
export { foo, bar as baz };
export default foo;
"#;
    let outcome = p.parse("mod.ts", code, Language::TypeScript).unwrap();

    let foo = outcome.symbols.iter().find(|s| s.name == "foo");
    assert!(foo.is_some());
    // foo should be both default export and named export
    assert!(
        foo.unwrap().is_default_export,
        "foo should be default export"
    );
}

#[test]
fn two_step_forwarding_marks_import_as_reexport() {
    let p = JsTsParser::new();
    let code = "import { x } from './b';\nexport { x };\n";
    let outcome = p.parse("a.ts", code, Language::TypeScript).unwrap();
    let imp = outcome
        .imports
        .iter()
        .find(|i| i.import_string == "./b")
        .expect("import record for './b' must exist");
    assert!(
        imp.is_reexport,
        "two-step forwarding (import then local export of the same binding) \
             must mark the originating import as a re-export"
    );
}

#[test]
fn two_step_forwarding_with_import_alias_marks_reexport() {
    let p = JsTsParser::new();
    let code = "import { x as localX } from './b';\nexport { localX as y };\n";
    let outcome = p.parse("a.ts", code, Language::TypeScript).unwrap();
    let imp = outcome
        .imports
        .iter()
        .find(|i| i.import_string == "./b")
        .expect("import record for './b' must exist");
    assert!(
        imp.is_reexport,
        "export of an aliased import binding (localX) must mark the import as a re-export"
    );
}

#[test]
fn two_step_forwarding_export_default_of_imported_binding_marks_reexport() {
    let p = JsTsParser::new();
    let code = "import { x } from './b';\nexport default x;\n";
    let outcome = p.parse("a.ts", code, Language::TypeScript).unwrap();
    let imp = outcome
        .imports
        .iter()
        .find(|i| i.import_string == "./b")
        .expect("import record for './b' must exist");
    assert!(
        imp.is_reexport,
        "`export default x` where x is an imported binding must mark the import as a re-export"
    );
}

#[test]
fn local_export_does_not_mark_unrelated_import_as_reexport() {
    let p = JsTsParser::new();
    let code = "import { x } from './b';\nconst y = 1;\nexport { y };\n";
    let outcome = p.parse("a.ts", code, Language::TypeScript).unwrap();
    let imp = outcome
        .imports
        .iter()
        .find(|i| i.import_string == "./b")
        .expect("import record for './b' must exist");
    assert!(
        !imp.is_reexport,
        "exporting a local binding (y) must not mark an unrelated import as a re-export"
    );
    let y_sym = outcome
        .symbols
        .iter()
        .find(|s| s.name == "y")
        .expect("local symbol y must exist");
    assert_eq!(
        y_sym.export_name.as_deref(),
        Some("y"),
        "local export must still bind to the local symbol"
    );
}

#[test]
fn extract_fetch_post_method() {
    let code = r#"
async function createOrder(data) {
    const res = await fetch("/api/orders", { method: "POST", body: JSON.stringify(data) });
    return res.json();
}
"#;
    let parser = JsTsParser::new();
    let outcome = parser
        .parse("orders.js", code, Language::JavaScript)
        .unwrap();
    assert!(!outcome.http_call_edges.is_empty());
    let hce = &outcome.http_call_edges[0];
    assert_eq!(hce.method, Some("POST".to_string()));
}

#[test]
fn extract_fetch_default_get() {
    let code = r#"const data = await fetch("/api/users");"#;
    let parser = JsTsParser::new();
    let outcome = parser.parse("app.js", code, Language::JavaScript).unwrap();
    assert!(!outcome.http_call_edges.is_empty());
    assert_eq!(outcome.http_call_edges[0].method, Some("GET".to_string()));
}

#[test]
fn classify_literal_env_key() {
    assert_eq!(classify_literal("DATABASE_URL"), Some("env_key"));
    assert_eq!(classify_literal("API_KEY"), Some("env_key"));
    assert_eq!(classify_literal("NODE_ENV"), Some("env_key"));
}

#[test]
fn classify_literal_config_key() {
    assert_eq!(classify_literal("app.database.host"), Some("config_key"));
    assert_eq!(
        classify_literal("redis.connection.timeout"),
        Some("config_key")
    );
}

#[test]
fn classify_literal_topic() {
    assert_eq!(classify_literal("orders-topic"), Some("topic"));
    assert_eq!(classify_literal("user-events"), Some("topic"));
    assert_eq!(classify_literal("payment.events"), Some("topic"));
}

#[test]
fn classify_literal_queue() {
    assert_eq!(classify_literal("orders-queue"), Some("queue"));
    assert_eq!(classify_literal("tasks-fifo"), Some("queue"));
    assert_eq!(classify_literal("queue.orders"), Some("queue"));
}

#[test]
fn classify_literal_priority() {
    // URL takes priority over everything
    assert_eq!(classify_literal("https://api.example.com"), Some("url"));
    // Route takes priority
    assert_eq!(classify_literal("/api/users"), Some("route"));
}

#[test]
fn literal_enclosing_symbol_uid() {
    let p = JsTsParser::new();
    let code = r#"
function loadConfig() {
    const url = "https://api.example.com/v1";
    const key = "DATABASE_URL";
}
"#;
    let outcome = p.parse("config.ts", code, Language::TypeScript).unwrap();
    // All literals inside loadConfig should have enclosing_symbol_uid set
    for lit in &outcome.literal_index {
        assert!(
            lit.enclosing_symbol_uid.is_some(),
            "literal '{}' should have enclosing_symbol_uid",
            lit.literal
        );
    }
}

#[test]
fn literal_config_key_has_key_path() {
    let p = JsTsParser::new();
    let code = r#"
function getConfig() {
    return "app.database.host";
}
"#;
    let outcome = p.parse("config.ts", code, Language::TypeScript).unwrap();
    let config_lit = outcome
        .literal_index
        .iter()
        .find(|l| l.literal_kind == "config_key");
    assert!(config_lit.is_some(), "missing config_key literal");
    assert_eq!(
        config_lit.unwrap().key_path.as_deref(),
        Some("app.database.host")
    );
}

// ── Async broker extraction tests ─────────────────────────────

#[test]
fn extract_kafka_broker_call() {
    // Object name "kafka" matches OBJECT_PATTERNS → broker_type = "kafka".
    // Method "send" is in ASYNC_METHODS → call_kind = "async".
    let code = r#"
const kafka = require('kafkajs');
async function publishOrder(order) {
    await kafka.send({ topic: 'orders', messages: [{ value: JSON.stringify(order) }] });
}
"#;
    let p = JsTsParser::new();
    let outcome = p.parse("producer.js", code, Language::JavaScript).unwrap();

    let broker_edges: Vec<_> = outcome
        .http_call_edges
        .iter()
        .filter(|e| e.call_kind == "async")
        .collect();
    assert!(
        !broker_edges.is_empty(),
        "should detect kafka.send() as async broker call"
    );
    assert_eq!(broker_edges[0].call_kind, "async");
    assert_eq!(broker_edges[0].broker_type.as_deref(), Some("kafka"));
}

#[test]
fn extract_bullmq_broker_call() {
    // Object name "bullQueue" contains "bull" → matches OBJECT_PATTERNS → broker_type = "bullmq".
    // Method "dispatch" is in ASYNC_METHODS → call_kind = "async".
    // BullMQ's `queue.add()` is detected because "add" is in ASYNC_METHODS.
    let code = r#"
import { Queue } from 'bullmq';
const bullQueue = new Queue('notifications');
await bullQueue.dispatch('send-email', { to: 'user@example.com' });
"#;
    let p = JsTsParser::new();
    let outcome = p.parse("queue.ts", code, Language::TypeScript).unwrap();

    let broker_edges: Vec<_> = outcome
        .http_call_edges
        .iter()
        .filter(|e| e.call_kind == "async")
        .collect();
    assert!(
        !broker_edges.is_empty(),
        "should detect bullQueue.dispatch() as async broker call"
    );
    assert_eq!(broker_edges[0].broker_type.as_deref(), Some("bullmq"));
}

#[test]
fn http_call_not_misclassified_as_broker() {
    // Plain HTTP calls via axios should produce call_kind = "http", broker_type = None.
    let code = r#"
import axios from 'axios';
async function getUsers() {
    const res = await axios.get('/api/users');
    return res.data;
}
"#;
    let p = JsTsParser::new();
    let outcome = p.parse("client.ts", code, Language::TypeScript).unwrap();

    assert!(
        !outcome.http_call_edges.is_empty(),
        "should extract HTTP call from axios.get"
    );
    let hce = &outcome.http_call_edges[0];
    assert_eq!(hce.call_kind, "http");
    assert_eq!(
        hce.broker_type, None,
        "plain HTTP call should have no broker_type"
    );
}

#[test]
fn test_event_emitter_dispatch_sites() {
    let source = r#"
const emitter = new EventEmitter();
emitter.on('user:created', handleUser);
emitter.emit('user:created', data);
"#;
    let p = JsTsParser::new();
    let outcome = p.parse("test.js", source, Language::JavaScript).unwrap();
    assert_eq!(
        outcome.dispatch_sites.len(),
        2,
        "should extract 2 dispatch sites (on + emit), got: {:?}",
        outcome.dispatch_sites
    );
    let on_site = outcome
        .dispatch_sites
        .iter()
        .find(|s| s.site_kind == DispatchSiteKind::EventOn)
        .expect("should have an EventOn dispatch site");
    assert_eq!(on_site.key, "user:created");
    assert_eq!(on_site.handler_expr.as_deref(), Some("handleUser"));
    assert_eq!(on_site.receiver_expr.as_deref(), Some("emitter"));

    let emit_site = outcome
        .dispatch_sites
        .iter()
        .find(|s| s.site_kind == DispatchSiteKind::EventEmit)
        .expect("should have an EventEmit dispatch site");
    assert_eq!(emit_site.key, "user:created");
    assert!(emit_site.handler_expr.is_none());
    assert_eq!(emit_site.receiver_expr.as_deref(), Some("emitter"));
}

#[test]
fn test_event_listener_variants() {
    let source = r#"
window.addEventListener('click', handler);
bus.once('ready', onReady);
bus.subscribe('data', processData);
"#;
    let p = JsTsParser::new();
    let outcome = p.parse("test.js", source, Language::JavaScript).unwrap();
    assert_eq!(
        outcome.dispatch_sites.len(),
        3,
        "should detect addEventListener, once, subscribe"
    );
    assert!(outcome
        .dispatch_sites
        .iter()
        .all(|s| s.site_kind == DispatchSiteKind::EventOn));
    let keys: Vec<&str> = outcome
        .dispatch_sites
        .iter()
        .map(|s| s.key.as_str())
        .collect();
    assert!(keys.contains(&"click"));
    assert!(keys.contains(&"ready"));
    assert!(keys.contains(&"data"));
}

#[test]
fn test_event_dispatch_variants() {
    let source = r#"
emitter.emit('start', payload);
bus.trigger('update');
el.dispatchEvent('custom');
"#;
    let p = JsTsParser::new();
    let outcome = p.parse("test.js", source, Language::JavaScript).unwrap();
    assert_eq!(
        outcome.dispatch_sites.len(),
        3,
        "should detect emit, trigger, dispatchEvent"
    );
    assert!(outcome
        .dispatch_sites
        .iter()
        .all(|s| s.site_kind == DispatchSiteKind::EventEmit));
}

#[test]
fn test_jsx_tag_dispatch_sites() {
    let source = r#"
function App() {
    return (
        <div>
            <UserProfile name="test" />
            <Header />
            <span>text</span>
        </div>
    );
}
"#;
    let p = JsTsParser::new();
    let outcome = p.parse("app.tsx", source, Language::Tsx).unwrap();
    let jsx_sites: Vec<_> = outcome
        .dispatch_sites
        .iter()
        .filter(|s| s.site_kind == DispatchSiteKind::JsxTag)
        .collect();
    assert_eq!(
        jsx_sites.len(),
        2,
        "should extract 2 JSX tag sites (UserProfile + Header, NOT div/span), got: {:?}",
        jsx_sites
    );
    assert!(jsx_sites.iter().any(|s| s.key == "UserProfile"));
    assert!(jsx_sites.iter().any(|s| s.key == "Header"));
}

#[test]
fn test_jsx_member_expression_tag() {
    let source = r#"
function App() {
    return <Router.Switch><Route path="/" /></Router.Switch>;
}
"#;
    let p = JsTsParser::new();
    let outcome = p.parse("app.tsx", source, Language::Tsx).unwrap();
    let jsx_sites: Vec<_> = outcome
        .dispatch_sites
        .iter()
        .filter(|s| s.site_kind == DispatchSiteKind::JsxTag)
        .collect();
    // Router.Switch and Route are PascalCase
    assert!(
        jsx_sites.iter().any(|s| s.key.contains("Router")),
        "should detect Router.Switch, got: {:?}",
        jsx_sites
    );
    assert!(
        jsx_sites.iter().any(|s| s.key == "Route"),
        "should detect Route, got: {:?}",
        jsx_sites
    );
}

#[test]
fn test_use_state_setter_dispatch_sites() {
    let source = r#"
function Counter() {
    const [count, setCount] = useState(0);
    const [name, setName] = useState("");
    const handleClick = () => setCount(count + 1);
    return <button onClick={handleClick}>{count}</button>;
}
"#;
    let p = JsTsParser::new();
    let outcome = p.parse("counter.tsx", source, Language::Tsx).unwrap();

    // Check bindings (StateSetterBinding from useState destructuring)
    let binding_sites: Vec<_> = outcome
        .dispatch_sites
        .iter()
        .filter(|s| s.site_kind == DispatchSiteKind::StateSetterBinding)
        .collect();
    assert!(
        binding_sites.len() >= 2,
        "should detect at least 2 useState bindings (setCount + setName), got: {:?}",
        binding_sites
    );
    assert!(
        binding_sites.iter().any(|s| s.key == "setCount"),
        "should detect setCount binding, got: {:?}",
        binding_sites
    );
    assert!(
        binding_sites.iter().any(|s| s.key == "setName"),
        "should detect setName binding, got: {:?}",
        binding_sites
    );

    // Check calls (StateSetterCall from setCount(...) invocation)
    let call_sites: Vec<_> = outcome
        .dispatch_sites
        .iter()
        .filter(|s| s.site_kind == DispatchSiteKind::StateSetterCall)
        .collect();
    assert!(
        call_sites.iter().any(|s| s.key == "setCount"),
        "should detect setCount call site, got: {:?}",
        call_sites
    );
}

#[test]
fn test_class_set_state_dispatch_sites() {
    let source = r#"
class Counter extends React.Component {
    handleClick() {
        this.setState({ count: this.state.count + 1 });
    }
    render() {
        return <button onClick={this.handleClick}>{this.state.count}</button>;
    }
}
"#;
    let p = JsTsParser::new();
    let outcome = p.parse("counter.tsx", source, Language::Tsx).unwrap();
    let setter_sites: Vec<_> = outcome
        .dispatch_sites
        .iter()
        .filter(|s| s.site_kind == DispatchSiteKind::StateSetterCall)
        .collect();
    assert!(
        setter_sites.iter().any(|s| s.key == "setState"),
        "should detect this.setState as StateSetterCall, got: {:?}",
        setter_sites
    );
    assert_eq!(
        setter_sites
            .iter()
            .find(|s| s.key == "setState")
            .unwrap()
            .receiver_expr
            .as_deref(),
        Some("this")
    );
}
