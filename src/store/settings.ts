import { create } from "zustand";
import { ipc } from "../lib/ipc";
import { useBoardStore } from "./board";
import type { AppConfig, ProviderProfile, ProviderSettings, ProxySettings } from "../types";
import { isValidProviderBaseUrl, normalizeProviderBaseUrl, providerApiKeyLooksLikeUrl } from "../lib/provider";

const DEFAULT_PROXY: ProxySettings = {
  enabled: false,
  protocol: "http",
  host: "127.0.0.1",
  port: 7897,
};

const DEFAULT_PROVIDER_PROFILE_ID = "default-provider";

const DEFAULT_PROVIDER_PROFILE: Omit<ProviderProfile, "id"> = {
  name: "External provider",
  base_url: "",
  api_key: "",
  model: "",
  api_kind: "chat_completions",
};

const DEFAULT_PROVIDER: ProviderSettings = {
  enabled: false,
  active_id: DEFAULT_PROVIDER_PROFILE_ID,
  profiles: [{ id: DEFAULT_PROVIDER_PROFILE_ID, ...DEFAULT_PROVIDER_PROFILE }],
  ...DEFAULT_PROVIDER_PROFILE,
};

function defaultConfig(): AppConfig {
  return {
    proxy: { ...DEFAULT_PROXY },
    provider: cloneProvider(DEFAULT_PROVIDER),
    telemetry_opt_out: false,
    last_telemetry_date: null,
    close_to_tray: true,
  };
}

const sameProxy = (a: ProxySettings, b: ProxySettings) =>
  a.enabled === b.enabled && a.protocol === b.protocol && a.host === b.host && a.port === b.port;

const sameProvider = (a: ProviderSettings, b: ProviderSettings) =>
  a.enabled === b.enabled &&
  a.active_id === b.active_id &&
  a.name === b.name &&
  a.base_url === b.base_url &&
  a.api_key === b.api_key &&
  a.model === b.model &&
  a.api_kind === b.api_kind &&
  a.profiles.length === b.profiles.length &&
  a.profiles.every((p, i) => sameProviderProfile(p, b.profiles[i]));

const sameProviderProfile = (a: ProviderProfile, b: ProviderProfile | undefined) =>
  !!b &&
  a.id === b.id &&
  a.name === b.name &&
  a.base_url === b.base_url &&
  a.api_key === b.api_key &&
  a.model === b.model &&
  a.api_kind === b.api_kind;

const proxyProtocol = (value: unknown): ProxySettings["protocol"] =>
  value === "socks5" ? "socks5" : "http";

function cloneProviderProfile(profile: ProviderProfile): ProviderProfile {
  return { ...profile };
}

function cloneProvider(provider: ProviderSettings): ProviderSettings {
  return { ...provider, profiles: provider.profiles.map(cloneProviderProfile) };
}

