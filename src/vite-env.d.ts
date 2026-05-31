/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Closed-source cloud API base. Unset → CLOUD_ENABLED=false. */
  readonly VITE_JASMINE_API_BASE?: string;
  /** Closed-source cloud API key, baked into official builds. Unset → CLOUD_ENABLED=false. */
  readonly VITE_JASMINE_API_KEY?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

// Build-time package version inlined via vite.config.ts `define:`.
declare const __APP_VERSION__: string;
