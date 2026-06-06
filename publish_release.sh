#!/usr/bin/env bash
# publish_release.sh — publish the macOS installers to a GitHub Release.
#
# macOS half of the release. Windows publishes independently from a Windows box
# via publish_release.ps1, uploading its installer to the SAME GitHub release.
#
# NOTE: this fork has no auto-update server / CDN yet, so this only mirrors the
# .dmg installers to GitHub Releases — no R2/CDN upload, no updater manifests,
# no code-signing step here. When a Jasmine-owned update server exists, restore
# the manifest/upload path (see git history for the previous R2 version).
#
# Prerequisite:  ./build_release.sh            (default = arm + Intel)
#
# Usage:
#   ./publish_release.sh                       # create release + upload .dmg
#   ./publish_release.sh --dry-run             # print what would happen

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

info() { printf '→ %s\n' "$*"; }
ok()   { printf '  \033[32m✓\033[0m %s\n' "$*"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$*"; }
die()  { printf '  \033[31m✗\033[0m %s\n' "$*" >&2; exit 1; }
size_of() { ls -lh "$1" | awk '{print $5}'; }
expected_dmg_name() {
  case "$1" in
    aarch64-apple-darwin) echo "Jasmine_${2}_aarch64.dmg" ;;
    x86_64-apple-darwin) echo "Jasmine_${2}_x64.dmg" ;;
    *) echo "" ;;
  esac
}

[[ "$(uname -s)" == "Darwin" ]] || die "macOS only — on Windows run publish_release.ps1"

DRY_RUN=0
for arg in "$@"; do
  case "$arg" in
    --dry-run)   DRY_RUN=1 ;;
    -h|--help)   sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown flag: $arg" ;;
  esac
done

command -v gh >/dev/null || die "gh CLI not on PATH — install GitHub CLI and run 'gh auth login'"

GH_REPO="arkyu2077/jasmine"

# ── version (must match across the three manifests) ──────────────────────────
pkg_ver=$(node -p "require('./package.json').version" 2>/dev/null || echo "?")
conf_ver=$(node -p "require('./src-tauri/tauri.conf.json').version" 2>/dev/null || echo "?")
cargo_ver=$(grep -m1 '^version' src-tauri/Cargo.toml | sed -E 's/.*"(.*)".*/\1/')
if [[ "$pkg_ver" == "$conf_ver" && "$conf_ver" == "$cargo_ver" ]]; then
  VERSION="$conf_ver"
  ok "version: $VERSION (package.json = tauri.conf.json = Cargo.toml)"
else
  die "version mismatch: package.json=$pkg_ver tauri.conf.json=$conf_ver Cargo.toml=$cargo_ver"
fi
NOTES="Jasmine v${VERSION}"

# ── locate installers (arm + intel .dmg) ─────────────────────────────────────
TARGET_DIR="${ROOT}/src-tauri/target"
declare -a GH_FILES=()

for arch_pair in "aarch64-apple-darwin:aarch64" "x86_64-apple-darwin:x86_64"; do
  IFS=":" read -r RUST_TARGET ARCH <<< "$arch_pair"
  BUNDLE="$TARGET_DIR/$RUST_TARGET/release/bundle"
  [[ -d "$BUNDLE" ]] || die "bundle missing for $RUST_TARGET — run ./build_release.sh before publishing"
  EXPECTED_DMG=$(expected_dmg_name "$RUST_TARGET" "$VERSION")
  DMG=""
  [[ -n "$EXPECTED_DMG" && -f "$BUNDLE/dmg/$EXPECTED_DMG" ]] && DMG="$BUNDLE/dmg/$EXPECTED_DMG"
  [[ -n "$DMG" ]] || die "current-version .dmg missing for $RUST_TARGET: expected ${EXPECTED_DMG}"
  GH_FILES+=("$DMG")
  ok "$RUST_TARGET → $(basename "$DMG") ($(size_of "$DMG"))"
done

[[ ${#GH_FILES[@]} -gt 0 ]] || die "no installers found — did you run ./build_release.sh first?"

echo ""
info "GitHub release v${VERSION} (${GH_REPO}) will receive ${#GH_FILES[@]} asset(s):"
for f in "${GH_FILES[@]}"; do printf '    %s (%s)\n' "$(basename "$f")" "$(size_of "$f")"; done
echo ""

if [[ "$DRY_RUN" -eq 1 ]]; then
  ok "DRY RUN — nothing published."
  exit 0
fi

read -r -p "Proceed with GitHub release? [y/N] " yn
[[ "$yn" =~ ^[Yy]$ ]] || { warn "aborted by user"; exit 0; }

# ── GitHub Release ───────────────────────────────────────────────────────────
# Tag + create the release (idempotent) and upload the installers. Windows
# publishes to the SAME release from publish_release.ps1 (whichever runs first
# creates it; the other uploads with --clobber).
TAG="v${VERSION}"
info "GitHub release: ${GH_REPO}@${TAG}"
if ! git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  git tag -a "$TAG" -m "Jasmine $TAG" && git push origin "$TAG" && ok "tagged $TAG"
else
  ok "tag $TAG already exists"
fi
if ! gh release view "$TAG" --repo "$GH_REPO" >/dev/null 2>&1; then
  gh release create "$TAG" --repo "$GH_REPO" --title "Jasmine $TAG" --notes "$NOTES" --verify-tag && ok "created GitHub release $TAG"
fi
info "uploading ${#GH_FILES[@]} installer(s) — this may take a while"
gh release upload "$TAG" "${GH_FILES[@]}" --repo "$GH_REPO" --clobber \
  && ok "uploaded ${#GH_FILES[@]} installer(s) → GitHub Release"
echo ""

ok "macOS release v${VERSION} published to GitHub Releases"
warn "REMINDER: bump version in package.json / tauri.conf.json / Cargo.toml BEFORE"
warn "the next release, or you'll re-publish v${VERSION}."
warn "Windows publishes separately — run publish_release.ps1 on the Windows box."
