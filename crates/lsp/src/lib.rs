//! `kyokara-lsp` — Language Server Protocol implementation for Kyokara.
//!
//! Provides IDE features over LSP: live diagnostics, hover, go-to-definition,
//! find references, completion, code actions (quickfixes), and formatting.
//!
//! Uses salsa for incremental analysis — when a file's source text changes,
//! only that file's analysis is recomputed.

pub mod code_action;
pub mod completion;
pub mod db;
pub mod diagnostics;
pub mod format;
pub mod goto_def;
pub mod hover;
pub mod position;
pub mod references;
pub mod server;

use tower_lsp::{LspService, Server};

/// Start the LSP server on stdin/stdout.
pub async fn run_lsp() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(server::KyokaraLanguageServer::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
