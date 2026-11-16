//! Computes document symbols for a parsed zsh document.
//!
//! `source`/`.` and `alias` have no dedicated grammar nodes; their symbols are recognized by the command's name.
//!
//! Function definitions do have one: every declaration form parses to a
//! `function_definition` node, so their symbols come from the tree.
//!
//! Variable assignments also do: a `variable_assignment` node.

use std::collections::HashSet;

use tower_lsp_server::ls_types::{DocumentSymbol, Position, Range, SymbolKind};
use tree_sitter::{Node, Tree, TreeCursor};

/// Command names that pull another file into the current shell.
const SOURCE_COMMANDS: &[&str] = &["source", "."];

/// The command name that defines shell aliases.
const ALIAS_COMMAND: &str = "alias";

/// The declaration flag that marks the array being assigned as associative.
const ASSOCIATIVE_ARRAY_FLAG: char = 'A';

/// The name reported for lamba functions, e.g. `() { echo hi }`.
const ANONYMOUS_FUNCTION_NAME: &str = "Anon function";

/// Every symbol in `tree`: the `source`/`.` commands, the aliases and the
/// function definitions, in document order.
pub fn get_symbols(tree: &Tree, text: &str) -> Vec<DocumentSymbol> {
    let mut symbols = get_source_symbols(tree, text);
    symbols.extend(get_alias_symbols(tree, text));
    symbols.extend(get_function_symbols(tree, text));
    symbols.extend(get_variable_symbols(tree, text));

    // The kinds are gathered separately, so interleave them by start.
    symbols.sort_by_key(|symbol| (symbol.range.start.line, symbol.range.start.character));
    symbols
}

/// Returns a [`SymbolKind::FUNCTION`] symbol for every function definition.
fn get_function_symbols(tree: &Tree, text: &str) -> Vec<DocumentSymbol> {
    nodes_of_kind(tree, "function_definition")
        .into_iter()
        .filter_map(|node| function_symbol(node, text))
        .collect()
}

/// Returns a [`SymbolKind::MODULE`] symbol for every `source`/`.` command.
fn get_source_symbols(tree: &Tree, text: &str) -> Vec<DocumentSymbol> {
    command_nodes(tree)
        .into_iter()
        .filter(|command| SOURCE_COMMANDS.contains(&command_name(*command, text)))
        .filter_map(|command| source_symbol(command, text))
        .collect()
}

/// Returns a [`SymbolKind::CONSTANT`] symbol for every alias defined with the
/// `alias` command.
fn get_alias_symbols(tree: &Tree, text: &str) -> Vec<DocumentSymbol> {
    command_nodes(tree)
        .into_iter()
        .filter(|command| command_name(*command, text) == ALIAS_COMMAND)
        .filter_map(|command| alias_symbol(command, text))
        .collect()
}

/// Returns a symbol for every variable assignment: [`SymbolKind::OBJECT`] for
/// associative arrays, [`SymbolKind::ARRAY`] for arrays and
/// [`SymbolKind::VARIABLE`] for every other form.
fn get_variable_symbols(tree: &Tree, text: &str) -> Vec<DocumentSymbol> {
    // Dedup because each is a superset of the previous.
    // Array assignments are associative array assignments,
    // And they're also normal assignments.
    let passes = [
        (
            get_associative_array_assignments(tree, text),
            SymbolKind::OBJECT,
        ),
        (get_array_assignments(tree), SymbolKind::ARRAY),
        (get_all_assignments(tree), SymbolKind::VARIABLE),
    ];

    let mut seen = HashSet::new();
    let mut symbols: Vec<DocumentSymbol> = passes
        .into_iter()
        .flat_map(|(nodes, kind)| nodes.into_iter().map(move |node| (node, kind)))
        .filter(|(node, _)| seen.insert(node.id()))
        .map(|(node, kind)| variable_symbol(node, kind, text))
        .collect();

    // The passes are grouped by kind, so interleave them by start.
    symbols.sort_by_key(|symbol| (symbol.range.start.line, symbol.range.start.character));
    symbols
}

