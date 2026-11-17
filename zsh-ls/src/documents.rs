//! Maintains the language server's in-memory representation of open documents.
//!
//! Documents are indexed by URI and synchronized with the client through
//! `didOpen`, `didChange`, and `didClose` notifications.

use std::collections::HashMap;
use std::fmt;

use tower_lsp_server::ls_types::{DidChangeTextDocumentParams, TextDocumentItem, Uri};
use tree_sitter::{Parser, Tree};

/// The server's current representation of an open text document.
#[derive(Debug, Clone)]
pub struct Document {
    /// The document's complete text, preserved verbatim.
    pub text: String,

    /// The client's version number for this document.
    pub version: i32,

    /// The parsed syntax tree for `text`. Completely re-parsed on every change.
    pub tree: Tree,
}

/// Stores the documents currently open in the client.
pub struct DocumentStore {
    documents: HashMap<Uri, Document>,

    /// Parser reused across documents, set to the zsh grammar.
    parser: Parser,
}

/// An error encountered while synchronizing a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncError {
    /// A change notification was received for a document that is not open.
    UnknownDocument,
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::UnknownDocument => write!(f, "document is not open"),
        }
    }
}

impl std::error::Error for SyncError {}

impl fmt::Debug for DocumentStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `Parser` is not `Debug`, so skip it (and future fields) explicitly.
        f.debug_struct("DocumentStore")
            .field("documents", &self.documents)
            .finish_non_exhaustive()
    }
}

impl Default for DocumentStore {
    fn default() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_zsh::LANGUAGE.into())
            .expect("zsh grammar is valid");
        Self {
            documents: HashMap::new(),
            parser,
        }
    }
}

impl DocumentStore {
    /// Opens a document, inserting it into the store.
    ///
    /// If a document with the same URI already exists, it is replaced.
    ///
    /// Returns the previous document, if one existed.
    pub fn open(&mut self, item: TextDocumentItem) -> Option<Document> {
        let tree = Self::parse(&mut self.parser, &item.text);
        let previous = self.documents.insert(
            item.uri,
            Document {
                text: item.text,
                version: item.version,
                tree,
            },
        );
        self.trace("didOpen");
        previous
    }

    /// Closes a document, removing it from the store.
    ///
    /// Returns the removed document, or `None` if it was not open.
    pub fn close(&mut self, uri: &Uri) -> Option<Document> {
        let removed = self.documents.remove(uri);
        if removed.is_some() {
            self.trace("didClose");
        }
        removed
    }

    /// Applies a document change notification.
    ///
    /// Only full-document replacements are supported. If multiple content
    /// changes are supplied, only the last one is used.
    ///
    /// Notifications with a version older than the stored version are
    /// silently ignored.
    ///
    /// # Errors
    ///
    /// Returns [`SyncError::UnknownDocument`] if the document is not open.
    pub fn change(&mut self, params: DidChangeTextDocumentParams) -> Result<(), SyncError> {
        let doc = self
            .documents
            .get_mut(&params.text_document.uri)
            .ok_or(SyncError::UnknownDocument)?;

        if params.text_document.version < doc.version {
            return Ok(());
        }

        if let Some(change) = params.content_changes.last() {
            doc.text.clone_from(&change.text);
            doc.tree = Self::parse(&mut self.parser, &doc.text);
        }

        doc.version = params.text_document.version;
        self.trace("didChange");
        Ok(())
    }

    /// Parses `text` in full, returning the parse tree.
    /// Takes `&mut Parser` rather than `&mut self` so callers can
    /// borrow the parser and the document map as disjoint fields.
    fn parse(parser: &mut Parser, text: &str) -> Tree {
        parser
            .parse(text, None)
            .expect("tree-sitter returns a tree once a language is set")
    }

    /// Returns a reference to an open document.
    ///
    /// Returns `None` if no document with the given URI exists.
    pub fn get(&self, uri: &Uri) -> Option<&Document> {
        self.documents.get(uri)
    }

    /// Returns the number of currently open documents.
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Returns whether the store contains no open documents.
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    fn trace(&self, event: &str) {
        eprintln!("zsh-lsp: {event}\n{}", self);
    }
}

