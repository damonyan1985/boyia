//! Boyia Language Server — stdio LSP for `.boyia` files.

mod backend;
mod document;
mod server;
mod syntax;

use server::BoyiaLanguageServer;
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| BoyiaLanguageServer::new(client));
    Server::new(stdin, stdout, socket).serve(service).await;
}