/// The `variable_assignment` children of every `-A` declaration command, e.g.
/// the `ASSOC=( ... )` in `declare -A ASSOC=( ... )`.
fn get_associative_array_assignments<'a>(tree: &'a Tree, text: &str) -> Vec<Node<'a>> {
    nodes_of_kind(tree, "declaration_command")
        .into_iter()
        .filter(|command| declares_associative_array(*command, text))
        .flat_map(assignment_children)
        .collect()
}

/// The assignments whose `value` is a non-empty `array`.
fn get_array_assignments<'a>(tree: &'a Tree) -> Vec<Node<'a>> {
    get_all_assignments(tree)
        .into_iter()
        .filter(|assignment| is_non_empty_array(*assignment))
        .collect()
}

/// Every `variable_assignment` node in `tree`.
fn get_all_assignments<'a>(tree: &'a Tree) -> Vec<Node<'a>> {
    nodes_of_kind(tree, "variable_assignment")
}

/// Whether a `declaration_command` carries the `-A` flag, which declares the
/// arrays it assigns as associative.
fn declares_associative_array(command: Node<'_>, text: &str) -> bool {
    arguments(command).into_iter().any(|argument| {
        let argument = node_text(argument, text);
        // The flag may be combined with others, e.g. `declare -Ag`.
        argument.starts_with('-')
            && !argument.starts_with("--")
            && argument.contains(ASSOCIATIVE_ARRAY_FLAG)
    })
}

/// Whether an assignment's `value` is an `array` holding at least one element.
fn is_non_empty_array(assignment: Node<'_>) -> bool {
    assignment
        .child_by_field_name("value")
        .is_some_and(|value| value.kind() == "array" && value.named_child_count() > 0)
}

/// The `variable_assignment` children of a node.
fn assignment_children<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == "variable_assignment")
        .collect()
}

/// Every `command` node in `tree`, in source order.
fn command_nodes<'a>(tree: &'a Tree) -> Vec<Node<'a>> {
    nodes_of_kind(tree, "command")
}

/// Every node of `kind` in `tree`, in source order.
fn nodes_of_kind<'a>(tree: &'a Tree, kind: &str) -> Vec<Node<'a>> {
    let mut nodes = Vec::new();
    let mut cursor = tree.walk();
    collect_nodes(&mut cursor, kind, &mut nodes);
    nodes
}