function newProviderId() {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) return crypto.randomUUID();
  return `provider-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

function providerApiKind(value: unknown): ProviderProfile["api_kind"] {
  return value === "responses" ? "responses" : "chat_completions";
}

type RawProviderProfile = Partial<ProviderProfile> & { id?: unknown };
type RawProviderSettings = Partial<ProviderSettings> & {
  profiles?: RawProviderProfile[] | null;
};

function normalizeProviderProfile(
  value: RawProviderProfile | undefined | null,
  fallbackId: string,
): ProviderProfile {
  const baseUrl = typeof value?.base_url === "string" ? value.base_url : "";
  return {
    id: typeof value?.id === "string" && value.id.trim() ? value.id : fallbackId,
    name: typeof value?.name === "string" ? value.name : DEFAULT_PROVIDER_PROFILE.name,
    base_url: normalizeProviderBaseUrl(baseUrl),
    api_key: typeof value?.api_key === "string" ? value.api_key : "",
    model: typeof value?.model === "string" ? value.model : "",
    api_kind: providerApiKind(value?.api_kind),
  };
}

function ensureUniqueProfileIds(profiles: ProviderProfile[]): ProviderProfile[] {
  const seen = new Set<string>();
  return profiles.map((profile, index) => {
    let id = profile.id.trim() || `provider-${index + 1}`;
    while (seen.has(id)) id = `${id}-${index + 1}`;
    seen.add(id);
    return { ...profile, id };
  });
}

function activeProviderProfile(provider: ProviderSettings): ProviderProfile {
  return (
    provider.profiles.find((profile) => profile.id === provider.active_id) ??
    provider.profiles[0] ??
    { id: DEFAULT_PROVIDER_PROFILE_ID, ...DEFAULT_PROVIDER_PROFILE }
  );
}

function providerWithActiveSnapshot(
  provider: Omit<ProviderSettings, "name" | "base_url" | "api_key" | "model" | "api_kind"> & Partial<ProviderProfile>,
): ProviderSettings {
  const active =
    provider.profiles.find((profile) => profile.id === provider.active_id) ??
    provider.profiles[0] ??
    { id: DEFAULT_PROVIDER_PROFILE_ID, ...DEFAULT_PROVIDER_PROFILE };
  return {
    enabled: provider.enabled,
    active_id: active.id,
    profiles: provider.profiles,
    name: active.name,
    base_url: active.base_url,
    api_key: active.api_key,
    model: active.model,
    api_kind: active.api_kind,
  };
}

function normalizeProvider(value: RawProviderSettings | undefined | null): ProviderSettings {
  const rawProfiles = Array.isArray(value?.profiles) ? value.profiles : [];
  let profiles = rawProfiles.map((profile, index) =>
    normalizeProviderProfile(profile, `provider-${index + 1}`),
  );

  if (profiles.length === 0) {
    profiles = [
      normalizeProviderProfile(
        {
          id:
            typeof value?.active_id === "string" && value.active_id.trim()
              ? value.active_id
              : DEFAULT_PROVIDER_PROFILE_ID,
          name: value?.name,
          base_url: value?.base_url,
          api_key: value?.api_key,
          model: value?.model,
          api_kind: value?.api_kind,
        },
        DEFAULT_PROVIDER_PROFILE_ID,
      ),
    ];
  }

  profiles = ensureUniqueProfileIds(profiles);
  const requestedActiveId =
    typeof value?.active_id === "string" && value.active_id.trim() ? value.active_id : null;
  const active = profiles.find((profile) => profile.id === requestedActiveId) ?? profiles[0];
  return providerWithActiveSnapshot({
    enabled: !!value?.enabled,
    active_id: active.id,
    profiles,
  });
}

function patchActiveProvider(provider: ProviderSettings, patch: Partial<ProviderProfile>): ProviderSettings {
  const current = activeProviderProfile(provider);
  const nextActive: ProviderProfile = {
    ...current,
  };
  if (patch.name !== undefined) nextActive.name = patch.name;
  if (patch.base_url !== undefined) nextActive.base_url = patch.base_url;
  if (patch.api_key !== undefined) nextActive.api_key = patch.api_key;
  if (patch.model !== undefined) nextActive.model = patch.model;
  if (patch.api_kind !== undefined) nextActive.api_kind = providerApiKind(patch.api_kind);
  const profiles = provider.profiles.map((profile) =>
    profile.id === current.id ? nextActive : profile,
  );
  return providerWithActiveSnapshot({ ...provider, profiles, active_id: current.id });
}

function selectProviderProfile(provider: ProviderSettings, profileId: string): ProviderSettings {
  const nextActive = provider.profiles.find((profile) => profile.id === profileId);
  if (!nextActive) return provider;
  return providerWithActiveSnapshot({ ...provider, active_id: nextActive.id });
}

function addBlankProviderProfile(provider: ProviderSettings): ProviderSettings {
  const id = newProviderId();
  const profile: ProviderProfile = {
    id,
    ...DEFAULT_PROVIDER_PROFILE,
    name: `Provider ${provider.profiles.length + 1}`,
  };
  return providerWithActiveSnapshot({
    ...provider,
    active_id: id,
    profiles: [...provider.profiles, profile],
  });
}

function duplicateProviderProfile(provider: ProviderSettings): ProviderSettings {
  const id = newProviderId();
  const current = activeProviderProfile(provider);
  const baseName = current.name.trim() || DEFAULT_PROVIDER_PROFILE.name;
  const profile: ProviderProfile = {
    ...current,
    id,
    name: `${baseName} copy`,
  };
  return providerWithActiveSnapshot({
    ...provider,
    active_id: id,
    profiles: [...provider.profiles, profile],
  });
}

function removeProviderProfile(provider: ProviderSettings, profileId: string): ProviderSettings {
  if (provider.profiles.length <= 1) return provider;
  const index = provider.profiles.findIndex((profile) => profile.id === profileId);
  if (index < 0) return provider;
  const profiles = provider.profiles.filter((profile) => profile.id !== profileId);
  const nextActiveId =
    provider.active_id === profileId
      ? profiles[Math.max(0, index - 1)]?.id ?? profiles[0].id
      : provider.active_id;
  return providerWithActiveSnapshot({ ...provider, profiles, active_id: nextActiveId });
}

function providerPatchTouchesActiveProfile(patch: Partial<ProviderSettings>) {
  return (
    patch.name !== undefined ||
    patch.base_url !== undefined ||
    patch.api_key !== undefined ||
    patch.model !== undefined ||
    patch.api_kind !== undefined
  );
}

function applyProviderPatch(provider: ProviderSettings, patch: Partial<ProviderSettings>): ProviderSettings {
  const enabled = patch.enabled ?? provider.enabled;
  let next = providerWithActiveSnapshot({
    ...provider,
    enabled,
    profiles: patch.profiles ? ensureUniqueProfileIds(patch.profiles.map(cloneProviderProfile)) : provider.profiles,
    active_id: patch.active_id !== undefined ? patch.active_id : provider.active_id,
  });

  if (patch.active_id !== undefined) {
    next = selectProviderProfile(next, patch.active_id ?? "");
  }

  if (providerPatchTouchesActiveProfile(patch)) {
    next = patchActiveProvider(next, {
      name: patch.name,
      base_url: patch.base_url,
      api_key: patch.api_key,
      model: patch.model,
      api_kind: patch.api_kind,
    });
  }

  return next;
}

function providerCanApply(provider: ProviderSettings) {
  if (!provider.enabled) return true;
  return isValidProviderBaseUrl(provider.base_url) && !providerApiKeyLooksLikeUrl(provider.api_key);
}

function providerTitle(provider: ProviderSettings) {
  return provider.enabled ? provider.name.trim() || "External provider" : "Codex";
}

function providerHasChangedByNormalization(original: ProviderSettings, normalized: ProviderSettings) {
  return !sameProvider(original, normalized);
}

async function restartActiveSession() {
  const boardId = useBoardStore.getState().boardId;
  if (boardId) await ipc.stopSession(boardId);
}

async function prepareProviderRuntimeSession(title: string) {
  const boardId = useBoardStore.getState().boardId;
  if (!boardId) return;
  await ipc.prepareRuntimeSession(boardId, title);
  await ipc.stopSession(boardId);
}

interface SettingsState {
  config: AppConfig;
  loaded: boolean;
  /** Last proxy state persisted + applied to the sidecar — used to skip no-op restarts. */
  appliedProxy: ProxySettings;
  /** Last provider state persisted + applied to the sidecar — used to skip no-op restarts. */
  appliedProvider: ProviderSettings;
  /** Transient: a proxy commit is persisting + restarting the session (inline feedback). */
  proxyApplying: boolean;
  /** Transient: a provider commit is persisting + restarting the session. */
  providerApplying: boolean;
  /** Bumped after a commit so the active Codex session restarts. Runtime
   *  settings are injected at sidecar spawn — App.tsx watches this nonce. */
  restartNonce: number;

  load: () => Promise<void>;
  /** Edit the in-memory proxy (controlled inputs). Does not persist or restart. */
  setProxy: (patch: Partial<ProxySettings>) => void;
  /** Persist the current proxy and restart the active session so it takes effect.
   *  No-ops when nothing changed since the last apply. There is no Save button —
   *  call this on commit (toggle/select change, input blur); settings apply live. */
  commitProxy: () => Promise<void>;
  /** Edit the in-memory provider. Does not persist or restart until commit. */
  setProvider: (patch: Partial<ProviderSettings>) => void;
  /** Add/select/remove provider profiles in memory. Commit still controls persistence/runtime apply. */
  addProviderProfile: () => void;
  duplicateProviderProfile: () => void;
  selectProviderProfile: (profileId: string) => void;
  removeProviderProfile: (profileId: string) => void;
  /** Persist the provider and restart the active Codex session so it respawns
   *  with the new base URL / API key / model overrides. */
  commitProvider: () => Promise<void>;
  /** Toggle telemetry opt-out and persist immediately (no restart needed —
   *  the next bootDailyPing reads the fresh value). */
  setTelemetryOptOut: (value: boolean) => Promise<void>;
  /** Toggle close-to-tray and persist immediately. The Rust window-close
   *  handler reads config from disk each close, so no restart is needed. */
  setCloseToTray: (value: boolean) => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  config: defaultConfig(),
  loaded: false,
  appliedProxy: { ...DEFAULT_PROXY },
  appliedProvider: cloneProvider(DEFAULT_PROVIDER),
  proxyApplying: false,
  providerApplying: false,
  restartNonce: 0,

  load: async () => {
    try {
      const cfg = await ipc.cfgLoad();
      const rawProxy = cfg?.proxy;
      const mergedProvider = normalizeProvider(cfg?.provider as RawProviderSettings | undefined);
      const merged: AppConfig = {
        proxy: { ...DEFAULT_PROXY, ...rawProxy, protocol: proxyProtocol(rawProxy?.protocol) },
        provider: mergedProvider,
        telemetry_opt_out: !!cfg?.telemetry_opt_out,
        last_telemetry_date: cfg?.last_telemetry_date ?? null,
        close_to_tray: cfg?.close_to_tray ?? true,
      };
      set({
        config: merged,
        appliedProxy: { ...merged.proxy },
        appliedProvider: cloneProvider(merged.provider),
        loaded: true,
      });
    } catch {
      set({ loaded: true });
    }
  },

  setProxy: (patch) =>
    set((s) => ({ config: { ...s.config, proxy: { ...s.config.proxy, ...patch } } })),

  commitProxy: async () => {
    const { config, appliedProxy } = get();
    if (sameProxy(config.proxy, appliedProxy)) return; // nothing changed — no restart
    set({ proxyApplying: true });
    try {
      await ipc.cfgSave(config);
      // Apply to the running Codex sidecar. Tear the active session down FIRST
      // (stop_session removes it from the registry synchronously, then kills the
      // process) so the nonce-driven restart in App.tsx spawns a fresh sidecar —
      // start_session would otherwise early-return the still-registered session.
      await restartActiveSession();
      set((s) => ({ appliedProxy: { ...config.proxy }, restartNonce: s.restartNonce + 1 }));
    } finally {
      set({ proxyApplying: false });
    }
  },

  setProvider: (patch) =>
    set((s) => ({
      config: { ...s.config, provider: applyProviderPatch(s.config.provider, patch) },
    })),

  addProviderProfile: () =>
    set((s) => ({
      config: { ...s.config, provider: addBlankProviderProfile(s.config.provider) },
    })),

  duplicateProviderProfile: () =>
    set((s) => ({
      config: { ...s.config, provider: duplicateProviderProfile(s.config.provider) },
    })),

  selectProviderProfile: (profileId) =>
    set((s) => ({
      config: { ...s.config, provider: selectProviderProfile(s.config.provider, profileId) },
    })),

  removeProviderProfile: (profileId) =>
    set((s) => ({
      config: { ...s.config, provider: removeProviderProfile(s.config.provider, profileId) },
    })),

  commitProvider: async () => {
    const { config, appliedProvider } = get();
    const normalizedProvider = normalizeProvider(config.provider);
    const normalizedConfig: AppConfig = { ...config, provider: normalizedProvider };
    if (!providerCanApply(normalizedProvider)) {
      return;
    }
    const changedByNormalization = providerHasChangedByNormalization(config.provider, normalizedProvider);
    if (changedByNormalization) set({ config: normalizedConfig });
    if (sameProvider(normalizedProvider, appliedProvider)) {
      if (changedByNormalization) await ipc.cfgSave(normalizedConfig);
      return;
    }
    set({ providerApplying: true });
    try {
      await ipc.cfgSave(normalizedConfig);
      await prepareProviderRuntimeSession(providerTitle(normalizedProvider));
      set((s) => ({ appliedProvider: cloneProvider(normalizedProvider), restartNonce: s.restartNonce + 1 }));
    } finally {
      set({ providerApplying: false });
    }
  },

  setTelemetryOptOut: async (value) => {
    const { config } = get();
    const next: AppConfig = { ...config, telemetry_opt_out: value };
    set({ config: next });
    try {
      await ipc.cfgSave(next);
    } catch {
      // Persist failure → roll back so UI reflects truth.
      set({ config });
      throw new Error("failed to save settings");
    }
  },

  setCloseToTray: async (value) => {
    const { config } = get();
    const next: AppConfig = { ...config, close_to_tray: value };
    set({ config: next });
    try {
      await ipc.cfgSave(next);
    } catch {
      set({ config }); // roll back so UI reflects truth
      throw new Error("failed to save settings");
    }
  },
}));
