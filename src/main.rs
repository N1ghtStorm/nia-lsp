mod analysis;
mod server;

use tower_lsp_server::{LspService, Server};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let (service, socket) = LspService::new(server::NiaServer::new);

    // stdout is reserved for LSP messages. Use stderr for any future logging.
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        // Keep document changes and diagnostic publications in arrival order.
        .concurrency_level(1)
        .serve(service)
        .await;
}
