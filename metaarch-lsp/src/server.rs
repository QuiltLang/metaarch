//! The tower-lsp backend: full-sync documents, push diagnostics, semantic
//! tokens. Deliberately small — all analysis lives in [`crate::analysis`] and
//! [`crate::highlight`], which are plain functions over the document text.

use std::collections::HashMap;
use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

pub struct Backend {
    client: Client,
    docs: Mutex<HashMap<Url, String>>,
}

impl Backend {
    async fn refresh(&self, uri: Url, text: String, version: i32) {
        let diags = crate::analysis::diagnostics(&text);
        self.docs.lock().unwrap().insert(uri.clone(), text);
        self.client
            .publish_diagnostics(uri, diags, Some(version))
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "metaarch-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: crate::highlight::TOKEN_TYPES.to_vec(),
                                token_modifiers: Vec::new(),
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            ..Default::default()
                        },
                    ),
                ),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {}

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        self.refresh(doc.uri, doc.text, doc.version).await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        // Full sync: the last change carries the whole document.
        let Some(change) = params.content_changes.pop() else {
            return;
        };
        self.refresh(params.text_document.uri, change.text, params.text_document.version)
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.docs.lock().unwrap().remove(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let text = self
            .docs
            .lock()
            .unwrap()
            .get(&params.text_document.uri)
            .cloned();
        Ok(text.map(|text| {
            SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data: crate::highlight::semantic_tokens(&text),
            })
        }))
    }
}

/// Serve LSP over stdio until the client disconnects.
pub async fn run_stdio() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend {
        client,
        docs: Mutex::new(HashMap::new()),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
