//! LSP backend: document sync, diagnostics, symbols, completion, hover, definition.

use crate::backend;
use crate::document::{
    analyze_source, completion_items, document_symbols, find_definition, find_hover,
    goto_location, require_target, word_at, KEYWORDS,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

pub struct BoyiaLanguageServer {
    client: Client,
    documents: Arc<RwLock<HashMap<Url, String>>>,
}

impl BoyiaLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn publish_diagnostics(&self, uri: Url) {
        let text = {
            let docs = self.documents.read().await;
            docs.get(&uri).cloned().unwrap_or_default()
        };
        let diagnostics = backend::compile_diagnostics(&text, &uri);
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    fn resolve_require_path(base_uri: &Url, rel: &str) -> Option<Url> {
        let base_path = base_uri.to_file_path().ok()?;
        let base_dir = if base_path.is_file() {
            base_path.parent()?.to_path_buf()
        } else {
            base_path
        };
        let joined = resolve_relative_path(&base_dir, rel);
        Url::from_file_path(joined).ok()
    }
}

fn resolve_relative_path(base_dir: &Path, rel: &str) -> PathBuf {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        rel_path.to_path_buf()
    } else {
        base_dir.join(rel_path)
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for BoyiaLanguageServer {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "boyia_lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_symbol_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".into(), "(".into()]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Boyia language server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        {
            let mut docs = self.documents.write().await;
            docs.insert(uri.clone(), text);
        }
        self.publish_diagnostics(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            let mut docs = self.documents.write().await;
            docs.insert(uri.clone(), change.text);
        }
        self.publish_diagnostics(uri).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.publish_diagnostics(params.text_document.uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let mut docs = self.documents.write().await;
        docs.remove(&params.text_document.uri);
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let text = {
            let docs = self.documents.read().await;
            docs.get(&params.text_document.uri)
                .cloned()
                .unwrap_or_default()
        };
        let analysis = analyze_source(&text);
        let symbols = document_symbols(&text, &analysis);
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let text = {
            let docs = self.documents.read().await;
            docs.get(&uri).cloned().unwrap_or_default()
        };

        let analysis = analyze_source(&text);
        let prefix = word_at(&text, position).unwrap_or_default();
        let items: Vec<CompletionItem> = completion_items(&analysis)
            .into_iter()
            .filter(|label| prefix.is_empty() || label.starts_with(&prefix))
            .map(|label| CompletionItem {
                label: label.clone(),
                kind: Some(if KEYWORDS.contains(&label.as_str()) {
                    CompletionItemKind::KEYWORD
                } else {
                    CompletionItemKind::VARIABLE
                }),
                ..Default::default()
            })
            .collect();

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let text = {
            let docs = self.documents.read().await;
            docs.get(&uri).cloned().unwrap_or_default()
        };

        let analysis = analyze_source(&text);
        let content = match find_hover(&text, &analysis, position) {
            Some(c) => c,
            None => {
                let word = word_at(&text, position).unwrap_or_default();
                if word.is_empty() {
                    return Ok(None);
                }
                if KEYWORDS.contains(&word.as_str()) {
                    format!("**{}** — keyword", word)
                } else {
                    format!("**{}**", word)
                }
            }
        };

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: content,
            }),
            range: None,
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let text = {
            let docs = self.documents.read().await;
            docs.get(&uri).cloned().unwrap_or_default()
        };

        let analysis = analyze_source(&text);

        if let Some((_, path)) = require_target(&text, &analysis, position) {
            if let Some(target_uri) = Self::resolve_require_path(&uri, &path) {
                return Ok(Some(GotoDefinitionResponse::Scalar(goto_location(
                    &target_uri,
                    Range {
                        start: Position {
                            line: 0,
                            character: 0,
                        },
                        end: Position {
                            line: 0,
                            character: 0,
                        },
                    },
                ))));
            }
        }

        if let Some(range) = find_definition(&text, &analysis, position) {
            return Ok(Some(GotoDefinitionResponse::Scalar(goto_location(
                &uri, range,
            ))));
        }

        Ok(None)
    }
}
