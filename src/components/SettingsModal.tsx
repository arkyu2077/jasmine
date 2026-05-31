import { useEffect, useRef, useState } from "react";
import { AlertCircle, CheckCircle2, ChevronLeft, Copy, Eye, EyeOff, Loader2, Plus, Trash2, X } from "lucide-react";
import { useSettingsStore } from "../store/settings";
import { useT, useLocaleStore, type LocaleChoice } from "../i18n/locale";
import { ipc } from "../lib/ipc";
import type { MsgKey } from "../i18n/messages";
import type { ProviderSettings, ProxyProbeResult, ProxySettings } from "../types";
import { isValidProviderBaseUrl, normalizeProviderBaseUrl, providerApiKeyLooksLikeUrl } from "../lib/provider";

const PROTOCOLS: ProxySettings["protocol"][] = ["http", "socks5"];

const isValidHost = (h: string) => {
  const v = h.trim();
  return !!v && !v.includes("://") && !v.includes("@") && !v.includes("/") && v.length <= 253;
};
const isValidPort = (p: number) => p >= 1 && p <= 65535;

type ProxyProbeState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "ok"; result: ProxyProbeResult }
  | { status: "error"; result: ProxyProbeResult | null; detail: string | null };

function proxyProbeMessageKey(kind: string): MsgKey {
  switch (kind) {
    case "invalid_proxy":
      return "settings.proxyProbe.invalid";
    case "proxy_unreachable":
      return "settings.proxyProbe.unreachable";
    case "timeout":
      return "settings.proxyProbe.timeout";
    case "protocol_mismatch":
      return "settings.proxyProbe.protocolMismatch";
    case "proxy_auth_required":
      return "settings.proxyProbe.authRequired";
    case "upstream_unreachable":
    case "internet_unreachable":
    case "network_error":
    case "unexpected_status":
    case "captive_portal":
      return "settings.proxyProbe.upstreamBlocked";
    default:
      return "settings.proxyProbe.error";
  }
}

/** App settings: UI language and the network proxy (injected into the Codex
 *  sidecar). Everything applies live — there is no Save button. Language is
 *  instant; proxy edits commit on change/blur and restart the session. */
