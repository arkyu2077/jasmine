//! Filesystem layout.
//!
//! Two distinct Jasmine state locations — don't confuse them:
//!
//! 1. **Global app dir** `~/.jasmine/` — app-level state that is NOT tied to any
//!    one Board: logs, the recent-Boards index, app settings. Overridable with
//!    `JASMINE_HOME` (tests / portable installs). Existing `~/.cameo/` installs
//!    continue to be read for compatibility.
//!
//! 2. **Per-Board sidecar** `<board-folder>/.jasmine/` — everything that belongs
//!    to one Board: `board.json` (Placements / Annotations / layout),
//!    `meta.json` (threadId / runtime / settings), `session.jsonl` (timeline),
//!    `thumbs/`, and dispatch temp images. Lives INSIDE the user's folder so the
//!    Board is self-contained and portable (like `.git`). The Codex agent's cwd
//!    is the folder itself; it is told not to touch `.jasmine/` or legacy
//!    `.cameo/` state but CAN read under it (sandbox = workspace-write rooted at
//!    the folder) — which is why dispatch temp images live here (decision D5).

use std::path::{Path, PathBuf};

// ── Global app dir ─────────────────────────────────────────────────────────

pub fn jasmine_data_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("JASMINE_HOME") {
        return PathBuf::from(custom);
    }
    if let Ok(custom) = std::env::var("CAMEO_HOME") {
        return PathBuf::from(custom);
    }
    let home = dirs::home_dir().expect("home dir");
    let current = home.join(".jasmine");
    let legacy = home.join(".cameo");
    if !current.exists() && legacy.exists() {
        migrate_legacy_global_dir(&legacy, &current);
    }
    current
}

fn migrate_legacy_global_dir(legacy: &Path, current: &Path) {
    let _ = std::fs::create_dir_all(current);
    for name in [
        "config.json",
        "workspaces.json",
        "boards.jsonl",
        "device_id",
    ] {
        let src = legacy.join(name);
        let dst = current.join(name);
        if src.exists() && !dst.exists() {
            let _ = std::fs::copy(src, dst);
        }
    }
}

pub fn jasmine_logs_dir() -> PathBuf {
    jasmine_data_dir().join("logs")
}

/// Newline-delimited JSON index of recently opened Boards (path + last-opened).
pub fn boards_index_path() -> PathBuf {
    jasmine_data_dir().join("boards.jsonl")
}

/// Global app config (network proxy etc.) — `~/.jasmine/config.json`.
pub fn app_config_path() -> PathBuf {
    jasmine_data_dir().join("config.json")
}

pub fn ensure_data_layout() -> std::io::Result<()> {
    std::fs::create_dir_all(jasmine_data_dir())?;
    std::fs::create_dir_all(jasmine_logs_dir())?;
    Ok(())
}

// ── Per-Board sidecar (inside the user's folder) ─────────────────────────────

/// `<folder>/.jasmine` (or existing legacy `<folder>/.cameo`)
pub fn board_sidecar_dir(folder: &Path) -> PathBuf {
    let current = folder.join(".jasmine");
    let legacy = folder.join(".cameo");
    if !current.exists() && legacy.exists() {
        legacy
    } else {
        current
    }
}

/// `<folder>/.jasmine/board.json` — Placements / Annotations / layout.
pub fn board_doc_path(folder: &Path) -> PathBuf {
    board_sidecar_dir(folder).join("board.json")
}

/// `<folder>/.jasmine/meta.json` — threadId / runtime / settings.
pub fn board_meta_path(folder: &Path) -> PathBuf {
    board_sidecar_dir(folder).join("meta.json")
}

/// `<folder>/.jasmine/sessions.json` — the session index.
pub fn board_sessions_doc(folder: &Path) -> PathBuf {
    board_sidecar_dir(folder).join("sessions.json")
}

/// `<folder>/.jasmine/sessions/` — per-session message timelines.
pub fn board_sessions_dir(folder: &Path) -> PathBuf {
    board_sidecar_dir(folder).join("sessions")
}

/// `<folder>/.jasmine/sessions/<id>.jsonl` — one session's append-only timeline.
pub fn board_session_timeline(folder: &Path, session_id: &str) -> PathBuf {
    board_sessions_dir(folder).join(format!("{session_id}.jsonl"))
}

/// `<folder>/.jasmine/thumbs` — thumbnail cache.
pub fn board_thumbs_dir(folder: &Path) -> PathBuf {
    board_sidecar_dir(folder).join("thumbs")
}

/// `<folder>/.jasmine/tmp` — dispatch temp images (clean + overlay). Inside the
/// workspace so the Codex sandbox can read them (decision D5).
pub fn board_tmp_dir(folder: &Path) -> PathBuf {
    board_sidecar_dir(folder).join("tmp")
}

/// Create the per-Board sidecar layout. Idempotent.
pub fn ensure_board_sidecar(folder: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(board_sidecar_dir(folder))?;
    std::fs::create_dir_all(board_thumbs_dir(folder))?;
    std::fs::create_dir_all(board_tmp_dir(folder))?;
    std::fs::create_dir_all(board_sessions_dir(folder))?;
    Ok(())
}
