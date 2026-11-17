//! Computes folding ranges for a parsed zsh document.
//! Any node whose kind appears in [`FOLDABLE_NODE_KINDS`] becomes a foldable
//! region spanning that node's first to last line.

use std::collections::HashSet;

use tree_sitter::Node;

use crate::analysis::Analyzer;

/// tree-sitter-zsh node kinds that should produce a folding range.
/// Every entry maps directly onto a construct in the grammar.
const FOLDABLE_NODE_KINDS: &[&str] = &[
    "function_definition",
    "if_statement",
    "elif_clause",
    "else_clause",
    "compound_statement",
    "compound_statement_no_always",
    "always_clause",
    "for_statement",
    "terse_for_statement",
    "c_style_for_statement",
    "while_statement",
    "repeat_statement",
    "select_statement",
    "case_statement",
    "case_item",
];

/// Collects the foldable regions of one document.
#[derive(Debug, Default)]
pub struct FoldingAnalyzer {
    ranges: Vec<(u32, u32)>,

    // Nested watched nodes can share the same line span (e.g. a function and its
    // `{ ... }` body are both `(start, end)`). This is to dedup those ranges.
    seen: HashSet<(u32, u32)>,
}

impl FoldingAnalyzer {
    /// The foldable regions found, as `(start_line, end_line)` pairs.
    pub fn ranges(&self) -> &[(u32, u32)] {
        &self.ranges
    }
}

impl Analyzer for FoldingAnalyzer {
    fn reset(&mut self) {
        self.ranges.clear();
        self.seen.clear();
    }

    fn visit(&mut self, node: Node<'_>, _text: &str) {
        if !FOLDABLE_NODE_KINDS.contains(&node.kind()) {
            return;
        }

        let start = node.start_position().row as u32;
        let mut end = node.end_position().row as u32;

        // Some clauses (`elif`, `else`) end at column 0 of the line that
        // follows them, so that final row carries none of the clause's own
        // content. Fold only down to the last line that belongs to the node.
        if end > start && node.end_position().column == 0 {
            end -= 1;
        }

        // Skip nodes that span a single line, and nodes repeating a span.
        if start < end && self.seen.insert((start, end)) {
            self.ranges.push((start, end));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::{Parser, Tree};

    fn parse(text: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_zsh::LANGUAGE.into())
            .expect("zsh grammar is valid");
        parser.parse(text, None).expect("tree is produced")
    }

    /// The ranges the analyzer finds for `text`, parsed on its own.
    fn folding_ranges(text: &str) -> Vec<(u32, u32)> {
        let tree = parse(text);
        let mut analyzer = FoldingAnalyzer::default();
        crate::analysis::run(&tree, text, &mut [&mut analyzer]);
        analyzer.ranges().to_vec()
    }

    #[test]
    fn function_definition_yields_range() {
        assert_eq!(
            folding_ranges("foo() {\n  echo hi\n  echo bye\n}\n"),
            vec![(0, 3)]
        );
    }

    #[test]
    fn single_line_function_is_omitted() {
        // A one-line node has `start == end`, so it is skipped.
        assert!(folding_ranges("foo() { echo hi; }\n").is_empty());
    }

    #[test]
    fn two_line_function_yields_range() {
        assert_eq!(folding_ranges("foo() {\n}\n"), vec![(0, 1)]);
    }

    #[test]
    fn no_foldable_nodes_yields_empty() {
        assert!(folding_ranges("echo a\necho b\n").is_empty());
    }

    #[test]
    fn if_elif_else_yield_nested_ranges() {
        // The `elif`/`else` clauses end at column 0 of the following line, so
        // their ranges shrink back to their last content line.
        assert_eq!(
            folding_ranges("if a; then\n  x\nelif b; then\n  y\nelse\n  z\nfi\n"),
            vec![(0, 6), (2, 3), (4, 5)]
        );
    }

    #[test]
    fn brace_group_with_always_yields_ranges() {
        // `always_clause` and its `{ ... }` body share a span, so the duplicate
        // collapses to a single range.
        assert_eq!(
            folding_ranges("{\n  echo try\n} always {\n  echo cleanup\n}\n"),
            vec![(0, 4), (2, 4)]
        );
    }

    #[test]
    fn loop_forms_yield_ranges() {
        assert_eq!(
            folding_ranges("for x in a b; do\n  echo $x\ndone\n"),
            vec![(0, 2)]
        );
        assert_eq!(
            folding_ranges("while true; do\n  echo hi\n  break\ndone\n"),
            vec![(0, 3)]
        );
        assert_eq!(
            folding_ranges("until false; do\n  echo hi\n  break\ndone\n"),
            vec![(0, 3)]
        );
    }

    #[test]
    fn case_statement_and_arms_yield_ranges() {
        assert_eq!(
            folding_ranges(
                "case $1 in\n  a)\n    echo a\n    echo a2\n    ;;\n  b)\n    echo b\n    echo b2\n    ;;\nesac\n"
            ),
            vec![(0, 9), (1, 4), (5, 8)]
        );
    }
}
