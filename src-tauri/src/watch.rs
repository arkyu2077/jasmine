//! Per-board folder watcher.
//!
//! Codex edits video by shelling out to ffmpeg, which writes output files into
//! the board folder. Unlike image generation (a structured `imageGeneration`
//! item), these arrive only as files on disk — so we watch the folder and surface
//! new media on the canvas in real time, including intermediate products produced
//! mid-turn.
//!
//! Robustness (per Codex review):
//! - **Half-written files:** the debouncer coalesces rapid events (~750ms); on top
//!   of that we require the file size to be stable across a short delay before
//!   ingesting, so a still-encoding mp4 isn't probed/decoded prematurely. Codex is
//!   also told (prompt.rs) to write a temp name then atomically rename to the final
//!   extension — and we only treat known media extensions as candidates, so
//!   `*.tmp`/`*.part` working files are ignored until the rename lands.
//! - **Lineage:** routed through the board's Codex session so a file produced
//!   during a turn chains to that turn's source placement (`current_sources`),
//!   exactly like image outputs. Manual drops (no session) get no parent.
//! - **Idempotency:** the single chokepoint is `board::place_minted` (skips paths
//!   already tracked), so watcher / generation / turn-sweep can't double-mint.

use crate::board::{self, BoardRegistry};
use crate::codex::CodexRegistry;
use crate::runtime::{CodexEventEnvelope, UnifiedEvent, CODEX_EVENT};
use crate::{assets, storage};
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Coalesce window for filesystem events.
const DEBOUNCE: Duration = Duration::from_millis(750);
/// Re-stat delay used to confirm a file finished being written.
const STABILITY_DELAY: Duration = Duration::from_millis(400);

#[derive(Default)]
pub struct WatchRegistry {
    inner: Mutex<HashMap<String, Debouncer<RecommendedWatcher>>>,
}

impl WatchRegistry {
    /// Stop and drop the watcher for a board (idempotent).
    pub fn stop(&self, board_id: &str) {
        self.inner.lock().remove(board_id);
    }

    fn put(&self, board_id: String, d: Debouncer<RecommendedWatcher>) {
        self.inner.lock().insert(board_id, d);
    }
}

/// Start (or restart) watching a board's folder for new media files. Replaces any
/// existing watcher for the same board so re-opening doesn't leak watchers.
///
/// NOTE: there is no close-board command, so a watcher lives until the board is
/// re-opened (replaced here) or the app exits (the registry is dropped). Opening
/// many distinct boards accumulates watchers; acceptable for V1's usage.
pub fn start(app: &AppHandle, board_id: String, folder: PathBuf) {
    let watchers = app.state::<Arc<WatchRegistry>>().inner().clone();
    watchers.stop(&board_id);

    let cb_app = app.clone();
    let cb_board = board_id.clone();
    let cb_folder = folder.clone();
    let debouncer = new_debouncer(DEBOUNCE, move |res: DebounceEventResult| {
        let events = match res {
            Ok(evts) => evts,
            Err(_) => return,
        };
        for ev in events {
            let Some(name) = ev.path.file_name().and_then(|n| n.to_str()).map(String::from) else {
                continue;
            };
            // A render request (`<base>.render.json`) → drive the motion-graphics
            // render primitive. Top-level only, like media candidates.
            if ev.path.parent() == Some(cb_folder.as_path())
                && name.ends_with(crate::render::REQUEST_SUFFIX)
            {
                let app = cb_app.clone();
                let board_id = cb_board.clone();
                let folder = cb_folder.clone();
                let path = ev.path.clone();
                tauri::async_runtime::spawn(async move {
                    // Wait for the JSON to finish writing (same size-stable gate as
                    // media) so a half-written request isn't parsed as invalid.
                    if !is_stable(&path).await {
                        return;
                    }
                    crate::render::handle_render_request(&app, &board_id, &folder, &name);
                });
                continue;
            }
            if !is_candidate(&ev.path, &cb_folder) {
                continue;
            }
            let app = cb_app.clone();
            let board_id = cb_board.clone();
            let folder = cb_folder.clone();
            let path = ev.path.clone();
            tauri::async_runtime::spawn(async move {
                handle_settled(app, board_id, folder, name, path).await;
            });
        }
    });

    match debouncer {
        Ok(mut d) => {
            if let Err(e) = d.watcher().watch(&folder, RecursiveMode::NonRecursive) {
                tracing::warn!(module = "watch", "watch {} failed: {e}", folder.display());
                return;
            }
            watchers.put(board_id, d);
            tracing::info!(module = "watch", "watching {}", folder.display());
        }
        Err(e) => tracing::warn!(module = "watch", "debouncer init failed: {e}"),
    }
}

/// A top-level media file (not hidden, not a temp/working file). Non-media
/// extensions (e.g. ffmpeg's `*.tmp` while encoding) are ignored until the
/// atomic rename produces a real media extension.
fn is_candidate(path: &Path, folder: &Path) -> bool {
    if path.parent() != Some(folder) {
        return false; // top-level only (matches scan_media)
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('.') {
        return false;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| assets::is_media_ext(&e.to_lowercase()))
        .unwrap_or(false)
}

/// Confirm the file finished being written: present, non-empty, and the same size
/// across [`STABILITY_DELAY`]. Guards against probing a half-encoded video.
async fn is_stable(path: &Path) -> bool {
    let Some(s1) = std::fs::metadata(path).ok().map(|m| m.len()) else {
        return false;
    };
    tokio::time::sleep(STABILITY_DELAY).await;
    matches!(std::fs::metadata(path).ok().map(|m| m.len()), Some(s2) if s2 == s1 && s2 > 0)
}

async fn handle_settled(
    app: AppHandle,
    board_id: String,
    folder: PathBuf,
    name: String,
    path: PathBuf,
) {
    if !is_stable(&path).await {
        return;
    }
    // Route through the Codex session when one exists so lineage attribution and
    // event emission share the turn's source context + output index.
    let session = app.state::<Arc<CodexRegistry>>().get(&board_id);
    match session {
        Some(s) => crate::codex::on_external_media(&s.inner, &name).await,
        None => ingest_direct(&app, &board_id, &folder, &name),
    }
}

/// Ingest a manually-added media file when no Codex session is running for the
/// board (e.g. dropped via the OS file manager). No lineage; emits directly.
fn ingest_direct(app: &AppHandle, board_id: &str, folder: &Path, name: &str) {
    let boards = app.state::<Arc<BoardRegistry>>();
    let Some(entry) = boards.get(board_id) else {
        return;
    };
    if entry.doc.lock().assets.iter().any(|a| a.path == name) {
        return;
    }
    let asset = match assets::mint_asset(folder, name, crate::model::Origin::Imported) {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!(module = "watch", "ingest mint {name} failed: {e}");
            return;
        }
    };
    let save_guard = entry.save.lock();
    let placed = {
        let mut doc = entry.doc.lock();
        board::place_minted(&asset, None, 0, &mut doc).map(|pl| (pl, doc.clone()))
    };
    let Some((placement, doc_clone)) = placed else {
        return;
    };
    if let Err(e) = storage::save_board_doc(folder, &doc_clone) {
        tracing::warn!(module = "watch", "save after manual media ingest failed: {e}");
    }
    drop(save_guard);
    let _ = app.emit(
        CODEX_EVENT,
        CodexEventEnvelope {
            board_id: board_id.to_string(),
            event: UnifiedEvent::MediaIngested {
                asset,
                placement,
                placeholder_id: None,
            },
        },
    );
}
