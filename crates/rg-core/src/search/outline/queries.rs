//! tree-sitter queries and per-language extraction.
//!
//! Every language is handled by a real query over the tree-sitter AST. The
//! flat set of captures is nested by byte range in [`super::assemble`], which
//! is what produces `impl` → methods, `class` → methods, nested Python
//! functions, and so on.

use std::sync::OnceLock;

use tree_sitter::{
    Language as TsLanguage, Node, Parser, Query, QueryCursor, StreamingIterator, Tree,
};

use super::{assemble, ItemKind, Language, Outline, RawItem, MAX_SIGNATURE_CHARS};

const RUST_QUERY: &str = r#"
(function_item name: (identifier) @name) @function
(function_signature_item name: (identifier) @name) @function
(struct_item name: (type_identifier) @name) @struct
(enum_item name: (type_identifier) @name) @enum
(trait_item name: (type_identifier) @name) @trait
(impl_item) @impl
(mod_item name: (identifier) @name) @module
(const_item name: (identifier) @name) @const
(static_item name: (identifier) @name) @const
(type_item name: (type_identifier) @name) @type
(macro_definition name: (identifier) @name) @macro
"#;

const TS_QUERY: &str = r#"
(function_declaration name: (identifier) @name) @function
(generator_function_declaration name: (identifier) @name) @function
(lexical_declaration (variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)])) @function
(variable_declaration (variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)])) @function
(class_declaration name: (type_identifier) @name) @class
(interface_declaration name: (type_identifier) @name) @interface
(enum_declaration name: (identifier) @name) @enum
(type_alias_declaration name: (type_identifier) @name) @type
(internal_module name: (identifier) @name) @module
(method_definition name: (property_identifier) @name) @method
(abstract_method_signature name: (property_identifier) @name) @method
(method_signature name: (property_identifier) @name) @method
"#;

const PYTHON_QUERY: &str = r#"
(function_definition name: (identifier) @name) @function
(class_definition name: (identifier) @name) @class
"#;

const GO_QUERY: &str = r#"
(function_declaration name: (identifier) @name) @function
(method_declaration name: (field_identifier) @name) @method
(type_declaration (type_spec name: (type_identifier) @name type: (struct_type))) @struct
(type_declaration (type_spec name: (type_identifier) @name type: (interface_type))) @interface
"#;

