//! Runs the document analyses over a parse tree.
//!
//! Every analysis is an [`Analyzer`], which is passed each node of the tree
//! in document order. One traversal of the parse tree runs every analyzer.

use std::collections::HashMap;

use tower_lsp_server::ls_types::{DocumentSymbol, Uri};
use tree_sitter::{Node, Tree, TreeCursor};

use crate::folding::FoldingAnalyzer;
use crate::symbols::SymbolAnalyzer;

/// An analysis that is fed the nodes of one document's tree, in document order.
pub trait Analyzer {
    /// Drops everything learned from the previous tree.
    fn reset(&mut self);

    /// Consumes one node of the document's tree.
    fn visit(&mut self, node: Node<'_>, text: &str);
}

/// The analyses kept for a single document.
#[derive(Debug, Default)]
struct DocumentAnalyses {
    folding: FoldingAnalyzer,
    symbols: SymbolAnalyzer,
}

/// Maintains the language server's in-memory representation of analysis. Documentes are indexed by URI.
#[derive(Debug, Default)]
pub struct AnalysisStore {
    documents: HashMap<Uri, DocumentAnalyses>,
}

impl AnalysisStore {
    /// Runs every analysis over `tree`.
    pub fn analyze(&mut self, uri: &Uri, tree: &Tree, text: &str) {
        let analyses = self.documents.entry(uri.clone()).or_default();

        let mut analyzers: [&mut dyn Analyzer; 2] = [&mut analyses.folding, &mut analyses.symbols];
        for analyzer in analyzers.iter_mut() {
            analyzer.reset();
        }

        run(tree, text, &mut analyzers);
    }

    /// Closes a document, removing it from the store.
    pub fn close(&mut self, uri: &Uri) {
        self.documents.remove(uri);
    }

    /// The folding ranges found for `uri`.
    pub fn folding_ranges(&self, uri: &Uri) -> Option<&[(u32, u32)]> {
        self.documents
            .get(uri)
            .map(|analyses| analyses.folding.ranges())
    }

    /// The symbols found for `uri`.
    pub fn symbols(&self, uri: &Uri) -> Option<&[DocumentSymbol]> {
        self.documents
            .get(uri)
            .map(|analyses| analyses.symbols.symbols())
    }
}

/// Runs every analyzer over the tree, one node at a time.
pub fn run(tree: &Tree, text: &str, analyzers: &mut [&mut dyn Analyzer]) {
    let mut cursor = tree.walk();
    visit(&mut cursor, text, analyzers);
}

/// Depth-first walk in document order.
fn visit(cursor: &mut TreeCursor<'_>, text: &str, analyzers: &mut [&mut dyn Analyzer]) {
    let node = cursor.node();
    for analyzer in analyzers.iter_mut() {
        analyzer.visit(node, text);
    }

    if cursor.goto_first_child() {
        loop {
            visit(cursor, text, analyzers);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
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

    fn uri(path: &str) -> Uri {
        format!("file://{path}").parse().expect("valid uri")
    }

    fn symbol_names(store: &AnalysisStore, path: &str) -> Vec<String> {
        store
            .symbols(&uri(path))
            .unwrap_or_default()
            .iter()
            .map(|symbol| symbol.name.clone())
            .collect()
    }

    #[test]
    fn analyze_records_both_analyses() {
        let text = "foo() {\n  echo hi\n  echo bye\n}\n";
        let mut store = AnalysisStore::default();
        store.analyze(&uri("/tmp/a.zsh"), &parse(text), text);

        assert_eq!(
            store.folding_ranges(&uri("/tmp/a.zsh")),
            Some(&[(0, 3)][..])
        );
        assert_eq!(symbol_names(&store, "/tmp/a.zsh"), ["foo"]);
    }

    #[test]
    fn analyze_replaces_the_previous_analysis() {
        let mut store = AnalysisStore::default();

        store.analyze(&uri("/tmp/a.zsh"), &parse("FIRST=1\n"), "FIRST=1\n");
        store.analyze(&uri("/tmp/a.zsh"), &parse("SECOND=2\n"), "SECOND=2\n");

        // The stale `FIRST` must not survive the re-run.
        assert_eq!(symbol_names(&store, "/tmp/a.zsh"), ["SECOND"]);
    }

    #[test]
    fn analyze_keeps_documents_apart() {
        let mut store = AnalysisStore::default();

        store.analyze(&uri("/tmp/a.zsh"), &parse("FIRST=1\n"), "FIRST=1\n");
        store.analyze(&uri("/tmp/b.zsh"), &parse("SECOND=2\n"), "SECOND=2\n");

        assert_eq!(symbol_names(&store, "/tmp/a.zsh"), ["FIRST"]);
        assert_eq!(symbol_names(&store, "/tmp/b.zsh"), ["SECOND"]);
    }

    #[test]
    fn unanalyzed_document_has_no_results() {
        let store = AnalysisStore::default();
        let uri = uri("/tmp/missing.zsh");

        assert!(store.symbols(&uri).is_none());
        assert!(store.folding_ranges(&uri).is_none());
    }

    #[test]
    fn close_forgets_the_document() {
        let mut store = AnalysisStore::default();
        let uri = uri("/tmp/a.zsh");
        store.analyze(&uri, &parse("FIRST=1\n"), "FIRST=1\n");

        store.close(&uri);

        assert!(store.symbols(&uri).is_none());
    }
}
