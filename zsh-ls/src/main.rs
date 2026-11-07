mod documents;
mod server;

use tower_lsp_server::{LspService, Server as LspServer};

use crate::server::Server;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|_client| Server::new());
    LspServer::new(stdin, stdout, socket).serve(service).await;
}
