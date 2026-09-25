use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::json;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

mod analysis;
mod completion;
mod hover;
mod navigation;
mod state;

use analysis::{
    code_actions, find_colors, folding_ranges, get_signature_help, inlay_hints, semantic_tokens,
};
use completion::{completions, path_completions, use_symbol_completions};
use hover::hover_at;
use navigation::{
    definition_at, find_references, linked_editing_ranges, prepare_rename_at, rename_at,
};
use state::{DocumentEntry, WorkspaceIndex, apply_change};

// ---------------------------------------------------------------------------
// Backend
// ---------------------------------------------------------------------------

struct Backend {
    client: Client,
    documents: Arc<RwLock<HashMap<Url, Arc<DocumentEntry>>>>,
    index: Arc<RwLock<WorkspaceIndex>>,
    /// Per-URI debounce handles. The value is a join handle for an in-flight
    /// diagnostic publish; replacing the entry cancels the prior handle.
    pending_diags: Arc<Mutex<HashMap<Url, JoinHandle<()>>>>,
    /// Whether the client accepted the `utf-8` position encoding. When false,
    /// positions are UTF-16 code units (the LSP default).
    utf8_positions: AtomicBool,
}

const DIAG_DEBOUNCE: Duration = Duration::from_millis(150);

impl Backend {
    /// Look up the open document for `uri`. Returns the cached entry, which
    /// the caller can use to access text plus the lazy parse cache.
    async fn doc(&self, uri: &Url) -> Option<Arc<DocumentEntry>> {
        self.documents.read().await.get(uri).cloned()
    }

    /// Replace the stored document with a new entry, scheduling a debounced
    /// diagnostic publish and refreshing the workspace symbol index.
    async fn set_doc(&self, uri: Url, text: String, version: i32) {
        let entry = Arc::new(DocumentEntry::new(text, version));
        self.documents
            .write()
            .await
            .insert(uri.clone(), entry.clone());
        self.doc_updated(uri, entry).await;
    }

    /// Follow-up work after a document entry has been stored.
    async fn doc_updated(&self, uri: Url, entry: Arc<DocumentEntry>) {
        // Refresh the workspace symbol index for this file synchronously —
        // symbol extraction is cheap (single pass over text) and keeps
        // workspace-wide queries consistent without waiting on the debounce.
        if let Ok(path) = uri.to_file_path() {
            self.index
                .write()
                .await
                .update_from_text(&path, &entry.text);
        }

        self.schedule_diagnostics(uri, entry).await;
    }

    /// Schedule a diagnostic publish after a short debounce window. If another
    /// change comes in during the window, the previous publish is cancelled
    /// before it runs, so we only parse and publish once per quiet period.
    async fn schedule_diagnostics(&self, uri: Url, entry: Arc<DocumentEntry>) {
        let client = self.client.clone();
        let uri_for_task = uri.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(DIAG_DEBOUNCE).await;
            let parse = entry.parse();
            let diags = build_diagnostics(&entry.text, &parse);
            client
                .publish_diagnostics(uri_for_task, diags, Some(entry.version))
                .await;
        });

        let mut pending = self.pending_diags.lock().await;
        if let Some(prev) = pending.insert(uri, handle) {
            prev.abort();
        }
    }

    /// Walk the workspace once on first use and register a file watcher for
    /// `.hl` files. Idempotent — subsequent calls short-circuit.
    async fn ensure_index_ready(&self) {
        let needs_scan = {
            let idx = self.index.read().await;
            !idx.scanned && idx.root.is_some()
        };
        if needs_scan {
            self.index.write().await.scan();
        }
    }
}

