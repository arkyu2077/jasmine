//! Per-Board session index + message timelines (v0.0.2 multi-session).
//!
//! A Board has N sessions (conversations); the canvas (board.json) is shared.
//! This module is pure file I/O — it owns the Board sidecar session index
//! and per-session `.jsonl` timelines.
//! (opaque message timelines; the frontend owns the message/block shape and
//! just hands JSON in/out).

use crate::paths::{board_session_timeline, board_sessions_doc};
use crate::provider::{RuntimeProviderIdentity, RuntimeProviderKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub id: String,
    pub thread_id: Option<String>,
    #[serde(default)]
    pub provider_key: Option<String>,
    #[serde(default)]
    pub provider_name: Option<String>,
    #[serde(default)]
    pub provider_kind: Option<String>,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionsDoc {
    pub active_session_id: Option<String>,
    pub sessions: Vec<SessionMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_key: Option<String>,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn load(folder: &Path) -> SessionsDoc {
    match std::fs::read(board_sessions_doc(folder)) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => SessionsDoc::default(),
    }
}

pub fn save(folder: &Path, doc: &SessionsDoc) {
    if let Err(e) = write_json_atomic(board_sessions_doc(folder), doc) {
        tracing::warn!(module = "session", "save sessions.json failed: {e}");
    }
}

fn write_json_atomic(path: PathBuf, doc: &SessionsDoc) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let base = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("sessions.json");
    let tmp = path.with_file_name(format!("{base}.{}.tmp", nanoid::nanoid!(8)));
    let json = serde_json::to_vec_pretty(doc)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

fn make_session(
    thread_id: Option<String>,
    title: &str,
    provider: Option<&RuntimeProviderIdentity>,
) -> SessionMeta {
    let t = now_ms();
    SessionMeta {
        id: nanoid::nanoid!(12),
        thread_id,
        provider_key: provider.map(|p| p.key.clone()),
        provider_name: provider.map(|p| p.name.clone()),
        provider_kind: provider.map(|p| p.kind.as_str().to_string()),
        title: title.to_string(),
        created_at: t,
        updated_at: t,
    }
}

/// Ensure at least one session exists. Migrates a legacy single-session
/// `meta.threadId` into session #1. Returns the (possibly created) doc.
pub fn ensure_initial(folder: &Path, legacy_thread: Option<String>) -> SessionsDoc {
    let mut doc = load(folder);
    if doc.sessions.is_empty() {
        let s = make_session(legacy_thread, "New session", None);
        doc.active_session_id = Some(s.id.clone());
        doc.sessions.push(s);
        save(folder, &doc);
    } else if doc.active_session_id.is_none() {
        doc.active_session_id = doc.sessions.first().map(|s| s.id.clone());
        save(folder, &doc);
    }
    doc
}

pub fn provider_matches(session: &SessionMeta, provider: &RuntimeProviderIdentity) -> bool {
    session.provider_key.as_deref() == Some(provider.key.as_str())
}

pub fn ensure_active_for_provider(
    folder: &Path,
    legacy_thread: Option<String>,
    provider: &RuntimeProviderIdentity,
    title: &str,
) -> SessionsDoc {
    let mut doc = load(folder);
    let mut changed = false;

    if doc.sessions.is_empty() {
        let legacy_thread = if provider.kind == RuntimeProviderKind::Codex {
            legacy_thread
        } else {
            None
        };
        let s = make_session(legacy_thread, title, Some(provider));
        doc.active_session_id = Some(s.id.clone());
        doc.sessions.push(s);
        changed = true;
    } else {
        let active_matches = doc
            .active_session_id
            .as_deref()
            .and_then(|id| doc.sessions.iter().find(|s| s.id == id))
            .is_some_and(|s| provider_matches(s, provider));

        if !active_matches {
            if let Some(existing) = doc
                .sessions
                .iter()
                .filter(|s| provider_matches(s, provider))
                .max_by_key(|s| s.updated_at)
            {
                doc.active_session_id = Some(existing.id.clone());
            } else {
                let s = make_session(None, title, Some(provider));
                doc.active_session_id = Some(s.id.clone());
                doc.sessions.push(s);
            }
            changed = true;
        }
    }

    if changed {
        save(folder, &doc);
    }
    doc
}

/// Create a new session and make it active. Returns it.
pub fn new_session(folder: &Path) -> SessionMeta {
    new_session_with_title(folder, "New session")
}

pub fn new_session_with_title(folder: &Path, title: &str) -> SessionMeta {
    new_session_with_title_and_provider(folder, title, None)
}

pub fn new_session_with_provider(folder: &Path, provider: &RuntimeProviderIdentity) -> SessionMeta {
    new_session_with_title_and_provider(folder, "New session", Some(provider))
}

pub fn new_session_with_title_and_provider(
    folder: &Path,
    title: &str,
    provider: Option<&RuntimeProviderIdentity>,
) -> SessionMeta {
    let mut doc = load(folder);
    let title = title.trim();
    let title = if title.is_empty() {
        "New session"
    } else {
        title
    };
    let s = make_session(None, title, provider);
    doc.active_session_id = Some(s.id.clone());
    doc.sessions.push(s.clone());
    save(folder, &doc);
    s
}

pub fn set_active(folder: &Path, id: &str) {
    let mut doc = load(folder);
    if doc.sessions.iter().any(|s| s.id == id) {
        doc.active_session_id = Some(id.to_string());
        save(folder, &doc);
    }
}

pub fn set_thread(folder: &Path, id: &str, thread_id: &str) {
    set_thread_and_provider(folder, id, thread_id, None);
}

pub fn set_thread_and_provider(
    folder: &Path,
    id: &str,
    thread_id: &str,
    provider: Option<&RuntimeProviderIdentity>,
) {
    let mut doc = load(folder);
    if let Some(s) = doc.sessions.iter_mut().find(|s| s.id == id) {
        s.thread_id = Some(thread_id.to_string());
        if let Some(provider) = provider {
            s.provider_key = Some(provider.key.clone());
            s.provider_name = Some(provider.name.clone());
            s.provider_kind = Some(provider.kind.as_str().to_string());
        }
        s.updated_at = now_ms();
        save(folder, &doc);
    }
}

pub fn rename(folder: &Path, id: &str, title: &str) {
    let mut doc = load(folder);
    if let Some(s) = doc.sessions.iter_mut().find(|s| s.id == id) {
        s.title = title.to_string();
        s.updated_at = now_ms();
        save(folder, &doc);
    }
}

pub fn thread_of(folder: &Path, id: &str) -> Option<String> {
    load(folder)
        .sessions
        .into_iter()
        .find(|s| s.id == id)
        .and_then(|s| s.thread_id)
}

/// Append one message (opaque JSON) to a session's timeline + bump updatedAt.
pub fn append_message(folder: &Path, id: &str, msg: &Value) {
    let line = match serde_json::to_string(msg) {
        Ok(l) => l,
        Err(_) => return,
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(board_session_timeline(folder, id))
    {
        let _ = writeln!(f, "{line}");
    }
    let mut doc = load(folder);
    if let Some(s) = doc.sessions.iter_mut().find(|s| s.id == id) {
        s.updated_at = now_ms();
        save(folder, &doc);
    }
}

/// Read a session's timeline as opaque JSON messages.
pub fn load_timeline(folder: &Path, id: &str) -> Vec<Value> {
    match std::fs::read_to_string(board_session_timeline(folder, id)) {
        Ok(text) => text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::RuntimeProviderKind;

    fn external_identity(name: &str, key: &str) -> RuntimeProviderIdentity {
        RuntimeProviderIdentity {
            key: key.to_string(),
            name: name.to_string(),
            kind: RuntimeProviderKind::External,
            codex_provider_id: Some("jasmine_external_test".to_string()),
        }
    }

    #[test]
    fn provider_sessions_do_not_share_active_thread() {
        let dir = tempfile::tempdir().unwrap();
        let codex = RuntimeProviderIdentity::codex();
        let external = external_identity("modelsrouter", "external:modelsrouter:test");

        let codex_doc = ensure_active_for_provider(
            dir.path(),
            Some("local-thread".to_string()),
            &codex,
            "Codex",
        );
        let codex_id = codex_doc.active_session_id.clone().unwrap();
        assert_eq!(codex_doc.sessions.len(), 1);
        assert_eq!(
            codex_doc.sessions[0].provider_key.as_deref(),
            Some("codex:default")
        );
        assert_eq!(
            codex_doc.sessions[0].thread_id.as_deref(),
            Some("local-thread")
        );

        let external_doc = ensure_active_for_provider(dir.path(), None, &external, "modelsrouter");
        let external_id = external_doc.active_session_id.clone().unwrap();
        assert_ne!(codex_id, external_id);
        assert_eq!(external_doc.sessions.len(), 2);
        let active_external = external_doc
            .sessions
            .iter()
            .find(|s| s.id == external_id)
            .unwrap();
        assert_eq!(
            active_external.provider_key.as_deref(),
            Some("external:modelsrouter:test")
        );
        assert_eq!(active_external.thread_id, None);

        let back_to_codex = ensure_active_for_provider(dir.path(), None, &codex, "Codex");
        assert_eq!(
            back_to_codex.active_session_id.as_deref(),
            Some(codex_id.as_str())
        );
    }

    #[test]
    fn unbound_sessions_are_not_treated_as_provider_matches() {
        let dir = tempfile::tempdir().unwrap();
        let stale = new_session_with_title(dir.path(), "legacy external");
        set_thread(dir.path(), &stale.id, "stale-thread");

        let external = external_identity("modelsrouter", "external:modelsrouter:test");
        let doc = ensure_active_for_provider(dir.path(), None, &external, "modelsrouter");
        let active_id = doc.active_session_id.unwrap();

        assert_ne!(active_id, stale.id);
        assert_eq!(doc.sessions.len(), 2);
        let active = doc.sessions.iter().find(|s| s.id == active_id).unwrap();
        assert_eq!(
            active.provider_key.as_deref(),
            Some("external:modelsrouter:test")
        );
        assert_eq!(active.thread_id, None);
    }
}
