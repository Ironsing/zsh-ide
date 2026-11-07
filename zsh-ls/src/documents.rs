//! Handles document synchronization.

use std::collections::HashMap;
use std::fmt;

use tower_lsp_server::ls_types::{DidChangeTextDocumentParams, TextDocumentItem, Uri};

/// A single open document.
#[derive(Debug, Clone)]
pub struct Document {
    /// Full text.
    pub text: String,
    /// The latest version so far, obtained from the LSP client.
    pub version: i32,
}

/// A map of all open documents, keyed by location.
#[derive(Debug, Default)]
pub struct DocumentStore {
    documents: HashMap<Uri, Document>,
}

/// Errors that can occur while applying a document change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncError {
    /// A change was received for a document that is not currently open.
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

impl DocumentStore {
    /// Handle `textDocument/didOpen`.
    pub fn open(&mut self, item: TextDocumentItem) -> Option<Document> {
        let previous = self.documents.insert(
            item.uri,
            Document {
                text: item.text,
                version: item.version,
            },
        );
        self.trace("didOpen");
        previous
    }

    /// Handle `textDocument/didClose`.
    pub fn close(&mut self, uri: &Uri) -> Option<Document> {
        let removed = self.documents.remove(uri);
        if removed.is_some() {
            self.trace("didClose");
        }
        removed
    }

    /// Handle `textDocument/didChange`.
    /// A change carrying a version older than the one on record is ignored.
    /// An empty `content_changes` list only bumps the version.
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
        }

        doc.version = params.text_document.version;
        self.trace("didChange");
        Ok(())
    }

    /// Get the document for a URI, if open.
    pub fn get(&self, uri: &Uri) -> Option<&Document> {
        self.documents.get(uri)
    }

    /// Number of open documents.
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Whether no documents are open.
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// Dump the whole store to stderr.
    ///
    /// The editor forwards the server's stderr to an output channel, so this is
    /// how document synchronization can be watched from the UI.
    fn trace(&self, event: &str) {
        eprintln!("zsh-lsp: {event}\n{}", self);
    }
}

impl fmt::Display for DocumentStore {
    /// Pretty-print every open document, sorted by URI so that the output is
    /// stable from one call to the next.
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
