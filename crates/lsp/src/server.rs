//! Tower-LSP backend: lifecycle, text sync, and handler dispatch.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use salsa::Setter;
use tokio::sync::{Mutex as AsyncMutex, RwLock};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::db::{self, FileAnalysis, LspDatabase, SourceFile};
use crate::position;

/// The Kyokara language server state.
///
/// The salsa database is wrapped in a `parking_lot::Mutex` (not tokio's)
/// because salsa's `Storage` is `!Sync`. All other state uses tokio's
/// `RwLock` for async-friendly sharing.
pub struct KyokaraLanguageServer {
    /// LSP client handle for publishing diagnostics, etc.
    client: Client,
    /// Salsa database for incremental tracking.
    db: Mutex<LspDatabase>,
    /// Map from URI to salsa input handle.
    files: Mutex<HashMap<Url, SourceFile>>,
    /// Map from URI to the latest atomic document snapshot.
    ///
    /// `FileAnalysis` already carries the source text used to build the analysis,
    /// so storing one snapshot avoids reader-visible generation skew.
    documents: RwLock<HashMap<Url, Arc<FileAnalysis>>>,
    /// Serialize on_change recomputations to avoid stale overwrite races.
    on_change_gate: AsyncMutex<()>,
    #[cfg(test)]
    /// Test-only hook to force deterministic interleaving in concurrent on_change tests.
    test_on_change_delay_yields: Mutex<HashMap<String, u32>>,
    #[cfg(test)]
    /// Test-only hook to pause after recompute but before publishing a new snapshot.
    test_pre_publish_delay_yields: Mutex<HashMap<String, u32>>,
}

impl KyokaraLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            db: Mutex::new(LspDatabase::default()),
            files: Mutex::new(HashMap::new()),
            documents: RwLock::new(HashMap::new()),
            on_change_gate: AsyncMutex::new(()),
            #[cfg(test)]
            test_on_change_delay_yields: Mutex::new(HashMap::new()),
            #[cfg(test)]
            test_pre_publish_delay_yields: Mutex::new(HashMap::new()),
        }
    }

    #[cfg(test)]
    fn set_test_on_change_delay_yields(&self, text: &str, delay_yields: u32) {
        self.test_on_change_delay_yields
            .lock()
            .insert(text.to_string(), delay_yields);
    }

    #[cfg(test)]
    fn set_test_pre_publish_delay_yields(&self, text: &str, delay_yields: u32) {
        self.test_pre_publish_delay_yields
            .lock()
            .insert(text.to_string(), delay_yields);
    }

    #[cfg(test)]
    async fn maybe_delay_for_text(&self, text: &str) {
        let delay_yields = self.test_on_change_delay_yields.lock().get(text).copied();
        if let Some(ticks) = delay_yields {
            for _ in 0..ticks {
                tokio::task::yield_now().await;
            }
        }
    }

    #[cfg(test)]
    async fn maybe_delay_before_publish(&self, text: &str) {
        let delay_yields = self.test_pre_publish_delay_yields.lock().get(text).copied();
        if let Some(ticks) = delay_yields {
            for _ in 0..ticks {
                tokio::task::yield_now().await;
            }
        }
    }

    /// Update a file's source, recompute analysis, and publish diagnostics.
    async fn on_change(&self, uri: Url, text: String) {
        let _gate = self.on_change_gate.lock().await;

        // Check if salsa detects a change (text equality).
        let needs_recompute = {
            let mut db = self.db.lock();
            let mut files = self.files.lock();

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
            #[cfg(test)]
            self.maybe_delay_for_text(&text).await;

            let analysis = Arc::new(build_file_analysis(&uri, &text));

            #[cfg(test)]
            self.maybe_delay_before_publish(&text).await;

            self.documents
                .write()
                .await
                .insert(uri.clone(), analysis.clone());

            // Publish diagnostics.
            let diags = crate::diagnostics::to_lsp_diagnostics(&analysis, &analysis.source);
            self.client.publish_diagnostics(uri, diags, None).await;
        }
    }

    #[cfg(test)]
    async fn document_snapshot(&self, uri: &Url) -> Option<Arc<FileAnalysis>> {
        self.documents.read().await.get(uri).cloned()
    }
}

fn build_file_analysis(uri: &Url, text: &str) -> FileAnalysis {
    if let Ok(path) = uri.to_file_path()
        && let Some(entry_file) = kyokara_hir::package_entry_file_for_source(&path)
        && let Some(analysis) = build_package_file_analysis(&entry_file, &path, text)
    {
        return analysis;
    }

    FileAnalysis::from_check_result(kyokara_hir::check_file(text), text.to_string())
}