impl fmt::Display for DocumentStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} open document(s)", self.documents.len())?;

        let mut documents: Vec<(&Uri, &Document)> = self.documents.iter().collect();
        documents.sort_by(|(left, _), (right, _)| left.as_str().cmp(right.as_str()));

        for (uri, document) in documents {
            write!(f, "\n  {}", uri.as_str())?;
            write!(f, "\n    version: {}", document.version)?;
            write!(f, "\n    text ({} bytes):", document.text.len())?;
            for line in document.text.lines() {
                write!(f, "\n      | {line}")?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::ls_types::{
        TextDocumentContentChangeEvent, VersionedTextDocumentIdentifier,
    };

    fn uri(path: &str) -> Uri {
        format!("file://{path}").parse().expect("valid uri")
    }

    fn open_params(path: &str, version: i32, text: &str) -> TextDocumentItem {
        TextDocumentItem {
            uri: uri(path),
            language_id: "zsh".to_string(),
            version,
            text: text.to_string(),
        }
    }

    fn change_params(path: &str, version: i32, texts: &[&str]) -> DidChangeTextDocumentParams {
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri(path),
                version,
            },
            content_changes: texts
                .iter()
                .map(|text| TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: (*text).to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn open_inserts_document() {
        let mut store = DocumentStore::default();
        let previous = store.open(open_params("/tmp/a.zsh", 1, "echo a\n"));

        assert!(previous.is_none());
        assert_eq!(store.len(), 1);
        let doc = store.get(&uri("/tmp/a.zsh")).expect("document is open");
        assert_eq!(doc.text, "echo a\n");
        assert_eq!(doc.version, 1);
    }

    #[test]
    fn open_parses_tree() {
        let mut store = DocumentStore::default();
        store.open(open_params("/tmp/a.zsh", 1, "echo a\n"));

        let doc = store.get(&uri("/tmp/a.zsh")).expect("document is open");
        let root = doc.tree.root_node();
        assert_eq!(root.kind(), "program");
        assert!(!root.has_error());
    }

    #[test]
    fn open_duplicate_returns_previous() {
        let mut store = DocumentStore::default();
        store.open(open_params("/tmp/a.zsh", 1, "echo a\n"));
        let previous = store.open(open_params("/tmp/a.zsh", 2, "echo b\n"));

        assert_eq!(store.len(), 1);
        let previous = previous.expect("previous document returned");
        assert_eq!(previous.text, "echo a\n");
        assert_eq!(previous.version, 1);
        let doc = store.get(&uri("/tmp/a.zsh")).expect("document is open");
        assert_eq!(doc.text, "echo b\n");
        assert_eq!(doc.version, 2);
    }

    #[test]
    fn close_removes_document() {
        let mut store = DocumentStore::default();
        store.open(open_params("/tmp/a.zsh", 1, "echo a\n"));

        let removed = store.close(&uri("/tmp/a.zsh")).expect("document was open");
        assert_eq!(removed.text, "echo a\n");
        assert!(store.is_empty());
        assert!(store.get(&uri("/tmp/a.zsh")).is_none());
    }

    #[test]
    fn close_unknown_document_returns_none() {
        let mut store = DocumentStore::default();
        assert!(store.close(&uri("/tmp/missing.zsh")).is_none());
    }

    #[test]
    fn change_replaces_full_text() {
        let mut store = DocumentStore::default();
        store.open(open_params("/tmp/a.zsh", 1, "echo a\n"));

        store
            .change(change_params("/tmp/a.zsh", 2, &["echo a\necho b\n"]))
            .expect("change applies");

        let doc = store.get(&uri("/tmp/a.zsh")).expect("document is open");
        assert_eq!(doc.text, "echo a\necho b\n");
        assert_eq!(doc.version, 2);
    }

    #[test]
    fn change_reparses_tree() {
        let mut store = DocumentStore::default();
        store.open(open_params("/tmp/a.zsh", 1, "echo a\n"));

        let before = store
            .get(&uri("/tmp/a.zsh"))
            .expect("document is open")
            .tree
            .root_node()
            .named_child_count();

        store
            .change(change_params(
                "/tmp/a.zsh",
                2,
                &["echo a\necho b\necho c\n"],
            ))
            .expect("change applies");

        let doc = store.get(&uri("/tmp/a.zsh")).expect("document is open");
        let root = doc.tree.root_node();
        assert!(!root.has_error());
        assert!(root.named_child_count() > before);
    }

    #[test]
    fn change_multiple_events_last_wins() {
        let mut store = DocumentStore::default();
        store.open(open_params("/tmp/a.zsh", 1, "echo a\n"));

        store
            .change(change_params(
                "/tmp/a.zsh",
                2,
                &["intermediate\n", "final\n"],
            ))
            .expect("change applies");

        let doc = store.get(&uri("/tmp/a.zsh")).expect("document is open");
        assert_eq!(doc.text, "final\n");
    }

    #[test]
    fn change_stale_version_is_skipped() {
        let mut store = DocumentStore::default();
        store.open(open_params("/tmp/a.zsh", 5, "echo a\n"));

        store
            .change(change_params("/tmp/a.zsh", 4, &["stale\n"]))
            .expect("stale change is ignored without error");

        let doc = store.get(&uri("/tmp/a.zsh")).expect("document is open");
        assert_eq!(doc.text, "echo a\n");
        assert_eq!(doc.version, 5);
    }

    #[test]
    fn change_empty_events_only_bumps_version() {
        let mut store = DocumentStore::default();
        store.open(open_params("/tmp/a.zsh", 1, "echo a\n"));

        store
            .change(change_params("/tmp/a.zsh", 2, &[]))
            .expect("change applies");

        let doc = store.get(&uri("/tmp/a.zsh")).expect("document is open");
        assert_eq!(doc.text, "echo a\n");
        assert_eq!(doc.version, 2);
    }

    #[test]
    fn change_unknown_document_errors() {
        let mut store = DocumentStore::default();
        let err = store
            .change(change_params("/tmp/missing.zsh", 1, &["echo a\n"]))
            .expect_err("change on unknown document fails");
        assert_eq!(err, SyncError::UnknownDocument);
    }

    #[test]
    fn text_roundtrips_unicode_and_crlf_verbatim() {
        let mut store = DocumentStore::default();
        store.open(open_params("/tmp/a.zsh", 1, "echo héllo 🌍\n"));

        store
            .change(change_params("/tmp/a.zsh", 2, &["print \"héllo 🌍\"\r\n"]))
            .expect("change applies");

        let doc = store.get(&uri("/tmp/a.zsh")).expect("document is open");
        assert_eq!(doc.text, "print \"héllo 🌍\"\r\n");
    }
}
