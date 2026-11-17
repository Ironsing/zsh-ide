use std::sync::{RwLock, RwLockWriteGuard};

use tower_lsp_server::LanguageServer;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentSymbolParams, DocumentSymbolResponse, FoldingRange, FoldingRangeParams,
    FoldingRangeProviderCapability, InitializeParams, InitializeResult, OneOf, ServerCapabilities,
    ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
};

use crate::analysis::AnalysisStore;
use crate::documents::DocumentStore;

/// The language server.
#[derive(Debug, Default)]
pub struct Server {
    /// Contents of every open document. They're behind a lock because `LanguageServer`
    /// implementers have to be ```Send + Sync + `static.```
    documents: RwLock<DocumentStore>,

    /// Analysis results of every open document. Behind a lock for the same reason.
    analyses: RwLock<AnalysisStore>,
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

    /// Lock the analyses, panicking only if a previous handler panicked while
    /// holding the write lock.
    fn analyses(&self) -> RwLockWriteGuard<'_, AnalysisStore> {
        self.analyses.write().expect("analysis store lock poisoned")
    }

    /// Re-runs every analysis over an open document's current tree.
    fn analyze(&self, uri: &Uri) {
        let documents = self.store();
        let Some(document) = documents.get(uri) else {
            return;
        };

        self.analyses().analyze(uri, &document.tree, &document.text);
    }
}

impl LanguageServer for Server {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
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
        let uri = params.text_document.uri.clone();
        self.store().open(params.text_document);
        self.analyze(&uri);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        if let Err(err) = self.store().change(params) {
            eprintln!("zsh-lsp: ignoring textDocument/didChange: {err}");
            return;
        }
        self.analyze(&uri);
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.store().close(&uri);
        self.analyses().close(&uri);
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let analyses = self.analyses.read().expect("analysis store lock poisoned");
        let ranges = analyses
            .folding_ranges(&params.text_document.uri)
            .map(|ranges| {
                ranges
                    .iter()
                    .map(|&(start_line, end_line)| FoldingRange {
                        start_line,
                        end_line,
                        ..Default::default()
                    })
                    .collect()
            });

        Ok(ranges)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let analyses = self.analyses.read().expect("analysis store lock poisoned");
        let symbols = analyses
            .symbols(&params.text_document.uri)
            .map(|symbols| symbols.to_vec());

        Ok(symbols.map(DocumentSymbolResponse::Nested))
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::ls_types::{
        PartialResultParams, SymbolKind, TextDocumentIdentifier, TextDocumentItem, Uri,
        WorkDoneProgressParams,
    };

    fn uri(path: &str) -> Uri {
        format!("file://{path}").parse().expect("valid uri")
    }

    fn symbol_params(path: &str) -> DocumentSymbolParams {
        DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri: uri(path) },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        }
    }

    fn folding_params(path: &str) -> FoldingRangeParams {
        FoldingRangeParams {
            text_document: TextDocumentIdentifier { uri: uri(path) },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        }
    }

    fn open(server: &Server, path: &str, text: &str) {
        server.store().open(TextDocumentItem {
            uri: uri(path),
            language_id: "zsh".to_string(),
            version: 1,
            text: text.to_string(),
        });
        server.analyze(&uri(path));
    }

    #[tokio::test]
    async fn initialize_advertises_document_symbols() {
        let result = Server::new()
            .initialize(InitializeParams::default())
            .await
            .expect("initialize succeeds");

        assert_eq!(
            result.capabilities.document_symbol_provider,
            Some(OneOf::Left(true))
        );
    }

    #[tokio::test]
    async fn document_symbol_returns_source_and_alias_symbols() {
        let server = Server::new();
        open(&server, "/tmp/a.zsh", "alias l=lazygit\nsource ~/.zshrc\n");

        let response = server
            .document_symbol(symbol_params("/tmp/a.zsh"))
            .await
            .expect("request succeeds");

        let Some(DocumentSymbolResponse::Nested(symbols)) = response else {
            panic!("expected a nested symbol response");
        };

        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["l", "~/.zshrc"]);
        assert_eq!(symbols[0].kind, SymbolKind::CONSTANT);
        assert_eq!(symbols[1].kind, SymbolKind::MODULE);
    }

    #[tokio::test]
    async fn document_symbol_unknown_document_returns_none() {
        let response = Server::new()
            .document_symbol(symbol_params("/tmp/missing.zsh"))
            .await
            .expect("request succeeds");

        assert!(response.is_none());
    }

    #[tokio::test]
    async fn initialize_advertises_folding_ranges() {
        let result = Server::new()
            .initialize(InitializeParams::default())
            .await
            .expect("initialize succeeds");

        assert_eq!(
            result.capabilities.folding_range_provider,
            Some(FoldingRangeProviderCapability::Simple(true))
        );
    }

    #[tokio::test]
    async fn folding_range_returns_the_foldable_regions() {
        let server = Server::new();
        open(
            &server,
            "/tmp/a.zsh",
            "foo() {\n  echo hi\n  echo bye\n}\nif true; then\n  echo cond\nfi\n",
        );

        let ranges = server
            .folding_range(folding_params("/tmp/a.zsh"))
            .await
            .expect("request succeeds")
            .expect("ranges are returned");

        assert_eq!(
            ranges,
            [
                // The function body, then the `if` block, in source order.
                FoldingRange {
                    start_line: 0,
                    end_line: 3,
                    ..Default::default()
                },
                FoldingRange {
                    start_line: 4,
                    end_line: 6,
                    ..Default::default()
                },
            ]
        );
    }
}
