//! Behavioural tests for outline extraction, one realistic sample per language.

use super::*;

fn child<'a>(item: &'a OutlineItem, name: &str) -> &'a OutlineItem {
    item.children
        .iter()
        .find(|child| child.name == name)
        .unwrap_or_else(|| panic!("child {name} not found in {:?}", item.name))
}

fn item<'a>(outline: &'a Outline, name: &str) -> &'a OutlineItem {
    outline
        .items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("item {name} not found in {:?}", outline.items))
}

#[test]
fn rust_impl_methods_trait_and_module() {
    let source = r#"pub struct Point {
    x: i32,
}

impl Point {
    pub fn new(x: i32) -> Self {
        Self { x }
    }

    fn secret(&self) -> i32 {
        self.x
    }
}

pub trait Shape {
    fn area(&self) -> f64;
}

mod inner {
    pub fn helper() {}
}
"#;
    let outline = extract_outline("src/point.rs", source);
    assert_eq!(outline.language, Language::Rust);
    assert!(outline.error.is_none(), "{:?}", outline.error);

    let point = item(&outline, "Point");
    assert_eq!(point.kind, ItemKind::Struct);
    assert!(point.exported);

    let implementation = outline
        .items
        .iter()
        .find(|item| item.kind == ItemKind::Impl)
        .expect("impl item");
    assert_eq!(implementation.name, "Point");
    let new = child(implementation, "new");
    assert_eq!(new.kind, ItemKind::Method);
    assert!(new.exported);
    let secret = child(implementation, "secret");
    assert_eq!(secret.kind, ItemKind::Method);
    assert!(!secret.exported);

    // The body must never leak into the signature.
    assert_eq!(new.signature, "pub fn new(x: i32) -> Self {");
    assert!(!new.signature.contains("Self { x }"));
    assert_eq!(new.start_line, 6);
    assert_eq!(new.end_line, 8);

    let shape = item(&outline, "Shape");
    assert_eq!(shape.kind, ItemKind::Trait);
    assert_eq!(child(shape, "area").kind, ItemKind::Method);

    let inner = item(&outline, "inner");
    assert_eq!(inner.kind, ItemKind::Module);
    let helper = child(inner, "helper");
    assert_eq!(helper.kind, ItemKind::Function);
    assert!(helper.exported);
}

#[test]
fn typescript_class_interface_and_export() {
    let source = r#"export interface User {
  id: number;
  greet(name: string): string;
}

export class Service {
  private base = 1;

  run(input: string): number {
    return input.length;
  }
}

export function boot(): void {}

const helper = (x: number) => x + 1;
"#;
    let outline = extract_outline("web/app.ts", source);
    assert_eq!(outline.language, Language::TypeScript);
    assert!(outline.error.is_none(), "{:?}", outline.error);

    let user = item(&outline, "User");
    assert_eq!(user.kind, ItemKind::Interface);
    assert!(user.exported);
    assert_eq!(child(user, "greet").kind, ItemKind::Method);

    let service = item(&outline, "Service");
    assert_eq!(service.kind, ItemKind::Class);
    let run = child(service, "run");
    assert_eq!(run.kind, ItemKind::Method);
    assert_eq!(run.signature, "run(input: string): number {");

    let boot = item(&outline, "boot");
    assert_eq!(boot.kind, ItemKind::Function);
    assert!(boot.exported);

    let helper = item(&outline, "helper");
    assert_eq!(helper.kind, ItemKind::Function);
    assert!(!helper.exported);
    assert_eq!(helper.signature, "const helper = (x: number) => x + 1;");
}

#[test]
fn tsx_detected_and_parsed() {
    let source = r#"export function App(): JSX.Element {
  return <main>hello</main>;
}
"#;
    let outline = extract_outline("web/App.tsx", source);
    assert_eq!(outline.language, Language::Tsx);
    assert!(outline.error.is_none(), "{:?}", outline.error);
    assert_eq!(item(&outline, "App").kind, ItemKind::Function);
}

#[test]
fn python_nested_functions_and_private_names() {
    let source = r#"def outer(x):
    def inner(y):
        return y
    return inner(x)


class Greeter:
    def hello(self):
        return "hi"

    def _hidden(self):
        pass


def _private():
    pass
"#;
    let outline = extract_outline("pkg/greet.py", source);
    assert_eq!(outline.language, Language::Python);
    assert!(outline.error.is_none(), "{:?}", outline.error);

    let outer = item(&outline, "outer");
    assert_eq!(outer.kind, ItemKind::Function);
    assert!(outer.exported);
    let inner = child(outer, "inner");
    assert_eq!(inner.kind, ItemKind::Function);
    assert_eq!(inner.start_line, 2);
    assert_eq!(inner.end_line, 3);

    let greeter = item(&outline, "Greeter");
    assert_eq!(greeter.kind, ItemKind::Class);
    assert!(!child(greeter, "_hidden").exported);

    assert!(!item(&outline, "_private").exported);
}