export function SettingsModal({ onClose }: { onClose: () => void }) {
  const config = useSettingsStore((s) => s.config);
  const loaded = useSettingsStore((s) => s.loaded);
  const proxyApplying = useSettingsStore((s) => s.proxyApplying);
  const providerApplying = useSettingsStore((s) => s.providerApplying);
  const localeChoice = useLocaleStore((s) => s.choice);
  const proxy = config.proxy;
  const provider = config.provider;
  const providerProfiles = provider.profiles.length ? provider.profiles : [];
  const activeProviderId = provider.active_id ?? providerProfiles[0]?.id ?? "";
  const [proxyProbe, setProxyProbe] = useState<ProxyProbeState>({ status: "idle" });
  const [providerView, setProviderView] = useState<"list" | "edit">("list");
  const [showProviderKey, setShowProviderKey] = useState(false);
  const proxyProbeGenerationRef = useRef(0);
  const t = useT();

  useEffect(() => {
    if (!loaded) void useSettingsStore.getState().load();
  }, [loaded]);

  useEffect(() => {
    if (!provider.enabled) setProviderView("list");
  }, [provider.enabled]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const setProxy = (patch: Partial<ProxySettings>) => useSettingsStore.getState().setProxy(patch);
  const setProvider = (patch: Partial<ProviderSettings>) => useSettingsStore.getState().setProvider(patch);
  const addProviderProfile = () => useSettingsStore.getState().addProviderProfile();
  const duplicateProviderProfile = () => useSettingsStore.getState().duplicateProviderProfile();
  const selectProviderProfile = (id: string) => useSettingsStore.getState().selectProviderProfile(id);
  const removeProviderProfile = (id: string) => useSettingsStore.getState().removeProviderProfile(id);
  const hostOk = isValidHost(proxy.host);
  const portOk = isValidPort(proxy.port);
  const providerBaseOk = isValidProviderBaseUrl(provider.base_url);
  const normalizedProviderBaseUrl = providerBaseOk ? normalizeProviderBaseUrl(provider.base_url) : "";
  const providerBaseWillNormalize =
    providerBaseOk && provider.base_url.trim() !== normalizedProviderBaseUrl;
  const providerKeyLooksLikeUrl = providerApiKeyLooksLikeUrl(provider.api_key);

  useEffect(() => {
    if (!proxy.enabled || !hostOk || !portOk) {
      proxyProbeGenerationRef.current += 1;
      setProxyProbe({ status: "idle" });
      return;
    }

    const generation = proxyProbeGenerationRef.current + 1;
    proxyProbeGenerationRef.current = generation;
    setProxyProbe({ status: "checking" });

    const timer = window.setTimeout(() => {
      void ipc
        .probeProxy(proxy.protocol, proxy.host, proxy.port)
        .then((result) => {
          if (proxyProbeGenerationRef.current !== generation) return;
          if (result.ok) {
            setProxyProbe({ status: "ok", result });
          } else {
            setProxyProbe({ status: "error", result, detail: result.detail });
          }
        })
        .catch((error) => {
          if (proxyProbeGenerationRef.current !== generation) return;
          setProxyProbe({
            status: "error",
            result: null,
            detail: error instanceof Error ? error.message : String(error),
          });
        });
    }, 250);

    return () => {
      window.clearTimeout(timer);
      proxyProbeGenerationRef.current += 1;
    };
  }, [hostOk, portOk, proxy.enabled, proxy.host, proxy.port, proxy.protocol]);

  // Apply live: persist + restart the session, but only when the (now-current)
  // proxy is coherent so we never spawn the sidecar with a half-typed endpoint.
  const commit = () => {
    const p = useSettingsStore.getState().config.proxy;
    if (!p.enabled || (isValidHost(p.host) && isValidPort(p.port))) {
      void useSettingsStore.getState().commitProxy();
    }
  };
  const commitProvider = () => {
    const p = useSettingsStore.getState().config.provider;
    if (!p.enabled || isValidProviderBaseUrl(p.base_url)) {
      void useSettingsStore.getState().commitProvider();
    }
  };
  const proxyProbeDetail =
    proxyProbe.status === "ok"
      ? proxyProbe.result.detail
      : proxyProbe.status === "error"
        ? proxyProbe.detail
        : undefined;

  return (
    <div className="cm-modal-backdrop" onClick={onClose}>
      <div className="cm-modal" onClick={(e) => e.stopPropagation()} role="dialog" aria-modal="true">
        <div className="cm-modal__head">
          <h2 className="cm-modal__title">{t("settings.title")}</h2>
          <button className="cm-modal__close" onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        <div className="cm-modal__body">
          <section className="cm-set-section">
            <h3 className="cm-set-section__title">{t("settings.general")}</h3>
            <div className="cm-set-card">
              <div className="cm-set-row">
                <span className="cm-set-row__label">{t("settings.language")}</span>
                <select
                  className="cm-select"
                  value={localeChoice}
                  onChange={(e) => useLocaleStore.getState().setChoice(e.target.value as LocaleChoice)}
                >
                  <option value="system">{t("settings.language.system")}</option>
                  <option value="zh">中文</option>
                  <option value="en">English</option>
                </select>
              </div>
            </div>
          </section>

          <section className="cm-set-section">
            <div className="cm-set-section__head">
              <h3 className="cm-set-section__title">{t("settings.tray")}</h3>
              <button
                type="button"
                className={`cm-toggle${config.close_to_tray ? " is-on" : ""}`}
                role="switch"
                aria-checked={config.close_to_tray}
                aria-label={t("settings.tray")}
                onClick={() => void useSettingsStore.getState().setCloseToTray(!config.close_to_tray)}
              >
                <span className="cm-toggle__knob" />
              </button>
            </div>
            <p className="cm-set-section__desc">{t("settings.trayDesc")}</p>
          </section>

          <section className="cm-set-section">
            <div className="cm-set-section__head">
              <h3 className="cm-set-section__title">{t("settings.proxy")}</h3>
              <button
                type="button"
                className={`cm-toggle${proxy.enabled ? " is-on" : ""}`}
                role="switch"
                aria-checked={proxy.enabled}
                aria-label={t("settings.enableProxy")}
                onClick={() => {
                  setProxy({ enabled: !proxy.enabled });
                  commit();
                }}
              >
                <span className="cm-toggle__knob" />
              </button>
            </div>
            <p className="cm-set-section__desc">{t("settings.proxyDesc")}</p>

            {proxy.enabled && (
              <>
                <div className="cm-set-card">
                  <div className="cm-set-row">
                    <span className="cm-set-row__label">{t("settings.protocol")}</span>
                    <select
                      className="cm-select"
                      value={proxy.protocol}
                      onChange={(e) => {
                        setProxy({ protocol: e.target.value as ProxySettings["protocol"] });
                        commit();
                      }}
                    >
                      {PROTOCOLS.map((p) => (
                        <option key={p} value={p}>
                          {p}
                        </option>
                      ))}
                    </select>
                  </div>

                  <div className="cm-set-row">
                    <span className="cm-set-row__label">{t("settings.address")}</span>
                    <div className="cm-set-endpoint">
                      <input
                        className={`cm-input cm-input--host${hostOk ? "" : " is-invalid"}`}
                        value={proxy.host}
                        placeholder="127.0.0.1"
                        spellCheck={false}
                        aria-label={t("settings.host")}
                        onChange={(e) => setProxy({ host: e.target.value })}
                        onBlur={commit}
                      />
                      <span className="cm-set-endpoint__colon">:</span>
                      <input
                        className={`cm-input cm-input--port${portOk ? "" : " is-invalid"}`}
                        type="number"
                        min={1}
                        max={65535}
                        value={proxy.port}
                        aria-label={t("settings.port")}
                        onChange={(e) => setProxy({ port: Number(e.target.value) || 0 })}
                        onBlur={commit}
                      />
                    </div>
                  </div>
                </div>

                {!hostOk || !portOk ? (
                  <p className="cm-set-hint cm-set-hint--err">{t("settings.invalid")}</p>
                ) : proxyApplying ? (
                  <p className="cm-set-hint">{t("settings.proxyApplying")}</p>
                ) : proxyProbe.status !== "idle" ? (
                  <div
                    className={`cm-set-probe cm-set-probe--${proxyProbe.status}`}
                    title={proxyProbeDetail ?? undefined}
                    aria-live="polite"
                  >
                    {proxyProbe.status === "checking" ? (
                      <Loader2 className="cm-set-probe__icon cm-set-probe__spin" size={14} />
                    ) : proxyProbe.status === "ok" ? (
                      <CheckCircle2 className="cm-set-probe__icon" size={14} />
                    ) : (
                      <AlertCircle className="cm-set-probe__icon" size={14} />
                    )}
                    <span className="cm-set-probe__text">
                      {proxyProbe.status === "checking"
                        ? t("settings.proxyProbe.checking")
                        : proxyProbe.status === "ok"
                          ? t("settings.proxyProbe.ok")
                          : t(proxyProbeMessageKey(proxyProbe.result?.kind ?? "error"))}
                    </span>
                  </div>
                ) : null}
              </>
            )}
          </section>

          <section className="cm-set-section">
            <div className="cm-set-section__head">
              <h3 className="cm-set-section__title">{t("settings.provider")}</h3>
              <button
                type="button"
                className={`cm-toggle${provider.enabled ? " is-on" : ""}`}
                role="switch"
                aria-checked={provider.enabled}
                aria-label={t("settings.enableProvider")}
                onClick={() => {
                  setProvider({ enabled: !provider.enabled });
                  commitProvider();
                }}
              >
                <span className="cm-toggle__knob" />
              </button>
            </div>
            <p className="cm-set-section__desc">{t("settings.providerDesc")}</p>

            {provider.enabled && (
              <>
                {providerView === "list" ? (
                  <div className="cm-provider-screen">
                    <p className="cm-set-hint">
                      {t("settings.providerManagerHint", { count: providerProfiles.length })}
                    </p>
                    <div className="cm-provider-list" aria-label={t("settings.providerList")}>
                    <div className="cm-provider-list__head">
                      <span>{t("settings.providerList")}</span>
                      <button
                        type="button"
                        className="cm-mini-action"
                        onClick={() => {
                          addProviderProfile();
                          commitProvider();
                          setProviderView("edit");
                        }}
                      >
                        <Plus size={14} />
                        {t("settings.providerAddShort")}
                      </button>
                    </div>
                    <div className="cm-provider-list__items">
                      {providerProfiles.map((profile) => {
                        const isActive = profile.id === activeProviderId;
                        const name = profile.name.trim() || t("settings.providerUnnamed");
                        const protocol =
                          profile.api_kind === "responses"
                            ? t("agent.provider.protocol.responses")
                            : t("agent.provider.protocol.chatCompletions");
                        return (
                          <div
                            key={profile.id}
                            role="button"
                            tabIndex={0}
                            className={`cm-provider-item${isActive ? " is-active" : ""}`}
                            onClick={() => {
                              selectProviderProfile(profile.id);
                              commitProvider();
                              setProviderView("edit");
                            }}
                            onKeyDown={(e) => {
                              if (e.key === "Enter" || e.key === " ") {
                                e.preventDefault();
                                selectProviderProfile(profile.id);
                                commitProvider();
                                setProviderView("edit");
                              }
                            }}
                          >
                            <span className="cm-provider-item__main">
                              <span className="cm-provider-item__name">{name}</span>
                              <span className="cm-provider-item__meta">{profile.model || t("settings.providerNoModel")}</span>
                            </span>
                            <span className="cm-provider-item__sub">{protocol}</span>
                            <span className="cm-provider-item__url" title={profile.base_url}>
                              {profile.base_url || t("settings.providerNoBaseUrl")}
                            </span>
                            <button
                              type="button"
                              disabled={providerProfiles.length <= 1}
                              className="cm-provider-item__delete"
                              aria-label={t("settings.providerDelete")}
                              title={t("settings.providerDelete")}
                              onClick={(e) => {
                                e.stopPropagation();
                                removeProviderProfile(profile.id);
                                commitProvider();
                              }}
                            >
                              <Trash2 size={13} />
                            </button>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                  </div>
                ) : (
                  <div className="cm-provider-edit-page">
                    <div className="cm-provider-edit-head">
                      <button
                        type="button"
                        className="cm-back-btn"
                        onClick={() => setProviderView("list")}
                      >
                        <ChevronLeft size={16} />
                        {t("settings.providerBack")}
                      </button>
                      <div className="cm-provider-edit-head__title">
                        <span className="cm-provider-detail__eyebrow">{t("settings.providerEditing")}</span>
                        <span className="cm-provider-detail__title">
                          {provider.name.trim() || t("settings.providerUnnamed")}
                        </span>
                      </div>
                      <div className="cm-provider-edit-head__actions">
                        <button
                          type="button"
                          className="cm-mini-action"
                          onClick={() => {
                            duplicateProviderProfile();
                            commitProvider();
                          }}
                        >
                          <Copy size={14} />
                          {t("settings.providerDuplicate")}
                        </button>
                        <button
                          type="button"
                          className="cm-mini-action cm-mini-action--danger"
                          disabled={providerProfiles.length <= 1}
                          onClick={() => {
                            removeProviderProfile(activeProviderId);
                            commitProvider();
                            setProviderView("list");
                          }}
                        >
                          <Trash2 size={14} />
                          {t("settings.providerDeleteShort")}
                        </button>
                      </div>
                    </div>
                    <p className="cm-set-hint">{t("settings.providerEditHint")}</p>
                    <div className="cm-set-card cm-provider-detail__form">
                      <div className="cm-set-row">
                        <span className="cm-set-row__label">{t("settings.providerName")}</span>
                        <input
                          className="cm-input cm-input--wide"
                          value={provider.name}
                          placeholder="OpenRouter"
                          spellCheck={false}
                          aria-label={t("settings.providerName")}
                          onChange={(e) => setProvider({ name: e.target.value })}
                          onBlur={commitProvider}
                        />
                      </div>

                      <div className="cm-set-row">
                        <span className="cm-set-row__label">{t("settings.providerBaseUrl")}</span>
                        <input
                          className={`cm-input cm-input--wide${providerBaseOk ? "" : " is-invalid"}`}
                          value={provider.base_url}
                          placeholder="https://api.example.com/v1"
                          spellCheck={false}
                          aria-label={t("settings.providerBaseUrl")}
                          onChange={(e) => setProvider({ base_url: e.target.value })}
                          onBlur={(e) => {
                            setProvider({ base_url: normalizeProviderBaseUrl(e.currentTarget.value) });
                            commitProvider();
                          }}
                        />
                      </div>

                      <div className="cm-set-row">
                        <span className="cm-set-row__label">{t("settings.providerApiKind")}</span>
                        <select
                          className="cm-select"
                          value={provider.api_kind}
                          aria-label={t("settings.providerApiKind")}
                          onChange={(e) => {
                            setProvider({ api_kind: e.target.value as ProviderSettings["api_kind"] });
                            commitProvider();
                          }}
                        >
                          <option value="chat_completions">{t("settings.providerApiKind.chatCompletions")}</option>
                          <option value="responses">{t("settings.providerApiKind.responses")}</option>
                        </select>
                      </div>

                      <div className="cm-set-row">
                        <span className="cm-set-row__label">{t("settings.providerModel")}</span>
                        <input
                          className="cm-input cm-input--wide"
                          value={provider.model}
                          placeholder="gpt-5.3-codex"
                          spellCheck={false}
                          aria-label={t("settings.providerModel")}
                          onChange={(e) => setProvider({ model: e.target.value })}
                          onBlur={commitProvider}
                        />
                      </div>

                      <div className="cm-set-row">
                        <span className="cm-set-row__label">{t("settings.providerApiKey")}</span>
                        <div className="cm-secret">
                          <input
                            className={`cm-input cm-input--secret${providerKeyLooksLikeUrl ? " is-invalid" : ""}`}
                            type={showProviderKey ? "text" : "password"}
                            value={provider.api_key}
                            placeholder="sk-..."
                            spellCheck={false}
                            aria-label={t("settings.providerApiKey")}
                            onChange={(e) => setProvider({ api_key: e.target.value })}
                            onBlur={commitProvider}
                          />
                          <button
                            type="button"
                            className="cm-secret__toggle"
                            aria-label={t(showProviderKey ? "settings.hideSecret" : "settings.showSecret")}
                            title={t(showProviderKey ? "settings.hideSecret" : "settings.showSecret")}
                            onMouseDown={(e) => e.preventDefault()}
                            onClick={() => setShowProviderKey((v) => !v)}
                          >
                            {showProviderKey ? <EyeOff size={15} /> : <Eye size={15} />}
                          </button>
                        </div>
                      </div>
                    </div>
                  </div>
                )}

                {providerView !== "edit" ? null : !providerBaseOk ? (
                  <p className="cm-set-hint cm-set-hint--err">{t("settings.providerInvalid")}</p>
                ) : providerKeyLooksLikeUrl ? (
                  <p className="cm-set-hint cm-set-hint--err">{t("settings.providerKeyLooksLikeUrl")}</p>
                ) : providerApplying ? (
                  <p className="cm-set-hint">{t("settings.providerApplying")}</p>
                ) : providerBaseWillNormalize ? (
                  <p className="cm-set-hint">{t("settings.providerBaseUrlWillUse", { url: normalizedProviderBaseUrl })}</p>
                ) : (
                  <p className="cm-set-hint">{t("settings.providerKeyHint")}</p>
                )}
              </>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}
