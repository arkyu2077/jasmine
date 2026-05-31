# Bundling ffmpeg (V1 video support)

Jasmine's V1 video feature shells out to **ffmpeg** (Codex runs it to edit video)
and uses **ffprobe** (Rust-side metadata + poster extraction). The binaries are
shipped with the app and resolved at startup by `src-tauri/src/ffmpeg.rs`.

In **development** no bundled binaries are needed — `ffmpeg.rs` falls back to a
system `ffmpeg`/`ffprobe` on `PATH`. The steps below are only for **distributable
builds**.

## 1. Stage the binaries

Run on each build platform (or in CI) before `pnpm tauri build`:

```bash
./scripts/fetch-ffmpeg.sh
```

This downloads static `ffmpeg`/`ffprobe` into `src-tauri/binaries/` named with the
Rust target triple, matching Tauri's `externalBin` convention:

- `binaries/ffmpeg-aarch64-apple-darwin`, `binaries/ffprobe-aarch64-apple-darwin`
- `binaries/ffmpeg-x86_64-apple-darwin`, `binaries/ffprobe-x86_64-apple-darwin`
- `binaries/ffmpeg-x86_64-pc-windows-msvc.exe`, `binaries/ffprobe-...msvc.exe`

`binaries/` is git-ignored (large; fetched per build).

## 2. Add the externalBin entry to `tauri.conf.json`

**Only after step 1** (an `externalBin` pointing at missing files fails the
build). Add to the `bundle` object:

```jsonc
"bundle": {
  "externalBin": [
    "binaries/ffmpeg",
    "binaries/ffprobe"
  ],
  ...
}
```

Tauri bundles the per-triple files and places `ffmpeg`/`ffprobe` (suffix
stripped) next to the app executable at runtime. `ffmpeg::resolve_bundled_dir`
already probes the executable dir + resource dir, so no code change is needed.

> Override the resolved dir at runtime with `JASMINE_FFMPEG_DIR` (used by tests
> and for pointing at a custom build).

## 3. Licensing (REQUIRED — maintainer/legal decision)

Static ffmpeg builds are usually **GPL** (libx264, etc.). Shipping them is
redistribution. This is compatible with Jasmine's **AGPL-3.0** (ffmpeg is a
separate executable invoked via CLI = mere aggregation), but you MUST:

1. Confirm the exact build you ship is redistributable and GPLv3-compatible
   (the example sources in `fetch-ffmpeg.sh` — evermeet.cx, BtbN — are GPL).
2. Ship `binaries/THIRD_PARTY_NOTICES` (template provided) and an offer of
   corresponding source, per GPL.
3. Surface the attribution in the app's About/Settings.

Do not ship a build until this is confirmed. This was flagged as a locked
decision in the V1 plan.

## 4. Windows S1 verification (run on a Windows machine)

The macOS Codex sandbox was verified to execute the bundled ffmpeg (it runs
system tools from outside the workspace). Windows uses a different sandbox
mechanism, so it must be checked on real Windows hardware (Tauri can't
cross-compile/verify from macOS). Turnkey smoke — mirrors the macOS runtime test:

```powershell
# 1. Stage Windows binaries + build (git-bash for fetch; PowerShell for the rest)
bash scripts/fetch-ffmpeg.sh           # → binaries\ffmpeg-x86_64-pc-windows-msvc.exe (+ffprobe)
pnpm tauri build --bundles app          # or run `pnpm tauri dev`

# 2. Headless runtime check: open a board with a video, confirm import + watcher
$BOARD = "$env:TEMP\jmtest"; Remove-Item -Recurse -Force $BOARD -EA 0; mkdir $BOARD | Out-Null
& ".\binaries\ffmpeg-x86_64-pc-windows-msvc.exe" -y -f lavfi -i "testsrc=size=640x360:rate=25:duration=3" -pix_fmt yuv420p "$BOARD\clip.mp4"
$env:JASMINE_OPEN_BOARD = $BOARD
# launch the built exe (Jasmine.exe) or `pnpm tauri dev`
# EXPECT in %USERPROFILE%\.jasmine\logs: "bundled ffmpeg dir", "board opened ... placements=1", "watching ..."
# EXPECT $BOARD\.jasmine\board.json: clip.mp4 with mime video/mp4 + duration/fps

# 3. THE S1 GATE — Codex must execute the bundled ffmpeg under workspace-write:
#    set JASMINE_TEST_PROMPT to a trim instruction, launch, and confirm Codex's
#    shell runs ffmpeg WITHOUT a sandbox denial / approval prompt, and that
#    out.mp4 appears in board.json with parentId = clip.mp4's placement.
$env:JASMINE_TEST_PROMPT = "Using ffmpeg, trim clip.mp4 to its first 1 second as out.mp4. Do only this."
# launch → wait → check $BOARD\.jasmine\board.json for out.mp4 + parentId.
```

If step 3 shows a sandbox denial (Codex can't exec the bundled exe outside the
workspace), fall back to materializing ffmpeg into each board's `.jasmine\bin\`
(inside the workspace) — `ffmpeg.rs::resolve_bundled_dir` + the PATH injection
already make that a localized change.