#[test]
fn go_struct_methods_and_export_rules() {
    let source = r#"package demo

type Server struct {
    Addr string
}

type Handler interface {
    Serve()
}

func NewServer(addr string) *Server {
    return &Server{Addr: addr}
}

func (s *Server) Start() error {
    return nil
}

func (s *Server) stop() {}
"#;
    let outline = extract_outline("internal/server.go", source);
    assert_eq!(outline.language, Language::Go);
    assert!(outline.error.is_none(), "{:?}", outline.error);

    assert_eq!(item(&outline, "Server").kind, ItemKind::Struct);
    assert_eq!(item(&outline, "Handler").kind, ItemKind::Interface);
    assert!(item(&outline, "NewServer").exported);
    assert!(item(&outline, "Start").exported);
    assert_eq!(item(&outline, "Start").kind, ItemKind::Method);
    assert!(!item(&outline, "stop").exported);
}

#[test]
fn svelte_script_block_with_line_offsets() {
    let source = r#"<script lang="ts">
  import { onMount } from 'svelte';

  export function createCounter(start: number): number {
    return start;
  }

  class Store {
    get value(): number {
      return 1;
    }
  }
</script>

<h1>Hello</h1>
"#;
    let outline = extract_outline("web/Counter.svelte", source);
    assert_eq!(outline.language, Language::Svelte);
    assert!(outline.error.is_none(), "{:?}", outline.error);

    let counter = item(&outline, "createCounter");
    assert_eq!(counter.kind, ItemKind::Function);
    assert!(counter.exported);
    assert_eq!(counter.start_line, 4);
    assert_eq!(counter.end_line, 6);

    let store = item(&outline, "Store");
    assert_eq!(store.kind, ItemKind::Class);
    assert_eq!(store.start_line, 8);
    assert_eq!(child(store, "value").kind, ItemKind::Method);
}

#[test]
fn svelte_without_script_is_empty_not_an_error() {
    let outline = extract_outline("web/Static.svelte", "<h1>Hello</h1>\n");
    assert_eq!(outline.language, Language::Svelte);
    assert!(outline.items.is_empty());
    assert!(outline.error.is_none());
}

#[test]
fn syntax_errors_are_fail_soft() {
    let outline = extract_outline("src/broken.rs", "fn broken( {\n");
    assert_eq!(outline.language, Language::Rust);
    assert!(outline.error.is_some());
    // No panic, and whatever parsed may be returned as a partial outline.
    assert!(outline.item_count() <= MAX_ITEMS);
}

#[test]
fn binary_content_is_rejected() {
    let outline = extract_outline("src/blob.py", "def ok():\n    pass\n\0\n");
    assert_eq!(outline.language, Language::Python);
    assert_eq!(outline.error.as_deref(), Some("binary content"));
    assert!(outline.items.is_empty());
}

#[test]
fn oversized_files_are_skipped() {
    let source = "x".repeat(MAX_FILE_BYTES + 1);
    let outline = extract_outline("src/huge.rs", &source);
    assert_eq!(outline.error.as_deref(), Some("file too large"));
    assert!(outline.items.is_empty());
    assert!(!outline.truncated);
}

#[test]
fn item_cap_sets_truncated() {
    let mut source = String::new();
    for index in 0..MAX_ITEMS + 20 {
        source.push_str(&format!("fn f{index}() {{}}\n"));
    }
    let outline = extract_outline("src/many.rs", &source);
    assert!(outline.error.is_none());
    assert!(outline.truncated);
    assert_eq!(outline.item_count(), MAX_ITEMS);
    assert_eq!(outline.items.len(), MAX_ITEMS);
}

#[test]
fn unsupported_extension_reports_error() {
    let outline = extract_outline("notes.txt", "hello");
    assert_eq!(outline.language, Language::Other);
    assert_eq!(outline.error.as_deref(), Some("unsupported language"));
    assert!(outline.items.is_empty());
}

#[test]
fn signature_is_single_line_and_capped() {
    let long_name = "f".repeat(300);
    let source = format!("pub fn {long_name}() {{\n    let body = 1;\n}}\n");
    let outline = extract_outline("src/long.rs", &source);
    let item = &outline.items[0];
    assert!(!item.signature.contains('\n'));
    assert!(item.signature.chars().count() <= MAX_SIGNATURE_CHARS);
    assert!(!item.signature.contains("let body"));
}

#[test]
fn language_mapping_covers_js_and_unknown() {
    assert_eq!(Language::from_path("a.js"), Language::TypeScript);
    assert_eq!(Language::from_path("a.tsx"), Language::Tsx);
    assert_eq!(Language::from_path("a.pyi"), Language::Python);
    assert_eq!(Language::from_path("Makefile"), Language::Other);
    assert_eq!(Language::Other.as_str(), "other");
    assert!(!Language::Other.is_supported());
}