fn build_diagnostics(text: &str, result: &htmlang::parser::ParseResult) -> Vec<Diagnostic> {
    result
        .diagnostics
        .iter()
        .map(|d| {
            let severity = match d.severity {
                htmlang::parser::Severity::Error => DiagnosticSeverity::ERROR,
                htmlang::parser::Severity::Warning => DiagnosticSeverity::WARNING,
                htmlang::parser::Severity::Info => DiagnosticSeverity::INFORMATION,
                htmlang::parser::Severity::Help => DiagnosticSeverity::HINT,
            };
            let line = d.line.saturating_sub(1) as u32;
            let col_start = d.column.unwrap_or(0) as u32;
            let col_end = if d.column.is_some() {
                let lines_vec: Vec<&str> = text.lines().collect();
                lines_vec
                    .get(line as usize)
                    .map(|l| l.len() as u32)
                    .unwrap_or(col_start + 1)
            } else {
                1000
            };
            Diagnostic {
                range: Range::new(Position::new(line, col_start), Position::new(line, col_end)),
                severity: Some(severity),
                source: Some("htmlang".into()),
                message: d.message.clone(),
                ..Default::default()
            }
        })
        .collect()
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Capture the workspace root if the client provided one. Falls back
        // to the parent of the first opened file later, in `did_open`.
        #[allow(deprecated)]
        let root = params
            .workspace_folders
            .as_ref()
            .and_then(|folders| folders.first())
            .and_then(|f| f.uri.to_file_path().ok())
            .or_else(|| params.root_uri.as_ref().and_then(|u| u.to_file_path().ok()));
        if let Some(root) = root {
            self.index.write().await.set_root(root);
        }

        // Positions are byte offsets throughout this server, so prefer the
        // `utf-8` encoding — but only if the client offers it. Otherwise we
        // must stay on the UTF-16 default (VS Code refuses to start a server
        // that answers with an encoding it didn't offer).
        let client_supports_utf8 = params
            .capabilities
            .general
            .as_ref()
            .and_then(|g| g.position_encodings.as_ref())
            .is_some_and(|encs| encs.contains(&PositionEncodingKind::UTF8));
        self.utf8_positions
            .store(client_supports_utf8, Ordering::Relaxed);

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                position_encoding: client_supports_utf8.then_some(PositionEncodingKind::UTF8),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["@".into(), "$".into(), "[".into(), ",".into()]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                document_symbol_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::REFACTOR_EXTRACT,
                        ]),
                        ..Default::default()
                    },
                )),
                color_provider: Some(ColorProviderCapability::Simple(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: vec![
                                    SemanticTokenType::KEYWORD,
                                    SemanticTokenType::VARIABLE,
                                    SemanticTokenType::FUNCTION,
                                    SemanticTokenType::STRING,
                                    SemanticTokenType::COMMENT,
                                    SemanticTokenType::PROPERTY,
                                ],
                                token_modifiers: vec![SemanticTokenModifier::new("deprecated")],
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            ..Default::default()
                        },
                    ),
                ),
                inlay_hint_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                linked_editing_range_provider: Some(LinkedEditingRangeServerCapabilities::Simple(
                    true,
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_range_formatting_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["[".into(), ",".into()]),
                    retrigger_characters: Some(vec![",".into()]),
                    work_done_progress_options: Default::default(),
                }),
                document_link_provider: Some(DocumentLinkOptions {
                    resolve_provider: Some(false),
                    work_done_progress_options: Default::default(),
                }),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec!["htmlang.showReferences".into()],
                    ..Default::default()
                }),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        // Dynamically register a watcher for .hl files under the workspace.
        // Handled clientside (VS Code etc.) and dispatched back to us via
        // `did_change_watched_files`.
        let registration = Registration {
            id: "htmlang-watch-hl".into(),
            method: "workspace/didChangeWatchedFiles".into(),
            register_options: Some(json!({
                "watchers": [{ "globPattern": "**/*.hl" }]
            })),
        };
        let _ = self.client.register_capability(vec![registration]).await;
        self.ensure_index_ready().await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        // If we still don't have a workspace root, fall back to the parent of
        // the first opened file. Common case for `code path/to/file.hl`.
        if self.index.read().await.root.is_none()
            && let Ok(path) = params.text_document.uri.to_file_path()
            && let Some(parent) = path.parent()
        {
            let mut idx = self.index.write().await;
            idx.set_root(parent.to_path_buf());
            idx.scan();
        }
        self.set_doc(
            params.text_document.uri,
            params.text_document.text,
            params.text_document.version,
        )
        .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let version = params.text_document.version;

        let utf8 = self.utf8_positions.load(Ordering::Relaxed);

        // Hold the write lock across read-modify-write: tower-lsp may run
        // notification handlers concurrently, and two interleaved incremental
        // edits applied to the same base text would lose one of them.
        let entry = {
            let mut docs = self.documents.write().await;
            let mut text = docs.get(&uri).map(|d| d.text.clone()).unwrap_or_default();
            for change in &params.content_changes {
                apply_change(&mut text, change, utf8);
            }
            let entry = Arc::new(DocumentEntry::new(text, version));
            docs.insert(uri.clone(), entry.clone());
            entry
        };
        self.doc_updated(uri, entry).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents
            .write()
            .await
            .remove(&params.text_document.uri);
        if let Some(handle) = self
            .pending_diags
            .lock()
            .await
            .remove(&params.text_document.uri)
        {
            handle.abort();
        }
        // The index may hold symbols from unsaved buffer edits; resync it
        // with what's actually on disk.
        if let Ok(path) = params.text_document.uri.to_file_path() {
            let mut idx = self.index.write().await;
            if path.exists() {
                idx.update_from_disk(&path);
            } else {
                idx.remove(&path);
            }
        }
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let mut idx = self.index.write().await;
        for change in params.changes {
            let Ok(path) = change.uri.to_file_path() else {
                continue;
            };
            match change.typ {
                FileChangeType::CREATED | FileChangeType::CHANGED => {
                    idx.update_from_disk(&path);
                }
                FileChangeType::DELETED => {
                    idx.remove(&path);
                }
                _ => {}
            }
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let Some(doc) = self.doc(uri).await else {
            return Ok(None);
        };
        let text = &doc.text;

        let lines: Vec<&str> = text.lines().collect();
        if let Some(line) = lines.get(pos.line as usize) {
            let trimmed = line.trim_start();
            if trimmed.starts_with("@include ")
                || trimmed.starts_with("@import ")
                || trimmed.starts_with("@extends ")
            {
                let items = path_completions(uri, pos);
                if !items.is_empty() {
                    return Ok(Some(CompletionResponse::Array(items)));
                }
            }
            if let Some(after_use) = trimmed.strip_prefix("@use ") {
                let has_file = after_use.contains(".hl");
                if has_file {
                    let items = use_symbol_completions(uri, trimmed, pos);
                    if !items.is_empty() {
                        return Ok(Some(CompletionResponse::Array(items)));
                    }
                } else {
                    let items = path_completions(uri, pos);
                    if !items.is_empty() {
                        return Ok(Some(CompletionResponse::Array(items)));
                    }
                }
            }
        }

        let items = completions(text, pos);
        Ok(if items.is_empty() {
            None
        } else {
            Some(CompletionResponse::Array(items))
        })
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(doc) = self.doc(uri).await else {
            return Ok(None);
        };
        Ok(hover_at(&doc.text, pos))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
        let pos = params.text_document_position_params.position;
        let Some(doc) = self.doc(&uri).await else {
            return Ok(None);
        };

        // Try the local file first — fast and matches common case.
        if let Some(local) = definition_at(&doc.text, pos, &uri) {
            return Ok(Some(local));
        }

        // Fall back to the workspace index for cross-file lookups.
        let idx = self.index.read().await;
        let target = navigation::cross_file_definition(&doc.text, pos, &idx);
        Ok(target)
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = &params.text_document.uri;
        let pos = params.position;
        let Some(doc) = self.doc(uri).await else {
            return Ok(None);
        };
        Ok(prepare_rename_at(&doc.text, pos))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri.clone();
        let pos = params.text_document_position.position;
        let new_name = params.new_name;
        let Some(doc) = self.doc(&uri).await else {
            return Ok(None);
        };
        Ok(rename_at(&doc.text, pos, &new_name, &uri))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.doc(uri).await else {
            return Ok(None);
        };
        let symbols = doc.symbols();
        Ok(if symbols.is_empty() {
            None
        } else {
            // Re-stamp URI so the cached entry's "file:///" placeholder is
            // replaced with the document's own URI.
            let mut out = (*symbols).clone();
            for s in &mut out {
                s.location.uri = uri.clone();
            }
            Some(DocumentSymbolResponse::Flat(out))
        })
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri.clone();
        let Some(doc) = self.doc(&uri).await else {
            return Ok(None);
        };
        let actions = code_actions(&doc.text, &params.range, &params.context.diagnostics, &uri);
        Ok(if actions.is_empty() {
            None
        } else {
            Some(actions)
        })
    }

    async fn document_color(&self, params: DocumentColorParams) -> Result<Vec<ColorInformation>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.doc(uri).await else {
            return Ok(vec![]);
        };
        Ok(find_colors(&doc.text))
    }

    async fn color_presentation(
        &self,
        params: ColorPresentationParams,
    ) -> Result<Vec<ColorPresentation>> {
        let c = params.color;
        let r = (c.red * 255.0).round() as u8;
        let g = (c.green * 255.0).round() as u8;
        let b = (c.blue * 255.0).round() as u8;
        let a = (c.alpha * 255.0).round() as u8;
        let hex = if c.alpha < 1.0 {
            format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, a)
        } else {
            format!("#{:02x}{:02x}{:02x}", r, g, b)
        };
        let mut presentations = vec![ColorPresentation {
            label: hex.clone(),
            text_edit: Some(TextEdit {
                range: params.range,
                new_text: hex.clone(),
            }),
            additional_text_edits: None,
        }];
        // Offer a CSS named-color presentation when one matches exactly.
        if c.alpha >= 1.0
            && let Some(name) = analysis::named_color_for(r, g, b)
        {
            presentations.push(ColorPresentation {
                label: name.to_string(),
                text_edit: Some(TextEdit {
                    range: params.range,
                    new_text: name.to_string(),
                }),
                additional_text_edits: None,
            });
        }
        Ok(presentations)
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.doc(uri).await else {
            return Ok(None);
        };
        let ranges = folding_ranges(&doc.text);
        Ok(if ranges.is_empty() {
            None
        } else {
            Some(ranges)
        })
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.doc(uri).await else {
            return Ok(None);
        };
        let parse = doc.parse();
        let tokens = semantic_tokens(&doc.text, &parse);
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: tokens,
        })))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.doc(uri).await else {
            return Ok(None);
        };
        let hints = inlay_hints(&doc.text);
        Ok(if hints.is_empty() { None } else { Some(hints) })
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        self.ensure_index_ready().await;

        let query = params.query.to_lowercase();

        // Open buffers always win — they may be ahead of the on-disk version.
        let mut all_symbols: Vec<SymbolInformation> = Vec::new();
        let mut covered_files: std::collections::HashSet<std::path::PathBuf> =
            std::collections::HashSet::new();

        let docs = self.documents.read().await;
        for (uri, doc) in docs.iter() {
            if let Ok(path) = uri.to_file_path() {
                covered_files.insert(path);
            }
            for sym in doc.symbols().iter() {
                if query.is_empty() || sym.name.to_lowercase().contains(&query) {
                    let mut s = sym.clone();
                    s.location.uri = uri.clone();
                    all_symbols.push(s);
                }
            }
        }
        drop(docs);

        let idx = self.index.read().await;
        for (path, syms) in &idx.by_file {
            if covered_files.contains(path) {
                continue;
            }
            for sym in syms {
                if query.is_empty() || sym.name.to_lowercase().contains(&query) {
                    all_symbols.push(sym.clone());
                }
            }
        }

        Ok(if all_symbols.is_empty() {
            None
        } else {
            Some(all_symbols)
        })
    }

    async fn linked_editing_range(
        &self,
        params: LinkedEditingRangeParams,
    ) -> Result<Option<LinkedEditingRanges>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(doc) = self.doc(uri).await else {
            return Ok(None);
        };
        Ok(linked_editing_ranges(&doc.text, pos))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.doc(uri).await else {
            return Ok(None);
        };
        let formatted = htmlang::fmt::format(&doc.text);
        if formatted == doc.text {
            return Ok(None);
        }
        // End the edit at the true end of the document. `str::lines` drops a
        // trailing newline, which would leave it outside the replaced range
        // and append an extra blank line after the (newline-terminated)
        // formatted text.
        let last_line = doc.text.matches('\n').count() as u32;
        let last_col = doc.text.rsplit('\n').next().map_or(0, |l| l.len()) as u32;
        Ok(Some(vec![TextEdit {
            range: Range::new(Position::new(0, 0), Position::new(last_line, last_col)),
            new_text: formatted,
        }]))
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let range = params.range;
        let Some(doc) = self.doc(uri).await else {
            return Ok(None);
        };

        let lines: Vec<&str> = doc.text.lines().collect();
        let start_line = range.start.line as usize;
        let end_line = (range.end.line as usize).min(lines.len().saturating_sub(1));
        if start_line > end_line {
            return Ok(None);
        }

        let selection: String = lines[start_line..=end_line].join("\n");
        let formatted = htmlang::fmt::format(&selection);
        let formatted = formatted.trim_end_matches('\n').to_string();
        if formatted == selection {
            return Ok(None);
        }

        let last_col = lines[end_line].len() as u32;
        Ok(Some(vec![TextEdit {
            range: Range::new(
                Position::new(start_line as u32, 0),
                Position::new(end_line as u32, last_col),
            ),
            new_text: formatted,
        }]))
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let uri = params.text_document.uri.clone();
        let Some(doc) = self.doc(&uri).await else {
            return Ok(None);
        };
        let text = &doc.text;

        let Ok(this_path) = uri.to_file_path() else {
            return Ok(None);
        };
        let Some(dir) = this_path.parent() else {
            return Ok(None);
        };

        let mut links = Vec::new();
        for (i, raw_line) in text.lines().enumerate() {
            let trimmed = raw_line.trim_start();
            let indent = raw_line.len() - trimmed.len();
            let (prefix, filename) = if let Some(rest) = trimmed.strip_prefix("@include ") {
                ("@include ", rest)
            } else if let Some(rest) = trimmed.strip_prefix("@import ") {
                ("@import ", rest)
            } else if let Some(rest) = trimmed.strip_prefix("@use ") {
                ("@use ", rest)
            } else if let Some(rest) = trimmed.strip_prefix("@extends ") {
                ("@extends ", rest)
            } else {
                continue;
            };

            let name_token: &str = filename
                .trim_start_matches('"')
                .split(|c: char| c.is_whitespace() || c == ',')
                .next()
                .unwrap_or("")
                .trim_end_matches('"');
            if name_token.is_empty() {
                continue;
            }
            if name_token.contains('*') || name_token.contains('?') {
                continue;
            }

            let target = dir.join(name_token);
            if !target.exists() {
                continue;
            }
            let Ok(target_uri) = Url::from_file_path(&target) else {
                continue;
            };

            let scan_from = indent + prefix.len();
            let Some(rel_start) = raw_line[scan_from..].find(name_token) else {
                continue;
            };
            let start_col = (scan_from + rel_start) as u32;
            let end_col = start_col + name_token.len() as u32;

            links.push(DocumentLink {
                range: Range::new(
                    Position::new(i as u32, start_col),
                    Position::new(i as u32, end_col),
                ),
                target: Some(target_uri),
                tooltip: Some(format!("Open {}", name_token)),
                data: None,
            });
        }

        Ok(if links.is_empty() { None } else { Some(links) })
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = params.text_document.uri.clone();
        let Some(doc) = self.doc(&uri).await else {
            return Ok(None);
        };
        let text = &doc.text;
        let lines: Vec<&str> = text.lines().collect();

        #[derive(Clone)]
        struct Def {
            line: u32,
            col: u32,
            name: String,
            kind: &'static str,
        }
        let mut defs: Vec<Def> = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("@let ") {
                // `rest` is a suffix of `line`, so the name's column is
                // however much of the line precedes it.
                let col = (line.len() - rest.trim_start().len()) as u32;
                let rest = rest.trim();
                if let Some(name) = rest.split_whitespace().next() {
                    let has_body = lines
                        .get(i + 1)
                        .map(|l| l.starts_with("  ") || l.starts_with('\t'))
                        .unwrap_or(false);
                    let value_after_name = rest[name.len()..].trim_start();
                    if has_body
                        && (value_after_name.is_empty() || value_after_name.starts_with('$'))
                    {
                        defs.push(Def {
                            line: i as u32,
                            col,
                            name: name.to_string(),
                            kind: "fn",
                        });
                    } else if value_after_name.starts_with('[') {
                        defs.push(Def {
                            line: i as u32,
                            col,
                            name: name.to_string(),
                            kind: "define",
                        });
                    } else {
                        defs.push(Def {
                            line: i as u32,
                            col,
                            name: name.to_string(),
                            kind: "let",
                        });
                    }
                }
            }
        }

        let mut lenses = Vec::with_capacity(defs.len());
        for def in &defs {
            let mut locations: Vec<Location> = Vec::new();
            let needle = match def.kind {
                "fn" => format!("@{}", def.name),
                _ => format!("${}", def.name),
            };
            for (i, line) in lines.iter().enumerate() {
                if i as u32 == def.line {
                    continue;
                }
                let mut from = 0;
                while let Some(idx) = line[from..].find(&needle) {
                    let pos = from + idx;
                    let after = line.as_bytes().get(pos + needle.len()).copied();
                    let ok = match after {
                        None => true,
                        Some(c) => !(c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
                    };
                    if ok {
                        locations.push(Location {
                            uri: uri.clone(),
                            range: Range::new(
                                Position::new(i as u32, pos as u32),
                                Position::new(i as u32, (pos + needle.len()) as u32),
                            ),
                        });
                    }
                    from = pos + needle.len();
                }
            }

            let title = if locations.len() == 1 {
                "1 reference".to_string()
            } else {
                format!("{} references", locations.len())
            };
            // A clickable lens that opens VS Code's references panel at this
            // definition. The arguments mirror what VS Code's built-in
            // `editor.action.showReferences` accepts: (uri, position, locations).
            // The locations are sent precomputed because a reference query at
            // the definition site would resolve the `@let` keyword instead.
            let line_pos = Position::new(def.line, 0);
            let name_pos = Position::new(def.line, def.col);
            lenses.push(CodeLens {
                range: Range::new(line_pos, line_pos),
                command: Some(Command {
                    title,
                    command: "htmlang.showReferences".into(),
                    arguments: Some(vec![json!(uri), json!(name_pos), json!(locations)]),
                }),
                data: None,
            });
        }

        Ok(if lenses.is_empty() {
            None
        } else {
            Some(lenses)
        })
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        if params.command == "htmlang.showReferences" {
            // The VS Code client converts this to `editor.action.showReferences`
            // through a registered middleware; on other clients it's a no-op.
            // We still acknowledge it so the lens click doesn't error.
            return Ok(None);
        }
        Ok(None)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri.clone();
        let pos = params.text_document_position.position;
        let Some(doc) = self.doc(&uri).await else {
            return Ok(None);
        };
        let mut refs = find_references(&doc.text, pos, &uri);

        // Workspace-wide reference search: scan every other indexed file for
        // textual occurrences of the same symbol.
        if let Some(symbol) = navigation::symbol_at(&doc.text, pos) {
            self.ensure_index_ready().await;
            let idx = self.index.read().await;
            for path in idx.iter_files() {
                let Ok(file_uri) = Url::from_file_path(path) else {
                    continue;
                };
                if file_uri == uri {
                    continue;
                }
                // Skip files that have an open buffer — we'd double-count
                // because their content might differ from disk.
                if self.documents.read().await.contains_key(&file_uri) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(path) else {
                    continue;
                };
                refs.extend(navigation::find_references_for_symbol(
                    &text, &symbol, &file_uri,
                ));
            }

            // Also search other open buffers.
            let docs = self.documents.read().await;
            for (other_uri, other_doc) in docs.iter() {
                if other_uri == &uri {
                    continue;
                }
                refs.extend(navigation::find_references_for_symbol(
                    &other_doc.text,
                    &symbol,
                    other_uri,
                ));
            }
        }

        Ok(if refs.is_empty() { None } else { Some(refs) })
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(doc) = self.doc(uri).await else {
            return Ok(None);
        };
        Ok(get_signature_help(&doc.text, pos))
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend {
        client,
        documents: Arc::new(RwLock::new(HashMap::new())),
        index: Arc::new(RwLock::new(WorkspaceIndex::new())),
        pending_diags: Arc::new(Mutex::new(HashMap::new())),
        utf8_positions: AtomicBool::new(false),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
