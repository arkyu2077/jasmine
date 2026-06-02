#!/usr/bin/env bash
#
# Fetch static ffmpeg + ffprobe for the CURRENT platform into
# src-tauri/binaries/, named with the Rust target triple suffix that Tauri's
# `bundle.externalBin` expects. Run once per build platform (locally or in CI)
# before `pnpm tauri build`.
#
# ── WHY THIS IS A SCRIPT, NOT A COMMITTED BINARY ──────────────────────────────
# ffmpeg static builds are typically GPL (libx264 etc.). Bundling them is
# redistribution: it is AGPL-3.0 compatible (separate executable, invoked via
# CLI = aggregation), BUT you MUST (a) confirm the exact build + its codecs are
# redistributable / GPLv3-compatible, and (b) ship the corresponding NOTICE +
# offer of source. See scripts/FFMPEG_BUNDLING.md and binaries/THIRD_PARTY_NOTICES.
# This is a locked plan decision for the maintainer/legal — DO NOT bundle a build
# without confirming it. The default source URLs below are EXAMPLES.
#
# In dev (no bundled binaries) Jasmine falls back to system ffmpeg on PATH, so
# this is only needed for distributable builds.
#
# Override sources with FFMPEG_URL / windows handling via env if you self-host.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/src-tauri/binaries"
mkdir -p "$OUT"

os="$(uname -s)"
# Tauri matches binaries/<name>-<triple> against the RUST BUILD TARGET, not the
# OS arch — on an Apple Silicon mac the toolchain is often x86_64 under Rosetta.
# Derive the triple from rustc so the staged name always matches the build.
TRIPLE="${RUST_TARGET:-$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')}"
[ -n "$TRIPLE" ] || { echo "could not determine Rust host triple (set RUST_TARGET)" >&2; exit 1; }
echo "Rust build target: $TRIPLE"

case "$os" in
  Darwin)
    EXE=""
    # Pick a source that matches the build target's arch:
    #  - arm64  → osxexperts.net (arm64-native static FFmpeg 8.1)
    #  - x86_64 → evermeet.cx (notarized x86_64 static ffmpeg/ffprobe)
    # Override either with FFMPEG_URL / FFPROBE_URL if you self-host.
    case "$TRIPLE" in
      aarch64-apple-darwin)
        FFMPEG_ZIP="${FFMPEG_URL:-https://www.osxexperts.net/ffmpeg81arm.zip}"
        FFPROBE_ZIP="${FFPROBE_URL:-https://www.osxexperts.net/ffprobe81arm.zip}"
        ;;
      *)
        FFMPEG_ZIP="${FFMPEG_URL:-https://evermeet.cx/ffmpeg/getrelease/ffmpeg/zip}"
        FFPROBE_ZIP="${FFPROBE_URL:-https://evermeet.cx/ffmpeg/getrelease/ffprobe/zip}"
        ;;
    esac
    ;;
  MINGW*|MSYS*|CYGWIN*)
    TRIPLE="x86_64-pc-windows-msvc"
    EXE=".exe"
    # BtbN publishes win64 gpl static builds (single zip with ffmpeg+ffprobe).
    BTBN_ZIP="${FFMPEG_URL:-https://github.com/BtbN/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-win64-gpl.zip}"
    ;;
  *)
    echo "unsupported OS: $os (V1 targets: macOS, Windows)" >&2
    exit 1
    ;;
esac

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Fetching ffmpeg/ffprobe for $TRIPLE ..."

if [ "$os" = "Darwin" ]; then
  curl -fsSL "$FFMPEG_ZIP" -o "$tmp/ffmpeg.zip"
  curl -fsSL "$FFPROBE_ZIP" -o "$tmp/ffprobe.zip"
  (cd "$tmp" && unzip -o -q ffmpeg.zip && unzip -o -q ffprobe.zip)
  install -m 0755 "$tmp/ffmpeg" "$OUT/ffmpeg-$TRIPLE"
  install -m 0755 "$tmp/ffprobe" "$OUT/ffprobe-$TRIPLE"
else
  curl -fsSL "$BTBN_ZIP" -o "$tmp/ffmpeg.zip"
  (cd "$tmp" && unzip -o -q ffmpeg.zip)
  bindir="$(find "$tmp" -type d -name bin | head -1)"
  cp "$bindir/ffmpeg$EXE" "$OUT/ffmpeg-$TRIPLE$EXE"
  cp "$bindir/ffprobe$EXE" "$OUT/ffprobe-$TRIPLE$EXE"
fi

# Ship the GPL notice next to the binaries.
cp "$ROOT/scripts/THIRD_PARTY_NOTICES.ffmpeg.txt" "$OUT/THIRD_PARTY_NOTICES"

echo "Staged:"
ls -la "$OUT"
echo
echo "NEXT: add the externalBin entry to src-tauri/tauri.conf.json (see"
echo "scripts/FFMPEG_BUNDLING.md), then verify binaries/THIRD_PARTY_NOTICES."
