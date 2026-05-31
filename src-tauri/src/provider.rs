//! External model provider settings for the Codex sidecar.
//!
//! Jasmine keeps this config local to the app config file and injects it into
//! `codex app-server` at spawn time through `-c` overrides. This avoids editing
//! the user's global `~/.codex/config.toml`.

use serde::{Deserialize, Serialize};
use std::process::Stdio;

const JASMINE_PROVIDER_KEY_ENV: &str = "JASMINE_EXTERNAL_PROVIDER_API_KEY";
const JASMINE_EXTERNAL_PROVIDER_PREFIX: &str = "jasmine_external";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProviderSettings {
    pub enabled: bool,
    pub active_id: Option<String>,
    pub profiles: Vec<ProviderProfile>,
    /// Legacy/current active-provider snapshot. Kept so older config files and
    /// older frontends still deserialize cleanly.
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub api_kind: ProviderApiKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProviderProfile {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub api_kind: ProviderApiKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderApiKind {
    ChatCompletions,
    Responses,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeProviderKind {
    Codex,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeProviderIdentity {
    pub key: String,
    pub name: String,
    pub kind: RuntimeProviderKind,
    pub codex_provider_id: Option<String>,
}

impl RuntimeProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::External => "external",
        }
    }
}

impl RuntimeProviderIdentity {
    pub fn codex() -> Self {
        Self {
            key: "codex:default".to_string(),
            name: "Codex".to_string(),
            kind: RuntimeProviderKind::Codex,
            codex_provider_id: None,
        }
    }

    pub fn is_external(&self) -> bool {
        self.kind == RuntimeProviderKind::External
    }
}

impl Default for ProviderApiKind {
    fn default() -> Self {
        Self::ChatCompletions
    }
}

impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            active_id: None,
            profiles: Vec::new(),
            name: "External provider".to_string(),
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            api_kind: ProviderApiKind::default(),
        }
    }
}

impl Default for ProviderProfile {
    fn default() -> Self {
        Self {
            id: "default-provider".to_string(),
            name: "External provider".to_string(),
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            api_kind: ProviderApiKind::default(),
        }
    }
}

impl ProviderProfile {
    fn display_name(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() {
            "External provider".to_string()
        } else {
            name.to_string()
        }
    }

    fn base_url(&self) -> Option<String> {
        let base_url = self.base_url.trim();
        normalize_base_url(base_url)
    }

    fn model(&self) -> Option<&str> {
        let model = self.model.trim();
        (!model.is_empty()).then_some(model)
    }

    fn api_key(&self) -> Option<&str> {
        let api_key = self.api_key.trim();
        (!api_key.is_empty()).then_some(api_key)
    }
}

impl ProviderSettings {
    fn active_profile(&self) -> Option<&ProviderProfile> {
        self.active_id
            .as_deref()
            .and_then(|id| self.profiles.iter().find(|profile| profile.id == id))
            .or_else(|| self.profiles.first())
    }

    pub fn active_display_name(&self) -> String {
        if let Some(profile) = self.active_profile() {
            return profile.display_name();
        }

        let name = self.name.trim();
        if name.is_empty() {
            "External provider".to_string()
        } else {
            name.to_string()
        }
    }

    fn base_url(&self) -> Option<String> {
        self.active_profile()
            .and_then(ProviderProfile::base_url)
            .or_else(|| normalize_base_url(self.base_url.trim()))
    }

    fn model(&self) -> Option<&str> {
        self.active_profile()
            .and_then(ProviderProfile::model)
            .or_else(|| {
                let model = self.model.trim();
                (!model.is_empty()).then_some(model)
            })
    }

    fn api_key(&self) -> Option<&str> {
        self.active_profile()
            .and_then(ProviderProfile::api_key)
            .or_else(|| {
                let api_key = self.api_key.trim();
                (!api_key.is_empty()).then_some(api_key)
            })
    }

    fn api_kind(&self) -> ProviderApiKind {
        self.active_profile()
            .map(|profile| profile.api_kind)
            .unwrap_or(self.api_kind)
    }

