use std::sync::{RwLock, RwLockWriteGuard};

use tower_lsp_server::LanguageServer;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    InitializeParams, InitializeResult, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind,
};

use crate::documents::DocumentStore;

/// The language server.
#[derive(Debug, Default)]
pub struct Server {
    /// Contents of every open document. They're behind a lock because `LanguageServer`
    /// implementers have to be ```Send + Sync + `static.```
    documents: RwLock<DocumentStore>,
}

impl Server {
    /// Create a server with no open documents.
    pub fn new() -> Self {
        Self::default()
    }

    /// Lock the document store, panicking only if a previous handler panicked
    /// while holding the write lock.
    fn store(&self) -> RwLockWriteGuard<'_, DocumentStore> {
        self.documents
            .write()
            .expect("document store lock poisoned")
    }
}

impl LanguageServer for Server {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: env!("CARGO_PKG_NAME").to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            ..Default::default()
        })
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.store().open(params.text_document);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Err(err) = self.store().change(params) {
            eprintln!("zsh-lsp: ignoring textDocument/didChange: {err}");
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.store().close(&params.text_document.uri);
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}