fn rust_query() -> Result<&'static Query, String> {
    static QUERY: OnceLock<Result<Query, String>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language: TsLanguage = tree_sitter_rust::LANGUAGE.into();
            Query::new(&language, RUST_QUERY).map_err(|e| format!("rust query: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn ts_query(tsx: bool) -> Result<&'static Query, String> {
    static TS: OnceLock<Result<Query, String>> = OnceLock::new();
    static TSX: OnceLock<Result<Query, String>> = OnceLock::new();
    let cell = if tsx { &TSX } else { &TS };
    cell.get_or_init(|| {
        let language: TsLanguage = if tsx {
            tree_sitter_typescript::LANGUAGE_TSX.into()
        } else {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
        };
        Query::new(&language, TS_QUERY).map_err(|e| format!("typescript query: {e}"))
    })
    .as_ref()
    .map_err(|e| e.clone())
}

fn python_query() -> Result<&'static Query, String> {
    static QUERY: OnceLock<Result<Query, String>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language: TsLanguage = tree_sitter_python::LANGUAGE.into();
            Query::new(&language, PYTHON_QUERY).map_err(|e| format!("python query: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn go_query() -> Result<&'static Query, String> {
    static QUERY: OnceLock<Result<Query, String>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language: TsLanguage = tree_sitter_go::LANGUAGE.into();
            Query::new(&language, GO_QUERY).map_err(|e| format!("go query: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(super) fn extract_rust(source: &str) -> Outline {
    let language: TsLanguage = tree_sitter_rust::LANGUAGE.into();
    let query = match rust_query() {
        Ok(query) => query,
        Err(error) => return Outline::failed(Language::Rust, error),
    };
    extract_with(
        source,
        &language,
        query,
        Language::Rust,
        rust_kind,
        |_name, sig, _export_prefix| sig.starts_with("pub"),
    )
}

pub(super) fn extract_typescript(source: &str, tsx: bool) -> Outline {
    let language: TsLanguage = if tsx {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    };
    let query = match ts_query(tsx) {
        Ok(query) => query,
        Err(error) => {
            return Outline::failed(
                if tsx {
                    Language::Tsx
                } else {
                    Language::TypeScript
                },
                error,
            )
        }
    };
    extract_with(
        source,
        &language,
        query,
        if tsx {
            Language::Tsx
        } else {
            Language::TypeScript
        },
        ts_kind,
        ts_exported,
    )
}

pub(super) fn extract_python(source: &str) -> Outline {
    let language: TsLanguage = tree_sitter_python::LANGUAGE.into();
    let query = match python_query() {
        Ok(query) => query,
        Err(error) => return Outline::failed(Language::Python, error),
    };
    extract_with(
        source,
        &language,
        query,
        Language::Python,
        python_kind,
        |name, _sig, _prefix| !name.starts_with('_'),
    )
}

pub(super) fn extract_go(source: &str) -> Outline {
    let language: TsLanguage = tree_sitter_go::LANGUAGE.into();
    let query = match go_query() {
        Ok(query) => query,
        Err(error) => return Outline::failed(Language::Go, error),
    };
    extract_with(
        source,
        &language,
        query,
        Language::Go,
        go_kind,
        |name, _sig, _prefix| name.chars().next().is_some_and(char::is_uppercase),
    )
}

/// Best-effort Svelte: parse `<script>` block contents as TypeScript and shift
/// line numbers back into the `.svelte` file. Markup is not inspected.
pub(super) fn extract_svelte(source: &str) -> Outline {
    let language: TsLanguage = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    let query = match ts_query(false) {
        Ok(query) => query,
        Err(error) => return Outline::failed(Language::Svelte, error),
    };

    let mut raws: Vec<RawItem> = Vec::new();
    let mut error: Option<String> = None;

    for (line_offset, content) in script_blocks(source) {
        match parse(content, &language) {
            Ok(tree) => {
                if tree.root_node().has_error() && error.is_none() {
                    error = Some("svelte: syntax errors in <script> block".to_string());
                }
                let mut items = collect(content, &tree, query, ts_kind, ts_exported);
                for item in &mut items {
                    item.start_line += line_offset;
                    item.end_line += line_offset;
                }
                raws.extend(items);
            }
            Err(message) => {
                if error.is_none() {
                    error = Some(message);
                }
            }
        }
    }

    assemble(Language::Svelte, raws, error)
}

/// Locate `<script …>…</script>` blocks and return `(line offset, content)`.
///
/// `line_offset` is the number of newlines before the content starts, so a
/// 1-based line inside the block maps to `line + line_offset` in the file.
fn script_blocks(source: &str) -> Vec<(u32, &str)> {
    let mut blocks = Vec::new();
    let mut position = 0usize;
    while let Some(found) = source[position..].find("<script") {
        let tag_start = position + found;
        let after_name = tag_start + "<script".len();
        // Reject `<scripture>` and similar tags.
        match source[after_name..].chars().next() {
            Some(c) if c.is_whitespace() || c == '>' => {}
            _ => {
                position = after_name;
                continue;
            }
        }
        let Some(gt) = source[after_name..].find('>') else {
            break;
        };
        let content_start = after_name + gt + 1;
        let Some(close) = source[content_start..].find("</script") else {
            break;
        };
        let content_end = content_start + close;
        let line_offset = source[..content_start]
            .bytes()
            .filter(|b| *b == b'\n')
            .count() as u32;
        blocks.push((line_offset, &source[content_start..content_end]));
        position = content_end + "</script".len();
    }
    blocks
}

fn extract_with(
    source: &str,
    language: &TsLanguage,
    query: &Query,
    outline_language: Language,
    kind_of: fn(&str) -> Option<ItemKind>,
    exported: fn(&str, &str, bool) -> bool,
) -> Outline {
    let tree = match parse(source, language) {
        Ok(tree) => tree,
        Err(error) => return Outline::failed(outline_language, error),
    };
    let error = tree
        .root_node()
        .has_error()
        .then(|| "syntax errors in source".to_string());
    let raws = collect(source, &tree, query, kind_of, exported);
    assemble(outline_language, raws, error)
}

fn parse(source: &str, language: &TsLanguage) -> Result<Tree, String> {
    let mut parser = Parser::new();
    parser
        .set_language(language)
        .map_err(|e| format!("failed to load grammar: {e}"))?;
    parser
        .parse(source, None)
        .ok_or_else(|| "parser produced no tree".to_string())
}

fn collect(
    source: &str,
    tree: &Tree,
    query: &Query,
    kind_of: fn(&str) -> Option<ItemKind>,
    exported: fn(&str, &str, bool) -> bool,
) -> Vec<RawItem> {
    let text = source.as_bytes();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), text);
    let mut items = Vec::new();

    while let Some(matched) = matches.next() {
        let mut name_node: Option<Node> = None;
        let mut item_node: Option<Node> = None;
        let mut kind: Option<ItemKind> = None;
        for capture in matched.captures {
            let capture_name = query.capture_names()[capture.index as usize];
            if capture_name == "name" {
                name_node = Some(capture.node);
            } else if let Some(found) = kind_of(capture_name) {
                item_node = Some(capture.node);
                kind = Some(found);
            }
        }
        let (Some(node), Some(kind)) = (item_node, kind) else {
            continue;
        };
        let name = name_node
            .and_then(|n| n.utf8_text(text).ok())
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .or_else(|| derive_name(node, text, kind));
        let Some(name) = name else {
            continue;
        };
        let signature = signature_of(node, text);
        let export_prefix = has_export_prefix(node, text);
        items.push(RawItem {
            name: name.clone(),
            kind,
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            start_line: start_line(node),
            end_line: end_line(node),
            signature: signature.clone(),
            exported: exported(&name, &signature, export_prefix),
        });
    }

    items
}

/// Query patterns sometimes only capture the declaration node (`impl_item`);
/// fall back to its first line for a stable label.
fn derive_name(node: Node, source: &[u8], kind: ItemKind) -> Option<String> {
    let signature = signature_of(node, source);
    if kind == ItemKind::Impl {
        let rest = signature.strip_prefix("impl").unwrap_or(&signature).trim();
        let rest = rest.strip_suffix('{').unwrap_or(rest).trim();
        if !rest.is_empty() {
            return Some(rest.to_string());
        }
    }
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .find(|child| {
            matches!(
                child.kind(),
                "identifier"
                    | "type_identifier"
                    | "field_identifier"
                    | "property_identifier"
                    | "constant"
            )
        })
        .and_then(|child| child.utf8_text(source).ok())
        .map(|name| name.trim().to_string());
    found
}

fn signature_of(node: Node, source: &[u8]) -> String {
    let text = node.utf8_text(source).unwrap_or("");
    let first_line = text.lines().next().unwrap_or("").trim();
    let collapsed = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&collapsed, MAX_SIGNATURE_CHARS)
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut truncated: String = value.chars().take(max.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

fn start_line(node: Node) -> u32 {
    node.start_position().row as u32 + 1
}

fn end_line(node: Node) -> u32 {
    let end = node.end_position();
    let start = node.start_position();
    if end.column == 0 && end.row > start.row {
        // The node ended at the start of the next line; the last content line
        // is the previous one (1-based equals the 0-based row).
        end.row as u32
    } else {
        end.row as u32 + 1
    }
}

fn rust_kind(capture: &str) -> Option<ItemKind> {
    Some(match capture {
        "function" => ItemKind::Function,
        "struct" => ItemKind::Struct,
        "enum" => ItemKind::Enum,
        "trait" => ItemKind::Trait,
        "impl" => ItemKind::Impl,
        "module" => ItemKind::Module,
        "const" => ItemKind::Const,
        "type" => ItemKind::Type,
        "macro" => ItemKind::Macro,
        _ => return None,
    })
}

fn ts_kind(capture: &str) -> Option<ItemKind> {
    Some(match capture {
        "function" => ItemKind::Function,
        "class" => ItemKind::Class,
        "interface" => ItemKind::Interface,
        "enum" => ItemKind::Enum,
        "type" => ItemKind::Type,
        "module" => ItemKind::Module,
        "method" => ItemKind::Method,
        _ => return None,
    })
}

fn python_kind(capture: &str) -> Option<ItemKind> {
    Some(match capture {
        "function" => ItemKind::Function,
        "class" => ItemKind::Class,
        _ => return None,
    })
}

fn go_kind(capture: &str) -> Option<ItemKind> {
    Some(match capture {
        "function" => ItemKind::Function,
        "method" => ItemKind::Method,
        "struct" => ItemKind::Struct,
        "interface" => ItemKind::Interface,
        _ => return None,
    })
}

fn ts_exported(_name: &str, _signature: &str, export_prefix: bool) -> bool {
    export_prefix
}

/// TypeScript wraps exported declarations in an `export_statement`, so the
/// declaration node's own text does not contain the keyword. Look at the
/// source prefix on the same line instead.
fn has_export_prefix(node: Node, source: &[u8]) -> bool {
    let start = node.start_byte();
    let line_start = source[..start]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    std::str::from_utf8(&source[line_start..start])
        .map(|prefix| prefix.split_whitespace().any(|token| token == "export"))
        .unwrap_or(false)
}
