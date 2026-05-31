//! Runtime-agnostic event stream. The Codex adapter (codex.rs) translates Codex
//! JSON-RPC notifications into `UnifiedEvent`s; everything above the adapter
//! consumes only these, keeping the runtime swappable (Codex now, others later).
//!
//! This is the deliberate swap boundary — there is one concrete runtime today,
//! so we keep the abstraction at the event shape rather than a premature trait.

use crate::model::{Asset, Placement};
use serde::Serialize;

/// One step in the agent's plan (turn/plan/updated). status ∈ pending |
/// inProgress | completed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub step: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UnifiedEvent {
    /// Thread established (start or resume).
    SessionInit {
        thread_id: String,
        model: String,
    },
    /// Assistant text token.
    TextDelta {
        text: String,
    },
    TextStop,
    /// Reasoning / plan stream.
    ThinkingStart,
    ThinkingDelta {
        text: String,
    },
    ThinkingStop,
    /// Generic tool lifecycle (Bash/Edit/Read/ImageGeneration/…) for the chat log.
    /// `detail` is the first-level gray subtitle (the command / file / query).
    ToolStart {
        tool_use_id: String,
        tool_name: String,
        detail: Option<String>,
    },
    ToolStop {
        tool_use_id: String,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
    /// A generation just started — show a loading placeholder at the predicted
    /// landing spot (right of source). Replaced by `ImageGenerated`.
    GenerationStarted {
        placeholder_id: String,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    /// The payload Jasmine lives on: a generated image, already minted as an Asset
    /// and placed (right of source) on the board. `placeholder_id` (if any) is
    /// the loading placeholder to remove.
    ImageGenerated {
        asset: Asset,
        placement: Placement,
        caption: Option<String>,
        placeholder_id: Option<String>,
    },
    /// Codex asked the client to approve something (auto-accepted in v1; shown).
    PermissionRequest {
        request_id: u64,
        summary: String,
    },
    /// Turn finished. `status` ∈ completed / aborted / error.
    TurnComplete {
        status: String,
        error: Option<String>,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    /// The agent's plan/todo for the turn (turn/plan/updated).
    PlanUpdated {
        explanation: Option<String>,
        steps: Vec<PlanStep>,
    },
    /// Subscription rate-limit usage (account/rateLimits/updated). `used_percent`
    /// + `resets_at` are the **primary (5-hour rolling)** window; `secondary_*`
    /// are the **weekly** window. `reached` indicates which one (if any) the
    /// user has actually hit (`"primary"` / `"secondary"`).
    RateLimits {
        used_percent: f64,
        resets_at: Option<f64>,
        secondary_used_percent: Option<f64>,
        secondary_resets_at: Option<f64>,
        reached: Option<String>,
    },
    Status {
        state: String,
    },
    /// Structured runtime transport state derived inside the runtime adapter.
    TransportStatus {
        phase: String,
        attempt: Option<u64>,
        max: Option<u64>,
        message: String,
    },
    /// Fatal runtime failure. Recoverable runtime diagnostics are `Log` events;
    /// turn state should normally settle through `TurnComplete`.
    Error {
        message: String,
    },
    /// Process exited / session ended.
    SessionComplete {
        ok: bool,
        message: String,
    },
    Log {
        level: String,
        message: String,
    },
}

/// Envelope emitted on the Tauri event channel `codex-event`, tagging which
/// Board the event belongs to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEventEnvelope {
    pub board_id: String,
    pub event: UnifiedEvent,
}

pub const CODEX_EVENT: &str = "codex-event";

#[cfg(test)]
mod wire_contract {
    //! Guards the Rust→TS event contract. `UnifiedEvent` is `#[serde(tag =
    //! "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]`, and
    //! the frontend's `CodexEvent` union (src/types.ts) + `handleEvent`
    //! (src/store/chat.ts) read those exact `kind` tags and camelCase fields. If
    //! a variant or field is renamed on one side without the other, turns
    //! silently mis-handle — so pin the serialized shape here. Update both sides
    //! together when you change one.
    use super::*;
    use serde_json::json;

    fn wire(event: UnifiedEvent) -> serde_json::Value {
        serde_json::to_value(event).unwrap()
    }

    #[test]
    fn event_kinds_and_fields_match_ts_codexevent() {
        assert_eq!(
            wire(UnifiedEvent::SessionInit { thread_id: "t1".into(), model: "gpt".into() }),
            json!({ "kind": "sessionInit", "threadId": "t1", "model": "gpt" }),
        );
        assert_eq!(
            wire(UnifiedEvent::TextDelta { text: "hi".into() }),
            json!({ "kind": "textDelta", "text": "hi" }),
        );
        assert_eq!(wire(UnifiedEvent::TextStop), json!({ "kind": "textStop" }));
        assert_eq!(wire(UnifiedEvent::ThinkingStart), json!({ "kind": "thinkingStart" }));
        assert_eq!(
            wire(UnifiedEvent::ToolStart {
                tool_use_id: "u1".into(),
                tool_name: "Bash".into(),
                detail: Some("ls".into()),
            }),
            json!({ "kind": "toolStart", "toolUseId": "u1", "toolName": "Bash", "detail": "ls" }),
        );
        assert_eq!(
            wire(UnifiedEvent::ToolStop { tool_use_id: "u1".into() }),
            json!({ "kind": "toolStop", "toolUseId": "u1" }),
        );
        assert_eq!(
            wire(UnifiedEvent::GenerationStarted {
                placeholder_id: "ph".into(),
                x: 1.0,
                y: 2.0,
                w: 3.0,
                h: 4.0,
            }),
            json!({ "kind": "generationStarted", "placeholderId": "ph", "x": 1.0, "y": 2.0, "w": 3.0, "h": 4.0 }),
        );
        // The terminal events that drive the message-flow guarantees.
        assert_eq!(
            wire(UnifiedEvent::TurnComplete { status: "completed".into(), error: None }),
            json!({ "kind": "turnComplete", "status": "completed", "error": null }),
        );
        assert_eq!(
            wire(UnifiedEvent::Error { message: "boom".into() }),
            json!({ "kind": "error", "message": "boom" }),
        );
        assert_eq!(
            wire(UnifiedEvent::SessionComplete { ok: false, message: "exited".into() }),
            json!({ "kind": "sessionComplete", "ok": false, "message": "exited" }),
        );
        assert_eq!(
            wire(UnifiedEvent::Log { level: "warn".into(), message: "n".into() }),
            json!({ "kind": "log", "level": "warn", "message": "n" }),
        );
        assert_eq!(
            wire(UnifiedEvent::TransportStatus {
                phase: "reconnecting".into(),
                attempt: Some(1),
                max: Some(3),
                message: "m".into(),
            }),
            json!({ "kind": "transportStatus", "phase": "reconnecting", "attempt": 1, "max": 3, "message": "m" }),
        );
        assert_eq!(
            wire(UnifiedEvent::Status { state: "running".into() }),
            json!({ "kind": "status", "state": "running" }),
        );
        assert_eq!(
            wire(UnifiedEvent::PermissionRequest { request_id: 7, summary: "ok?".into() }),
            json!({ "kind": "permissionRequest", "requestId": 7, "summary": "ok?" }),
        );
        assert_eq!(
            wire(UnifiedEvent::RateLimits {
                used_percent: 12.5,
                resets_at: None,
                secondary_used_percent: None,
                secondary_resets_at: None,
                reached: None,
            }),
            json!({
                "kind": "rateLimits",
                "usedPercent": 12.5,
                "resetsAt": null,
                "secondaryUsedPercent": null,
                "secondaryResetsAt": null,
                "reached": null,
            }),
        );
    }
}