fn build_package_file_analysis(
    entry_file: &Path,
    current_file: &Path,
    current_text: &str,
) -> Option<FileAnalysis> {
    let plan = kyokara_hir::discover_project_load_plan(entry_file);
    let copy_root = project_copy_root(&plan, entry_file);
    let temp_dir = tempfile::tempdir().ok()?;

    for package in &plan.packages {
        let Some(package_root) = package.package_root.as_deref() else {
            continue;
        };
        let manifest_path = package_root.join("kyokara.toml");
        if !manifest_path.is_file() {
            continue;
        }
        let dest = temp_dir
            .path()
            .join(project_relative_path(&copy_root, &manifest_path));
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).ok()?;
        }
        let manifest_source = std::fs::read_to_string(&manifest_path).ok()?;
        std::fs::write(dest, manifest_source).ok()?;
    }

    for (_module_path, file_path) in plan.iter_modules() {
        let dest = temp_dir
            .path()
            .join(project_relative_path(&copy_root, file_path));
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).ok()?;
        }
        let source = if file_path == current_file {
            current_text.to_string()
        } else {
            std::fs::read_to_string(file_path).ok()?
        };
        std::fs::write(dest, source).ok()?;
    }

    let temp_entry = temp_dir
        .path()
        .join(project_relative_path(&copy_root, entry_file));
    let temp_current = temp_dir
        .path()
        .join(project_relative_path(&copy_root, current_file));
    let result = kyokara_hir::check_project(&temp_entry);
    FileAnalysis::from_project_check_result_for_path(result, &temp_current)
}

fn project_copy_root(plan: &kyokara_hir::ProjectLoadPlan, entry_file: &Path) -> PathBuf {
    let package_roots: Vec<&Path> = plan
        .packages
        .iter()
        .filter_map(|package| package.package_root.as_deref())
        .collect();
    if package_roots.is_empty() {
        return entry_file.parent().unwrap_or(Path::new(".")).to_path_buf();
    }

    common_ancestor(&package_roots)
        .unwrap_or_else(|| entry_file.parent().unwrap_or(Path::new(".")).to_path_buf())
}

fn project_relative_path<'a>(root: &Path, path: &'a Path) -> &'a Path {
    path.strip_prefix(root)
        .unwrap_or_else(|_| path.file_name().map(Path::new).unwrap_or(path))
}

