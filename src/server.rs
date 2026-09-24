use std::collections::HashMap;

use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer};

use crate::analysis;

struct Document {
    text: String,
    version: i32,
}

pub struct NiaServer {
    client: Client,
    documents: Mutex<HashMap<Uri, Document>>,
}

impl NiaServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Mutex::new(HashMap::new()),
        }
    }

    async fn publish_diagnostics(&self, uri: Uri) {
        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&uri) else {
            return;
        };
        let diagnostics = analysis::diagnostics(&document.text);
        let version = document.version;
        drop(documents);

        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }
}

impl LanguageServer for NiaServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                position_encoding: Some(PositionEncodingKind::UTF16),
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "nia-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            ..Default::default()
        })
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        let uri = document.uri;
        self.documents.lock().await.insert(
            uri.clone(),
            Document {
                text: document.text,
                version: document.version,
            },
        );
        self.publish_diagnostics(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        // Full synchronization requires range-less replacement texts.
        if params
            .content_changes
            .iter()
            .any(|change| change.range.is_some())
        {
            return;
        }
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };
        let mut documents = self.documents.lock().await;
        let Some(document) = documents.get_mut(&uri) else {
            return;
        };
        if params.text_document.version <= document.version {
            return;
        }
        document.text = change.text;
        document.version = params.text_document.version;
        drop(documents);

        self.publish_diagnostics(uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.lock().await.remove(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }
}
