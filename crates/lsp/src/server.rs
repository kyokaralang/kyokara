//! Tower-LSP backend: lifecycle, text sync, and handler dispatch.

use std::collections::HashMap;
use std::sync::Arc;

use salsa::Setter;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::db::{self, FileAnalysis, LspDatabase, SourceFile};
use crate::position;

/// The Kyokara language server state.
///
/// The salsa database is wrapped in a `std::sync::Mutex` (not tokio's)
/// because salsa's `Storage` is `!Sync`. All other state uses tokio's
/// `RwLock` for async-friendly sharing.
pub struct KyokaraLanguageServer {
    /// LSP client handle for publishing diagnostics, etc.
    client: Client,
    /// Salsa database for incremental tracking.
    db: std::sync::Mutex<LspDatabase>,
    /// Map from URI to salsa input handle.
    files: std::sync::Mutex<HashMap<Url, SourceFile>>,
    /// Map from URI to latest source text.
    sources: RwLock<HashMap<Url, String>>,
    /// Map from URI to cached analysis result.
    analyses: RwLock<HashMap<Url, Arc<FileAnalysis>>>,
}

impl KyokaraLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            db: std::sync::Mutex::new(LspDatabase::default()),
            files: std::sync::Mutex::new(HashMap::new()),
            sources: RwLock::new(HashMap::new()),
            analyses: RwLock::new(HashMap::new()),
        }
    }

    /// Update a file's source, recompute analysis, and publish diagnostics.
    async fn on_change(&self, uri: Url, text: String) {
        // Check if salsa detects a change (text equality).
        let needs_recompute = {
            let mut db = self.db.lock().unwrap();
            let mut files = self.files.lock().unwrap();

            if let Some(existing) = files.get(&uri) {
                let old_text = existing.text(&*db);
                if old_text == text {
                    false
                } else {
                    existing.set_text(&mut *db).to(text.clone());
                    // Poke the tracked query to register the change.
                    let _ = db::check_file_query(&*db, *existing);
                    true
                }
            } else {
                let file = SourceFile::new(&*db, text.clone());
                files.insert(uri.clone(), file);
                let _ = db::check_file_query(&*db, file);
                true
            }
        };

        if needs_recompute {
            let analysis = Arc::new(FileAnalysis::from_check_result(
                kyokara_hir::check_file(&text),
                text.clone(),
            ));

            self.sources.write().await.insert(uri.clone(), text);
            self.analyses
                .write()
                .await
                .insert(uri.clone(), analysis.clone());

            // Publish diagnostics.
            let diags = crate::diagnostics::to_lsp_diagnostics(&analysis, &analysis.source);
            self.client.publish_diagnostics(uri, diags, None).await;
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for KyokaraLanguageServer {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".into(), ":".into()]),
                    ..Default::default()
                }),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "kyokara-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "kyokara-lsp initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.on_change(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // FULL sync: the last content change has the full text.
        if let Some(change) = params.content_changes.into_iter().last() {
            self.on_change(params.text_document.uri, change.text).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        if let Some(text) = params.text {
            self.on_change(params.text_document.uri, text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.sources.write().await.remove(&uri);
        self.analyses.write().await.remove(&uri);
        // Clear diagnostics for the closed file.
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let sources = self.sources.read().await;
        let analyses = self.analyses.read().await;

        let Some(source) = sources.get(&uri) else {
            return Ok(None);
        };
        let Some(analysis) = analyses.get(&uri) else {
            return Ok(None);
        };
        let Some(offset) = position::lsp_position_to_offset(pos, source) else {
            return Ok(None);
        };

        Ok(crate::hover::hover(analysis, source, offset))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let sources = self.sources.read().await;
        let analyses = self.analyses.read().await;

        let Some(source) = sources.get(&uri) else {
            return Ok(None);
        };
        let Some(analysis) = analyses.get(&uri) else {
            return Ok(None);
        };
        let Some(offset) = position::lsp_position_to_offset(pos, source) else {
            return Ok(None);
        };

        Ok(
            crate::goto_def::goto_definition(analysis, source, offset, &uri)
                .map(GotoDefinitionResponse::Scalar),
        )
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let sources = self.sources.read().await;
        let analyses = self.analyses.read().await;

        let Some(source) = sources.get(&uri) else {
            return Ok(None);
        };
        let Some(analysis) = analyses.get(&uri) else {
            return Ok(None);
        };
        let Some(offset) = position::lsp_position_to_offset(pos, source) else {
            return Ok(None);
        };

        let refs = crate::references::find_references(analysis, source, offset, &uri);
        if refs.is_empty() {
            Ok(None)
        } else {
            Ok(Some(refs))
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let sources = self.sources.read().await;
        let analyses = self.analyses.read().await;

        let Some(source) = sources.get(&uri) else {
            return Ok(None);
        };
        let Some(analysis) = analyses.get(&uri) else {
            return Ok(None);
        };
        let Some(offset) = position::lsp_position_to_offset(pos, source) else {
            return Ok(None);
        };

        Ok(crate::completion::completions(analysis, source, offset))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;

        let sources = self.sources.read().await;
        let analyses = self.analyses.read().await;

        let Some(source) = sources.get(&uri) else {
            return Ok(None);
        };
        let Some(analysis) = analyses.get(&uri) else {
            return Ok(None);
        };

        let actions = crate::code_action::code_actions(
            analysis,
            source,
            params.range,
            &uri,
            &params.context.diagnostics,
        );
        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let sources = self.sources.read().await;

        let Some(source) = sources.get(&uri) else {
            return Ok(None);
        };

        let edits = crate::format::format_document(source);
        if edits.is_empty() {
            Ok(None)
        } else {
            Ok(Some(edits))
        }
    }
}