/// Depth-first walk, collecting every node it visits whose kind is `kind`.
fn collect_nodes<'a>(cursor: &mut TreeCursor<'a>, kind: &str, nodes: &mut Vec<Node<'a>>) {
    let node = cursor.node();
    if node.kind() == kind {
        nodes.push(node);
    }

    if cursor.goto_first_child() {
        loop {
            collect_nodes(cursor, kind, nodes);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

/// Builds a symbol for a `source`/`.` command, named after its file argument.
fn source_symbol(command: Node<'_>, text: &str) -> Option<DocumentSymbol> {
    let file = *operands(command, text).first()?;

    Some(document_symbol(
        strip_quotes(node_text(file, text)).to_string(),
        None,
        SymbolKind::MODULE,
        node_range(command, text),
        // The argument, minus its quotes, is exactly the reported name.
        unquoted_node_range(file, text),
    ))
}

/// Builds a symbol for an `alias` command's `name=value` assignment.
fn alias_symbol(command: Node<'_>, text: &str) -> Option<DocumentSymbol> {
    let words = operands(command, text);

    // The name is a prefix of the first operand, so that node anchors its
    // selection range.
    let first = *words.first()?;
    let assignment: String = words.iter().map(|word| node_text(*word, text)).collect();

    let (name, value) = assignment.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    Some(document_symbol(
        name.to_string(),
        Some(format!("Alias for {}", strip_quotes(value.trim()))),
        SymbolKind::CONSTANT,
        node_range(command, text),
        node_prefix_range(first, name.len(), text),
    ))
}

/// Builds a symbol for a `function_definition`, named after its `name` child. Also handles anonymous
/// functions.
fn function_symbol(node: Node<'_>, text: &str) -> Option<DocumentSymbol> {
    let name = node.child_by_field_name("name");

    // An anonymous function has no name child, so there is nothing to report as
    // its name and nothing to select.
    let (name, selection_range) = match name {
        Some(name) => (node_text(name, text).to_string(), node_range(name, text)),
        None => (ANONYMOUS_FUNCTION_NAME.to_string(), node_range(node, text)),
    };

    Some(document_symbol(
        name,
        None,
        SymbolKind::FUNCTION,
        node_range(node, text),
        selection_range,
    ))
}

/// Builds a symbol for a `variable_assignment`, named after its `name` child.
///
/// The name is a `subscript` for `INDEXED[2]=value`, so it reads `INDEXED[2]`.
fn variable_symbol(node: Node<'_>, kind: SymbolKind, text: &str) -> DocumentSymbol {
    let name = node.child_by_field_name("name");
    let (name, selection_range) = match name {
        Some(name) => (node_text(name, text).to_string(), node_range(name, text)),
        None => (String::new(), node_range(node, text)),
    };

    document_symbol(name, None, kind, node_range(node, text), selection_range)
}

/// Assembles a [`DocumentSymbol`] spanning `range`, whose `selection_range`
/// selects the symbol's name within it.
///
/// `selection_range` must be contained in `range`; callers pass a descendant of
/// the node behind `range`.
#[allow(deprecated)] // `DocumentSymbol::deprecated` is deprecated in favour of `tags`.
fn document_symbol(
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    range: Range,
    selection_range: Range,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    }
}

/// The text of a command's `name` (its `command_name` child).
fn command_name<'a>(command: Node<'_>, text: &'a str) -> &'a str {
    command
        .child_by_field_name("name")
        .and_then(|name| name.utf8_text(text.as_bytes()).ok())
        .unwrap_or("")
}

/// The `argument` children of a `command`.
fn arguments<'a>(command: Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = command.walk();
    command
        .children_by_field_name("argument", &mut cursor)
        .collect()
}

/// The `argument` children of a `command` that aren't options (i.e. whose text
/// doesn't start with `-`).
fn operands<'a>(command: Node<'a>, text: &str) -> Vec<Node<'a>> {
    arguments(command)
        .into_iter()
        .filter(|argument| !node_text(*argument, text).starts_with('-'))
        .collect()
}

/// The LSP [`Range`] covering a node.
fn node_range(node: Node<'_>, text: &str) -> Range {
    Range {
        start: position(node.start_byte(), text),
        end: position(node.end_byte(), text),
    }
}

/// The [`Range`] covering the first `len` bytes of a node, clamped to the node.
fn node_prefix_range(node: Node<'_>, len: usize, text: &str) -> Range {
    let start = node.start_byte();
    let len = len.min(node.end_byte() - start);
    Range {
        start: position(start, text),
        end: position(start + len, text),
    }
}

/// A node's [`Range`] with one layer of matching quotes removed, when the node
/// is quoted.
fn unquoted_node_range(node: Node<'_>, text: &str) -> Range {
    if !is_quoted(node_text(node, text)) {
        return node_range(node, text);
    }

    Range {
        start: position(node.start_byte() + 1, text),
        end: position(node.end_byte() - 1, text),
    }
}

/// Convert a byte offset in the document into an LSP [`Position`].
fn position(byte: usize, text: &str) -> Position {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }

    let line_start = text[..byte].rfind('\n').map_or(0, |index| index + 1);
    Position {
        line: text[..line_start].matches('\n').count() as u32,
        character: text[line_start..byte].encode_utf16().count() as u32,
    }
}

/// The source text of a node.
fn node_text<'a>(node: Node<'_>, text: &'a str) -> &'a str {
    node.utf8_text(text.as_bytes()).unwrap_or("")
}

/// Whether `value` is wrapped in one layer of matching single or double quotes.
fn is_quoted(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[0] == bytes[bytes.len() - 1]
}