fn common_ancestor(paths: &[&Path]) -> Option<PathBuf> {
    let mut components: Vec<_> = paths.first()?.components().collect();
    for path in &paths[1..] {
        let current: Vec<_> = path.components().collect();
        let shared_len = components
            .iter()
            .zip(current.iter())
            .take_while(|(lhs, rhs)| lhs == rhs)
            .count();
        components.truncate(shared_len);
        if components.is_empty() {
            break;
        }
    }

    let mut ancestor = PathBuf::new();
    for component in components {
        ancestor.push(component.as_os_str());
    }
    Some(ancestor)
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
        self.files.lock().remove(&uri);
        self.documents.write().await.remove(&uri);
        // Clear diagnostics for the closed file.
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let documents = self.documents.read().await;
        let Some(analysis) = documents.get(&uri) else {
            return Ok(None);
        };
        let source = analysis.source.as_str();
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

        let documents = self.documents.read().await;
        let Some(analysis) = documents.get(&uri) else {
            return Ok(None);
        };
        let source = analysis.source.as_str();
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
        let include_declaration = params.context.include_declaration;

        let documents = self.documents.read().await;
        let Some(analysis) = documents.get(&uri) else {
            return Ok(None);
        };
        let source = analysis.source.as_str();
        let Some(offset) = position::lsp_position_to_offset(pos, source) else {
            return Ok(None);
        };

        let refs = crate::references::find_references_with_options(
            analysis,
            source,
            offset,
            &uri,
            include_declaration,
        );
        if refs.is_empty() {
            Ok(None)
        } else {
            Ok(Some(refs))
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let documents = self.documents.read().await;
        let Some(analysis) = documents.get(&uri) else {
            return Ok(None);
        };
        let source = analysis.source.as_str();
        let Some(offset) = position::lsp_position_to_offset(pos, source) else {
            return Ok(None);
        };

        Ok(crate::completion::completions(analysis, source, offset))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;

        let documents = self.documents.read().await;
        let Some(analysis) = documents.get(&uri) else {
            return Ok(None);
        };
        let source = analysis.source.as_str();

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
        let documents = self.documents.read().await;
        let Some(analysis) = documents.get(&uri) else {
            return Ok(None);
        };
        let source = analysis.source.as_str();

        let edits = crate::format::format_document(source);
        if edits.is_empty() {
            Ok(None)
        } else {
            Ok(Some(edits))
        }
    }
}
#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use serde_json::{Value, json};
    use tower::{Service, ServiceExt};
    use tower_lsp::LspService;
    use tower_lsp::jsonrpc::{Request, Response};
    use tower_lsp::lsp_types::Url;

    use super::KyokaraLanguageServer;

    fn initialize_request(id: i64) -> Request {
        Request::build("initialize")
            .params(json!({ "capabilities": {} }))
            .id(id)
            .finish()
    }

    async fn initialize(service: &mut LspService<KyokaraLanguageServer>) {
        let resp = service
            .ready()
            .await
            .expect("service ready")
            .call(initialize_request(1))
            .await
            .expect("initialize call should succeed")
            .expect("initialize must return response");
        assert!(resp.is_ok(), "initialize failed: {resp:?}");
    }

    async fn call_notification(
        service: &mut LspService<KyokaraLanguageServer>,
        method: &'static str,
        params: Value,
    ) {
        let resp = service
            .ready()
            .await
            .expect("service ready")
            .call(Request::build(method).params(params).finish())
            .await
            .expect("notification call should succeed");
        assert!(resp.is_none(), "notification should have no response");
    }

    async fn call_request(
        service: &mut LspService<KyokaraLanguageServer>,
        method: &'static str,
        id: i64,
        params: Value,
    ) -> Response {
        service
            .ready()
            .await
            .expect("service ready")
            .call(Request::build(method).params(params).id(id).finish())
            .await
            .expect("request call should succeed")
            .expect("request should produce a response")
    }

    fn reference_start_positions(resp: &Response) -> Vec<(u64, u64)> {
        let Some(result) = resp.result() else {
            return Vec::new();
        };
        let Some(items) = result.as_array() else {
            return Vec::new();
        };

        items
            .iter()
            .filter_map(|loc| {
                let start = loc.get("range")?.get("start")?;
                Some((
                    start.get("line")?.as_u64()?,
                    start.get("character")?.as_u64()?,
                ))
            })
            .collect()
    }

    fn hover_markup(resp: &Response) -> Option<String> {
        let result = resp.result()?;
        if result.is_null() {
            return None;
        }
        result
            .get("contents")
            .and_then(|v| v.get("value"))
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    fn completion_labels(resp: &Response) -> Vec<String> {
        let Some(result) = resp.result() else {
            return Vec::new();
        };
        if result.is_null() {
            return Vec::new();
        }

        result
            .get("items")
            .and_then(Value::as_array)
            .or_else(|| result.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.get("label").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lifecycle_reopen_same_text_preserves_language_features() {
        let (mut service, mut socket) = LspService::new(KyokaraLanguageServer::new);

        let drain = tokio::spawn(async move { while socket.next().await.is_some() {} });

        initialize(&mut service).await;

        let uri = "file:///test.ky";
        let source = "fn foo() -> Int { 42 }\nfn bar() -> Int { foo() }\n";

        call_notification(
            &mut service,
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "kyokara",
                    "version": 1,
                    "text": source
                }
            }),
        )
        .await;

        // Baseline: features should work after initial open.
        let hover_before = call_request(
            &mut service,
            "textDocument/hover",
            2,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 21 }
            }),
        )
        .await;
        assert!(hover_before.is_ok());
        assert!(
            hover_before.result().is_some_and(|r| !r.is_null()),
            "hover should not be null after initial open: {hover_before:?}"
        );

        call_notification(
            &mut service,
            "textDocument/didClose",
            json!({ "textDocument": { "uri": uri } }),
        )
        .await;

        // Reopen with unchanged text. Regressions here can leave analyses cache empty.
        call_notification(
            &mut service,
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "kyokara",
                    "version": 2,
                    "text": source
                }
            }),
        )
        .await;

        let hover_after = call_request(
            &mut service,
            "textDocument/hover",
            3,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 21 }
            }),
        )
        .await;
        assert!(hover_after.is_ok());
        assert!(
            hover_after.result().is_some_and(|r| !r.is_null()),
            "hover should work after close/reopen unchanged text: {hover_after:?}"
        );

        let def_after = call_request(
            &mut service,
            "textDocument/definition",
            4,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 21 }
            }),
        )
        .await;
        assert!(def_after.is_ok());
        assert!(
            def_after.result().is_some_and(|r| !r.is_null()),
            "goto-definition should work after close/reopen unchanged text: {def_after:?}"
        );

        let completion_after = call_request(
            &mut service,
            "textDocument/completion",
            5,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 0 }
            }),
        )
        .await;
        assert!(completion_after.is_ok());
        let completion_result = completion_after
            .result()
            .expect("completion response should have result");
        assert!(
            !completion_result.is_null(),
            "completion result must not be null"
        );

        let labels = completion_result
            .get("items")
            .and_then(Value::as_array)
            .or_else(|| completion_result.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.get("label").and_then(Value::as_str))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        assert!(
            labels.contains(&"foo"),
            "expected completion to include `foo`, got labels: {labels:?}"
        );

        drain.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lifecycle_close_clears_document_state() {
        let (mut service, mut socket) = LspService::new(KyokaraLanguageServer::new);

        let drain = tokio::spawn(async move { while socket.next().await.is_some() {} });

        initialize(&mut service).await;

        let uri = "file:///test.ky";
        let source = "fn foo() -> Int { 42 }\nfn bar() -> Int { foo() }\n";

        call_notification(
            &mut service,
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "kyokara",
                    "version": 1,
                    "text": source
                }
            }),
        )
        .await;

        let hover_before = call_request(
            &mut service,
            "textDocument/hover",
            10,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 21 }
            }),
        )
        .await;
        assert!(hover_before.is_ok(), "hover should succeed before close");
        assert!(
            hover_before.result().is_some_and(|r| !r.is_null()),
            "hover should be present before close: {hover_before:?}"
        );

        call_notification(
            &mut service,
            "textDocument/didClose",
            json!({ "textDocument": { "uri": uri } }),
        )
        .await;

        let hover_after_close = call_request(
            &mut service,
            "textDocument/hover",
            11,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 21 }
            }),
        )
        .await;
        assert!(hover_after_close.is_ok(), "hover request should not error");
        assert!(
            hover_after_close.result().is_some_and(Value::is_null),
            "closed document should return null hover: {hover_after_close:?}"
        );

        let def_after_close = call_request(
            &mut service,
            "textDocument/definition",
            12,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 21 }
            }),
        )
        .await;
        assert!(
            def_after_close.result().is_some_and(Value::is_null),
            "closed document should return null definition: {def_after_close:?}"
        );

        let completion_after_close = call_request(
            &mut service,
            "textDocument/completion",
            13,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 0 }
            }),
        )
        .await;
        assert!(
            completion_after_close.result().is_some_and(Value::is_null),
            "closed document should return null completion: {completion_after_close:?}"
        );

        drain.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn references_respect_include_declaration_flag() {
        let (mut service, mut socket) = LspService::new(KyokaraLanguageServer::new);
        let drain = tokio::spawn(async move { while socket.next().await.is_some() {} });

        initialize(&mut service).await;

        let uri = "file:///test.ky";
        let source = "fn foo() -> Int { 42 }\nfn bar() -> Int { foo() }\n";

        call_notification(
            &mut service,
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "kyokara",
                    "version": 1,
                    "text": source
                }
            }),
        )
        .await;

        let refs_with_decl = call_request(
            &mut service,
            "textDocument/references",
            20,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 21 },
                "context": { "includeDeclaration": true }
            }),
        )
        .await;
        assert!(
            refs_with_decl.is_ok(),
            "references request should succeed: {refs_with_decl:?}"
        );
        let with_decl_positions = reference_start_positions(&refs_with_decl);
        assert_eq!(
            with_decl_positions.len(),
            2,
            "expected definition + call when includeDeclaration=true, got: {with_decl_positions:?}"
        );
        assert!(
            with_decl_positions.contains(&(0, 3)),
            "definition position should be included, got: {with_decl_positions:?}"
        );

        let refs_without_decl = call_request(
            &mut service,
            "textDocument/references",
            21,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 21 },
                "context": { "includeDeclaration": false }
            }),
        )
        .await;
        assert!(
            refs_without_decl.is_ok(),
            "references request should succeed: {refs_without_decl:?}"
        );
        let without_decl_positions = reference_start_positions(&refs_without_decl);
        assert_eq!(
            without_decl_positions.len(),
            1,
            "expected only usage when includeDeclaration=false, got: {without_decl_positions:?}"
        );
        assert!(
            !without_decl_positions.contains(&(0, 3)),
            "definition position should be excluded, got: {without_decl_positions:?}"
        );

        drain.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lifecycle_did_change_recomputes_analysis() {
        let (mut service, mut socket) = LspService::new(KyokaraLanguageServer::new);
        let drain = tokio::spawn(async move { while socket.next().await.is_some() {} });

        initialize(&mut service).await;

        let uri = "file:///test.ky";
        let source_v1 = "fn foo() -> Int { 1 }\nfn main() -> Int { foo() }\n";
        let source_v2 = "fn baz() -> Int { 2 }\nfn main() -> Int { baz() }\n";

        call_notification(
            &mut service,
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "kyokara",
                    "version": 1,
                    "text": source_v1
                }
            }),
        )
        .await;

        let hover_before = call_request(
            &mut service,
            "textDocument/hover",
            30,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 19 }
            }),
        )
        .await;
        assert!(hover_before.is_ok(), "hover should succeed before change");
        let before_text = hover_markup(&hover_before).expect("expected hover before change");
        assert!(
            before_text.contains("fn foo"),
            "expected hover for foo before change, got: {before_text}"
        );

        call_notification(
            &mut service,
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": 2 },
                "contentChanges": [{ "text": source_v2 }]
            }),
        )
        .await;

        let hover_after = call_request(
            &mut service,
            "textDocument/hover",
            31,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 19 }
            }),
        )
        .await;
        assert!(hover_after.is_ok(), "hover should succeed after change");
        let after_text = hover_markup(&hover_after).expect("expected hover after change");
        assert!(
            after_text.contains("fn baz"),
            "expected hover for baz after change, got: {after_text}"
        );

        drain.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lifecycle_did_save_with_text_recomputes_analysis() {
        let (mut service, mut socket) = LspService::new(KyokaraLanguageServer::new);
        let drain = tokio::spawn(async move { while socket.next().await.is_some() {} });

        initialize(&mut service).await;

        let uri = "file:///test.ky";
        let source_v1 = "fn foo() -> Int { 1 }\n";
        let source_v2 = "fn foo() -> Int { 1 }\nfn baz() -> Int { 2 }\n";

        call_notification(
            &mut service,
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "kyokara",
                    "version": 1,
                    "text": source_v1
                }
            }),
        )
        .await;

        let completion_before = call_request(
            &mut service,
            "textDocument/completion",
            40,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 0 }
            }),
        )
        .await;
        assert!(
            completion_before.is_ok(),
            "completion should succeed before save"
        );
        let labels_before = completion_labels(&completion_before);
        assert!(
            !labels_before.iter().any(|l| l == "baz"),
            "baz should not exist before save, got labels: {labels_before:?}"
        );

        call_notification(
            &mut service,
            "textDocument/didSave",
            json!({
                "textDocument": { "uri": uri },
                "text": source_v2
            }),
        )
        .await;

        let completion_after = call_request(
            &mut service,
            "textDocument/completion",
            41,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 0 }
            }),
        )
        .await;
        assert!(
            completion_after.is_ok(),
            "completion should succeed after save"
        );
        let labels_after = completion_labels(&completion_after);
        assert!(
            labels_after.iter().any(|l| l == "baz"),
            "baz should appear after save with new text, got labels: {labels_after:?}"
        );

        drain.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_on_change_keeps_latest_analysis() {
        let (mut service, mut socket) = LspService::new(KyokaraLanguageServer::new);
        let drain = tokio::spawn(async move { while socket.next().await.is_some() {} });

        initialize(&mut service).await;

        let uri = Url::parse("file:///race.ky").expect("valid uri");
        let slow_source = "fn slow() -> Int { 1 }\nfn main() -> Int { slow() }\n";
        let fast_source = "fn fast() -> Int { 2 }\nfn main() -> Int { fast() }\n";

        service
            .inner()
            .set_test_on_change_delay_yields(slow_source, 256_u32);

        let slow_fut = service
            .inner()
            .on_change(uri.clone(), slow_source.to_string());
        let fast_fut = async {
            for _ in 0..4 {
                tokio::task::yield_now().await;
            }
            service
                .inner()
                .on_change(uri.clone(), fast_source.to_string())
                .await;
        };
        tokio::join!(slow_fut, fast_fut);

        let documents = service.inner().documents.read().await;
        let analysis = documents
            .get(&uri)
            .expect("analysis should exist after on_change");
        assert_eq!(
            analysis.source, fast_source,
            "latest analysis should not be overwritten by stale recompute"
        );

        drain.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn readers_observe_atomic_document_snapshots_during_on_change() {
        let (mut service, mut socket) = LspService::new(KyokaraLanguageServer::new);
        let drain = tokio::spawn(async move { while socket.next().await.is_some() {} });

        initialize(&mut service).await;

        let uri = Url::parse("file:///atomic.ky").expect("valid uri");
        let source_v1 = "fn foo() -> Int { 1 }\nfn main() -> Int { foo() }\n";
        let source_v2 = "fn baz() -> Int { 2 }\nfn main() -> Int { baz() }\n";

        service
            .inner()
            .on_change(uri.clone(), source_v1.to_string())
            .await;
        service
            .inner()
            .set_test_pre_publish_delay_yields(source_v2, 256_u32);

        let server = service.inner();
        let change_fut = server.on_change(uri.clone(), source_v2.to_string());
        let snapshot_during_fut = async {
            for _ in 0..4 {
                tokio::task::yield_now().await;
            }
            server
                .document_snapshot(&uri)
                .await
                .expect("expected document snapshot during change")
        };
        let (_change_result, snapshot_during) = tokio::join!(change_fut, snapshot_during_fut);

        assert!(
            snapshot_during.source.contains("fn foo"),
            "reader should observe old consistent snapshot before publish, got: {}",
            snapshot_during.source
        );

        let snapshot_after = server
            .document_snapshot(&uri)
            .await
            .expect("expected document snapshot after publish");
        assert!(
            snapshot_after.source.contains("fn baz"),
            "reader should observe new snapshot after publish, got: {}",
            snapshot_after.source
        );

        drain.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn package_file_open_uses_project_analysis_for_dependency_imports() {
        let (mut service, mut socket) = LspService::new(KyokaraLanguageServer::new);
        let drain = tokio::spawn(async move { while socket.next().await.is_some() {} });

        initialize(&mut service).await;

        let temp_root = std::env::temp_dir().join(format!(
            "kyokara-lsp-phase3-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let source = "import deps.json\nfn main() -> Int { json.parse(\"abc\") }\n";

        let app_manifest = temp_root.join("app").join("kyokara.toml");
        let app_main = temp_root.join("app").join("src").join("main.ky");
        let json_manifest = temp_root.join("json").join("kyokara.toml");
        let json_lib = temp_root.join("json").join("src").join("lib.ky");

        std::fs::create_dir_all(app_main.parent().expect("app src dir")).expect("create app src");
        std::fs::create_dir_all(json_lib.parent().expect("json src dir")).expect("create json src");
        std::fs::write(
            &app_manifest,
            "[package]\nname = \"app\"\nedition = \"2026\"\nkind = \"bin\"\n\n[dependencies]\njson = { path = \"../json\" }\n",
        )
        .expect("write app manifest");
        std::fs::write(&app_main, source).expect("write app main");
        std::fs::write(
            &json_manifest,
            "[package]\nname = \"json\"\nedition = \"2026\"\nkind = \"lib\"\n",
        )
        .expect("write json manifest");
        std::fs::write(&json_lib, "pub fn parse(s: String) -> Int { s.len() }\n")
            .expect("write json lib");

        let uri = Url::from_file_path(&app_main).expect("file uri");
        call_notification(
            &mut service,
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "kyokara",
                    "version": 1,
                    "text": source
                }
            }),
        )
        .await;

        let snapshot = service
            .inner()
            .document_snapshot(&uri)
            .await
            .expect("expected document snapshot");
        let diags = crate::diagnostics::to_lsp_diagnostics(&snapshot, &snapshot.source);
        assert!(
            !diags
                .iter()
                .any(|diag| diag.message.contains("unresolved import")),
            "did not expect unresolved import diagnostics, got: {diags:?}"
        );

        let parse_col = source
            .lines()
            .nth(1)
            .and_then(|line| line.find("parse"))
            .expect("parse offset") as u32;
        let completion = call_request(
            &mut service,
            "textDocument/completion",
            99,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": parse_col }
            }),
        )
        .await;
        assert!(
            completion.is_ok(),
            "completion should succeed: {completion:?}"
        );
        let labels = completion_labels(&completion);
        assert!(
            labels.iter().any(|label| label == "parse"),
            "expected dependency member completion, got labels: {labels:?}"
        );

        std::fs::remove_dir_all(&temp_root).expect("cleanup temp root");
        drain.abort();
    }
}