    pub fn runtime_identity(&self) -> RuntimeProviderIdentity {
        if !self.enabled {
            return RuntimeProviderIdentity::codex();
        }

        let Some(base_url) = self.base_url() else {
            return RuntimeProviderIdentity::codex();
        };

        let profile_id = self
            .active_profile()
            .map(|profile| profile.id.trim())
            .filter(|id| !id.is_empty())
            .unwrap_or("legacy");
        let api_kind = self.api_kind().as_str();
        let model = self.model().unwrap_or_default();
        let hash = provider_fingerprint(profile_id, &base_url, api_kind, model);
        let profile_key = safe_key_component(profile_id);

        RuntimeProviderIdentity {
            key: format!("external:{profile_key}:{hash}"),
            name: self.active_display_name(),
            kind: RuntimeProviderKind::External,
            codex_provider_id: Some(format!("{JASMINE_EXTERNAL_PROVIDER_PREFIX}_{hash}")),
        }
    }
}

impl ProviderApiKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
        }
    }
}

fn api_key_looks_like_url(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("http://") || trimmed.starts_with("https://")
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn config_arg(key: &str, value: impl Into<String>) -> String {
    format!("{key}={}", value.into())
}

fn provider_fingerprint(profile_id: &str, base_url: &str, api_kind: &str, model: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in ["jasmine-provider-v1", profile_id, base_url, api_kind, model] {
        hasher.update(part.as_bytes());
        hasher.update(&[0]);
    }
    hasher.finalize().to_hex().chars().take(12).collect()
}

fn safe_key_component(value: &str) -> String {
    let out: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "legacy".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn normalize_base_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let end = trimmed
        .find(|ch| ch == '?' || ch == '#')
        .unwrap_or(trimmed.len());
    let mut base = trimmed[..end].trim_end_matches('/').to_string();
    if !has_v1_path_segment(&base) {
        base.push_str("/v1");
    }
    Some(base)
}

fn has_v1_path_segment(value: &str) -> bool {
    let Some(scheme_end) = value.find("://") else {
        return false;
    };
    let after_authority = &value[scheme_end + 3..];
    let Some(path_start) = after_authority.find('/') else {
        return false;
    };
    after_authority[path_start + 1..]
        .split('/')
        .any(|segment| segment.eq_ignore_ascii_case("v1"))
}

fn codex_config_args_for_base_url(
    cfg: &ProviderSettings,
    base_url: &str,
    provider_id: &str,
) -> Vec<String> {
    if !cfg.enabled {
        return Vec::new();
    }

    let mut args = vec![
        config_arg("model_provider", toml_string(provider_id)),
        config_arg(
            &format!("model_providers.{provider_id}.name"),
            toml_string(&cfg.active_display_name()),
        ),
        config_arg(
            &format!("model_providers.{provider_id}.base_url"),
            toml_string(base_url),
        ),
        config_arg(
            &format!("model_providers.{provider_id}.wire_api"),
            // Codex 0.135 rejects the legacy "chat" wire API. Chat-only
            // providers are exposed to Codex through our local adapter.
            toml_string("responses"),
        ),
        config_arg(
            &format!("model_providers.{provider_id}.supports_websockets"),
            "false",
        ),
    ];

    if cfg.api_key().is_some() {
        args.push(config_arg(
            &format!("model_providers.{provider_id}.env_key"),
            toml_string(JASMINE_PROVIDER_KEY_ENV),
        ));
    }

    if let Some(model) = cfg.model() {
        args.push(config_arg("model", toml_string(model)));
    }

    args
}

pub fn codex_config_args(cfg: &ProviderSettings) -> Vec<String> {
    if !cfg.enabled {
        return Vec::new();
    }
    let Some(base_url) = cfg.base_url() else {
        return Vec::new();
    };
    let Some(provider_id) = cfg.runtime_identity().codex_provider_id else {
        return Vec::new();
    };
    codex_config_args_for_base_url(cfg, &base_url, &provider_id)
}

pub async fn apply_to_subprocess(
    cmd: &mut tokio::process::Command,
    cfg: &ProviderSettings,
) -> Result<Option<crate::provider_adapter::ProviderAdapterGuard>, String> {
    if !cfg.enabled {
        return Ok(None);
    }
    let Some(provider_base_url) = cfg.base_url() else {
        return Ok(None);
    };
    if cfg.api_key().is_some_and(api_key_looks_like_url) {
        return Err(
            "External provider API key looks like a URL. Put the provider URL in Base URL and the token in API key."
                .to_string(),
        );
    }
    let identity = cfg.runtime_identity();
    let Some(provider_id) = identity.codex_provider_id.clone() else {
        return Ok(None);
    };
    let adapter = if cfg.api_kind() == ProviderApiKind::ChatCompletions {
        Some(
            crate::provider_adapter::start_adapter(
                crate::provider_adapter::ProviderAdapterConfig {
                    base_url: provider_base_url.clone(),
                    api_key: cfg.api_key().map(str::to_string),
                    model: cfg.model().map(str::to_string),
                },
            )
            .await?,
        )
    } else {
        None
    };
    let codex_base_url = adapter
        .as_ref()
        .map(|guard| guard.base_url().to_string())
        .unwrap_or_else(|| provider_base_url.clone());

    let args = codex_config_args_for_base_url(cfg, &codex_base_url, &provider_id);
    if args.is_empty() {
        return Ok(adapter);
    }

    for arg in args {
        cmd.arg("-c").arg(arg);
    }

    if adapter.is_some() {
        cmd.env(JASMINE_PROVIDER_KEY_ENV, "jasmine-provider-adapter");
    } else if let Some(api_key) = cfg.api_key() {
        cmd.env(JASMINE_PROVIDER_KEY_ENV, api_key);
    } else {
        cmd.env_remove(JASMINE_PROVIDER_KEY_ENV);
    }

    // Prevent accidental leakage through inherited stdin when a provider fails
    // and the child emits an interactive prompt.
    cmd.stdin(Stdio::piped());

    tracing::info!(
        module = "provider",
        provider = %provider_id,
        provider_key = %identity.key,
        base_url = %provider_base_url,
        codex_base_url = %codex_base_url,
        model = cfg.model().unwrap_or_default(),
        api_kind = ?cfg.api_kind(),
        has_api_key = cfg.api_key().is_some(),
        "external provider injected into codex sidecar"
    );
    Ok(adapter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_id_from_args(args: &[String]) -> String {
        args.iter()
            .find_map(|arg| {
                arg.strip_prefix("model_provider=\"")
                    .and_then(|rest| rest.strip_suffix('"'))
            })
            .expect("model_provider arg")
            .to_string()
    }

    #[test]
    fn disabled_provider_has_no_overrides() {
        assert!(codex_config_args(&ProviderSettings::default()).is_empty());
    }

    #[test]
    fn enabled_provider_requires_base_url() {
        let cfg = ProviderSettings {
            enabled: true,
            api_key: "sk-test".to_string(),
            ..ProviderSettings::default()
        };

        assert!(codex_config_args(&cfg).is_empty());
    }

    #[test]
    fn provider_args_include_base_url_key_env_and_model() {
        let cfg = ProviderSettings {
            enabled: true,
            active_id: None,
            profiles: Vec::new(),
            name: "OpenRouter".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key: "sk-test".to_string(),
            model: "openai/gpt-5.3-codex".to_string(),
            api_kind: ProviderApiKind::Responses,
        };

        let args = codex_config_args(&cfg);
        let provider_id = provider_id_from_args(&args);

        assert!(provider_id.starts_with("jasmine_external_"));
        assert!(args.contains(&format!(
            "model_providers.{provider_id}.base_url=\"https://openrouter.ai/api/v1\""
        )));
        assert!(args.contains(&format!(
            "model_providers.{provider_id}.wire_api=\"responses\""
        )));
        assert!(args.contains(&format!(
            "model_providers.{provider_id}.env_key=\"JASMINE_EXTERNAL_PROVIDER_API_KEY\""
        )));
        assert!(args.contains(&"model=\"openai/gpt-5.3-codex\"".to_string()));
        assert!(!args.iter().any(|arg| arg.contains("sk-test")));
    }

    #[test]
    fn active_profile_overrides_legacy_snapshot() {
        let cfg = ProviderSettings {
            enabled: true,
            active_id: Some("modelsrouter".to_string()),
            profiles: vec![ProviderProfile {
                id: "modelsrouter".to_string(),
                name: "ModelsRouter".to_string(),
                base_url: "https://api.modelsrouter.com".to_string(),
                api_key: "sk-profile".to_string(),
                model: "gpt-5.5".to_string(),
                api_kind: ProviderApiKind::Responses,
            }],
            name: "Legacy".to_string(),
            base_url: "https://legacy.example.com/v1".to_string(),
            api_key: "sk-legacy".to_string(),
            model: "legacy-model".to_string(),
            api_kind: ProviderApiKind::ChatCompletions,
        };

        let args = codex_config_args(&cfg);
        let provider_id = provider_id_from_args(&args);

        assert!(args.contains(&format!(
            "model_providers.{provider_id}.name=\"ModelsRouter\""
        )));
        assert!(args.contains(&format!(
            "model_providers.{provider_id}.base_url=\"https://api.modelsrouter.com/v1\""
        )));
        assert!(args.contains(&"model=\"gpt-5.5\"".to_string()));
        assert_eq!(cfg.api_kind(), ProviderApiKind::Responses);
    }

    #[test]
    fn runtime_identity_is_per_active_profile_without_api_key() {
        let mut cfg = ProviderSettings {
            enabled: true,
            active_id: Some("a".to_string()),
            profiles: vec![
                ProviderProfile {
                    id: "a".to_string(),
                    name: "A".to_string(),
                    base_url: "https://api.example.com".to_string(),
                    api_key: "sk-secret-a".to_string(),
                    model: "gpt-a".to_string(),
                    api_kind: ProviderApiKind::Responses,
                },
                ProviderProfile {
                    id: "b".to_string(),
                    name: "B".to_string(),
                    base_url: "https://api.example.com".to_string(),
                    api_key: "sk-secret-b".to_string(),
                    model: "gpt-a".to_string(),
                    api_kind: ProviderApiKind::Responses,
                },
            ],
            ..ProviderSettings::default()
        };

        let a = cfg.runtime_identity();
        cfg.active_id = Some("b".to_string());
        let b = cfg.runtime_identity();

        assert_ne!(a.key, b.key);
        assert_ne!(a.codex_provider_id, b.codex_provider_id);
        assert!(!a.key.contains("sk-secret"));
        assert!(!b.key.contains("sk-secret"));
    }

    #[test]
    fn provider_defaults_to_chat_completions_adapter() {
        assert_eq!(
            ProviderSettings::default().api_kind,
            ProviderApiKind::ChatCompletions
        );
    }

    #[test]
    fn api_key_url_detection_catches_base_urls() {
        assert!(api_key_looks_like_url("https://api.modelsrouter.com"));
        assert!(!api_key_looks_like_url("sk-test"));
    }

    #[test]
    fn provider_base_url_adds_v1_for_root_urls() {
        let cfg = ProviderSettings {
            enabled: true,
            base_url: "https://api.modelsrouter.com".to_string(),
            ..ProviderSettings::default()
        };

        let args = codex_config_args(&cfg);
        let provider_id = provider_id_from_args(&args);

        assert!(args.contains(&format!(
            "model_providers.{provider_id}.base_url=\"https://api.modelsrouter.com/v1\""
        )));
    }

    #[test]
    fn provider_base_url_adds_v1_after_existing_path() {
        let cfg = ProviderSettings {
            enabled: true,
            base_url: "https://example.com/openai/".to_string(),
            ..ProviderSettings::default()
        };

        let args = codex_config_args(&cfg);
        let provider_id = provider_id_from_args(&args);

        assert!(args.contains(&format!(
            "model_providers.{provider_id}.base_url=\"https://example.com/openai/v1\""
        )));
    }

    #[test]
    fn provider_base_url_keeps_existing_v1_segment() {
        let cfg = ProviderSettings {
            enabled: true,
            base_url: "https://example.com/api/v1/".to_string(),
            ..ProviderSettings::default()
        };

        let args = codex_config_args(&cfg);
        let provider_id = provider_id_from_args(&args);

        assert!(args.contains(&format!(
            "model_providers.{provider_id}.base_url=\"https://example.com/api/v1\""
        )));
    }
}