/// Remove one layer of matching single or double quotes.
fn strip_quotes(value: &str) -> &str {
    if is_quoted(value) {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse(text: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_zsh::LANGUAGE.into())
            .expect("zsh grammar is valid");
        parser.parse(text, None).expect("tree is produced")
    }

    /// An LSP range, with every position on `line`.
    fn line_range(line: u32, start: u32, end: u32) -> Range {
        Range {
            start: Position {
                line,
                character: start,
            },
            end: Position {
                line,
                character: end,
            },
        }
    }

    #[test]
    fn source_command_yields_module_symbol() {
        let text = "source ~/.zshrc\n";
        let symbols = get_source_symbols(&parse(text), text);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "~/.zshrc");
        assert_eq!(symbols[0].kind, SymbolKind::MODULE);
        assert_eq!(symbols[0].detail, None);
    }

    #[test]
    fn dot_command_yields_module_symbol() {
        let text = ". ./lib.zsh\n";
        let symbols = get_source_symbols(&parse(text), text);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "./lib.zsh");
    }

    #[test]
    fn alias_command_yields_constant_symbol() {
        let text = "alias lzg=\"lazygit\"\n";
        let symbols = get_alias_symbols(&parse(text), text);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "lzg");
        assert_eq!(symbols[0].kind, SymbolKind::CONSTANT);
        assert_eq!(symbols[0].detail.as_deref(), Some("Alias for lazygit"));
    }

    #[test]
    fn unquoted_alias_yields_constant_symbol() {
        let text = "alias l=lazygit\n";
        let symbols = get_alias_symbols(&parse(text), text);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "l");
        assert_eq!(symbols[0].detail.as_deref(), Some("Alias for lazygit"));
    }

    #[test]
    fn source_and_alias_are_distinct() {
        let text = "source ~/.zshrc\nalias l=lazygit\n";
        let tree = parse(text);
        assert_eq!(get_source_symbols(&tree, text).len(), 1);
        assert_eq!(get_alias_symbols(&tree, text).len(), 1);
    }

    #[test]
    fn unrelated_commands_yield_nothing() {
        let text = "echo hi\nls -la\n";
        let tree = parse(text);
        assert!(get_source_symbols(&tree, text).is_empty());
        assert!(get_alias_symbols(&tree, text).is_empty());
    }

    #[test]
    fn get_symbols_interleaves_sources_and_aliases_in_document_order() {
        let text = "alias a=1\nsource ~/.zshrc\nalias b=2\n. ./lib.zsh\n";
        let symbols = get_symbols(&parse(text), text);

        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["a", "~/.zshrc", "b", "./lib.zsh"]);
    }

    #[test]
    fn range_and_selection_range_are_decoupled() {
        let text = "alias lzg=\"lazygit\"\n";
        let symbols = get_alias_symbols(&parse(text), text);
        assert_eq!(symbols.len(), 1);

        let symbol = &symbols[0];
        assert_eq!(symbol.range, line_range(0, 0, 19));
        assert_eq!(symbol.selection_range, line_range(0, 6, 9));
    }

    #[test]
    fn source_selection_covers_the_file_argument() {
        let text = "source \"~/.zshrc\"\n";
        let symbols = get_source_symbols(&parse(text), text);
        assert_eq!(symbols.len(), 1);

        let symbol = &symbols[0];
        assert_eq!(symbol.name, "~/.zshrc");
        assert_eq!(symbol.range, line_range(0, 0, 17));
        // The quotes are excluded, so the selection matches the name exactly.
        assert_eq!(symbol.selection_range, line_range(0, 8, 16));
    }

    #[test]
    fn character_offsets_count_utf16_code_units() {
        // `alias l='é🌍'` is 16 bytes but 13 UTF-16 code units: `é` is 2 bytes
        // in 1 unit and `🌍` is 4 bytes in 2 units.
        let text = "alias l='é🌍'\n";
        let symbols = get_alias_symbols(&parse(text), text);
        assert_eq!(symbols.len(), 1);

        let symbol = &symbols[0];
        assert_eq!(symbol.name, "l");
        assert_eq!(symbol.range, line_range(0, 0, 13));
        assert_eq!(symbol.selection_range, line_range(0, 6, 7));
    }

    #[test]
    fn multibyte_text_before_a_symbol_shifts_its_start() {
        // `é` is 2 bytes, so the alias command starts at byte 10 but unit 9.
        let text = "écho hi; alias l=x\n";
        let symbols = get_alias_symbols(&parse(text), text);
        assert_eq!(symbols.len(), 1);

        let symbol = &symbols[0];
        assert_eq!(symbol.name, "l");
        assert_eq!(symbol.range, line_range(0, 9, 18));
        assert_eq!(symbol.selection_range, line_range(0, 15, 16));
    }

    #[test]
    fn every_function_declaration_form_yields_a_function_symbol() {
        for text in [
            "foo() {\n  echo hi\n}\n",
            "foo () {\n  echo hi\n}\n",
            "foo()\n{\n  echo hi\n}\n",
            "function foo {\n  echo hi\n}\n",
            "function foo() {\n  echo hi\n}\n",
            "function foo\n{\n  echo hi\n}\n",
            "function foo()\n{\n  echo hi\n}\n",
            "foo() (\n  echo hi\n)\n",
        ] {
            let symbols = get_symbols(&parse(text), text);
            assert_eq!(symbols.len(), 1, "no symbol for {text:?}");
            assert_eq!(symbols[0].name, "foo", "wrong name for {text:?}");
            assert_eq!(symbols[0].kind, SymbolKind::FUNCTION);
        }
    }

    #[test]
    fn anonymous_function_yields_anon_function_symbol() {
        // `() { ... }` is a zsh 5.4 anonymous function; it has no `name` child.
        let text = "() {\n  echo hi\n}\n";
        let symbols = get_function_symbols(&parse(text), text);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Anon function");
        assert_eq!(symbols[0].kind, SymbolKind::FUNCTION);
        assert_eq!(symbols[0].detail, None);
        // There is no name to select, so the whole node is selected.
        assert_eq!(symbols[0].selection_range, symbols[0].range);
    }

    #[test]
    fn nested_and_sibling_functions_yield_a_symbol_each() {
        let text = "outer() {\n  inner() {\n    echo hi\n  }\n}\nbuild() {\n  echo hi\n}\n";
        let symbols = get_function_symbols(&parse(text), text);

        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["outer", "inner", "build"]);
    }

    #[test]
    fn function_range_spans_the_body_while_selection_covers_the_name() {
        let text = "foo() { echo hi; }\n";
        let symbols = get_function_symbols(&parse(text), text);
        assert_eq!(symbols.len(), 1);

        let symbol = &symbols[0];
        assert_eq!(symbol.range, line_range(0, 0, 18));
        assert_eq!(symbol.selection_range, line_range(0, 0, 3));
        // A named function carries no detail.
        assert_eq!(symbol.detail, None);
    }

    #[test]
    fn plain_assignments_yield_variable_symbols() {
        // `EMPTY=` goes last: the grammar takes the next line's word as its value.
        let text = "PLAIN=value\nAPPENDED+=more\nEMPTY=\n";
        let symbols = get_variable_symbols(&parse(text), text);

        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["PLAIN", "APPENDED", "EMPTY"]);
        assert!(
            symbols
                .iter()
                .all(|symbol| symbol.kind == SymbolKind::VARIABLE)
        );
    }

    #[test]
    fn empty_assignment_absorbs_the_following_line() {
        // The grammar reads the word on the next line as the value of `EMPTY=`,
        // so the second assignment is not a node of its own.
        let text = "EMPTY=\nSWALLOWED=value\n";
        let symbols = get_variable_symbols(&parse(text), text);

        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["EMPTY"]);
    }

    #[test]
    fn every_array_declaration_form_yields_an_array_symbol() {
        // The elements parse as different kinds, but each form is an array.
        for text in [
            "A=(one two three)",
            "A=(\"a b\" c)",
            "A=('x' 'y')",
            "A=($VAR)",
            "A=($(date))",
            "A=(*.txt)",
            "A=({1..3})",
            "A=([1]=x [2]=y)",
            "A=(k=v)",
        ] {
            let text = format!("{text}\n");
            let symbols = get_variable_symbols(&parse(&text), &text);
            assert_eq!(symbols.len(), 1, "wrong symbol count for {text:?}");
            assert_eq!(symbols[0].name, "A", "wrong name for {text:?}");
            assert_eq!(
                symbols[0].kind,
                SymbolKind::ARRAY,
                "wrong kind for {text:?}"
            );
        }
    }

    #[test]
    fn array_declarations_inside_a_declaration_command_yield_an_array_symbol() {
        // Without `-A`, `declare` declares an ordinary array.
        let text = "declare ARR=(a b)\n";
        let symbols = get_variable_symbols(&parse(text), text);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "ARR");
        assert_eq!(symbols[0].kind, SymbolKind::ARRAY);
    }

    #[test]
    fn associative_arrays_yield_an_object_symbol() {
        for text in [
            "declare -A ASSOC=( [key]=value )\n",
            "declare -A EMPTY=()\n",
            "declare -Ag GLOBAL=( [k]=v )\n",
            "local -A LOCAL=( [k]=v )\n",
            "typeset -A TYPESET=( [k]=v )\n",
        ] {
            let symbols = get_variable_symbols(&parse(text), text);
            assert_eq!(symbols.len(), 1, "wrong symbol count for {text:?}");
            assert_eq!(
                symbols[0].kind,
                SymbolKind::OBJECT,
                "wrong kind for {text:?}"
            );
        }
    }

    #[test]
    fn declaration_without_an_assignment_yields_no_symbol() {
        let text = "declare -A BARE\ndeclare -x OTHER\n";
        assert!(get_variable_symbols(&parse(text), text).is_empty());
    }

    #[test]
    fn each_assignment_yields_exactly_one_symbol() {
        // Every array and associative array is also a plain
        // `variable_assignment`, so three traversals would report it thrice.
        for text in ["A=(a b)\n", "declare -A A=( [k]=v )\n", "A=()\n", "A=x\n"] {
            assert_eq!(
                get_variable_symbols(&parse(text), text).len(),
                1,
                "for {text:?}"
            );
        }
    }

    #[test]
    fn empty_array_falls_back_to_a_variable_symbol() {
        // With no elements there is nothing to tell an array from an empty value.
        let text = "EMPTY_ARRAY=()\n";
        let symbols = get_variable_symbols(&parse(text), text);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].kind, SymbolKind::VARIABLE);
    }

    #[test]
    fn several_assignments_yield_a_symbol_each() {
        let text = "FIRST=one SECOND=two\n";
        let symbols = get_variable_symbols(&parse(text), text);

        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["FIRST", "SECOND"]);
    }

    #[test]
    fn assignment_prefixing_a_command_yields_a_variable_symbol() {
        let text = "PREFIX=value some_command\n";
        let symbols = get_variable_symbols(&parse(text), text);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "PREFIX");
        assert_eq!(symbols[0].kind, SymbolKind::VARIABLE);
    }

    #[test]
    fn subscripted_assignment_is_named_after_its_subscript() {
        let text = "INDEXED[2]=value\n";
        let symbols = get_variable_symbols(&parse(text), text);
        assert_eq!(symbols.len(), 1);

        let symbol = &symbols[0];
        assert_eq!(symbol.name, "INDEXED[2]");
        assert_eq!(symbol.range, line_range(0, 0, 16));
        assert_eq!(symbol.selection_range, line_range(0, 0, 10));
    }

    #[test]
    fn expansion_right_hand_sides_yield_variable_symbols() {
        let text = "FROM_VAR=${OTHER}\nFROM_COMMAND=$(echo hi)\nFROM_ARITHMETIC=$((1 + 2))\n";
        let symbols = get_variable_symbols(&parse(text), text);

        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["FROM_VAR", "FROM_COMMAND", "FROM_ARITHMETIC"]);
        assert!(
            symbols
                .iter()
                .all(|symbol| symbol.kind == SymbolKind::VARIABLE)
        );
    }

    #[test]
    fn variables_interleave_with_the_other_symbols() {
        let text = "alias l=x\nFOO=bar\nfoo() { echo hi }\n";
        let symbols = get_symbols(&parse(text), text);

        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["l", "FOO", "foo"]);
    }
}
