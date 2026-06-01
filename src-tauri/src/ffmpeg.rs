//! ffmpeg / ffprobe integration.
//!
//! V1 video support runs deterministic edits by letting the Codex agent shell out
//! to ffmpeg (decision: ability stays in Codex, Jasmine ships the tool). The same
//! binaries are used Rust-side for metadata (`ffprobe`) and poster extraction.
//!
//! **Delivery.** The binaries are bundled with the app (Tauri `externalBin` /
//! resources) and resolved at startup into [`set_bundled_dir`]. We DON'T download
//! at runtime — ffmpeg is the load-bearing dependency for the whole feature, so
//! 100% offline availability beats a smaller installer. If no bundled binary is
//! found (e.g. `pnpm tauri dev` without the binaries staged), we fall back to a
//! bare `ffmpeg`/`ffprobe` command resolved via PATH so development still works.
//!
//! **Sandbox.** Phase 0 S1 confirmed the Codex `workspace-write` seatbelt sandbox
//! executes binaries living outside the workspace (it already runs `sed`, `rg`,
//! `zsh` from system paths). So the bundled dir is injected into the sidecar PATH
//! (see `codex::build_augmented_path`) and Codex's shell finds our ffmpeg.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::process::hide_console_window;

/// Directory holding the bundled ffmpeg/ffprobe, resolved once at startup from the
/// Tauri resource/sidecar dir. `None` until set (dev fallback uses PATH).
static BUNDLED_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

/// Record the directory where the bundled ffmpeg/ffprobe live. Called once from
/// `lib.rs::setup` after probing candidate locations. Idempotent (first wins).
pub fn set_bundled_dir(dir: Option<PathBuf>) {
    let _ = BUNDLED_DIR.set(dir);
}

/// The bundled-binary directory, if one was found and actually contains ffmpeg.
pub fn bundled_dir() -> Option<PathBuf> {
    let dir = BUNDLED_DIR.get().cloned().flatten()?;
    if dir.join(exe_name("ffmpeg")).exists() {
        Some(dir)
    } else {
        None
    }
}

/// Directory to prepend to the Codex sidecar PATH so its shell finds our ffmpeg.
/// `None` in dev (no bundled binaries) — the sidecar then uses system ffmpeg.
pub fn path_dir() -> Option<PathBuf> {
    bundled_dir()
}

/// Absolute path to the ffmpeg binary to invoke. Prefers the bundled copy; falls
/// back to a bare command name resolved via PATH (dev / system install).
pub fn ffmpeg_path() -> PathBuf {
    match bundled_dir() {
        Some(dir) => dir.join(exe_name("ffmpeg")),
        None => PathBuf::from(exe_name("ffmpeg")),
    }
}

pub fn ffprobe_path() -> PathBuf {
    match bundled_dir() {
        Some(dir) => dir.join(exe_name("ffprobe")),
        None => PathBuf::from(exe_name("ffprobe")),
    }
}

/// Probe candidate directories for bundled binaries. Tauri places `externalBin`
/// sidecars next to the main executable; `resources` land in the platform
/// resource dir. We accept either, plus an explicit override for tests.
pub fn resolve_bundled_dir(resource_dir: Option<&Path>, exe_dir: Option<&Path>) -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("JASMINE_FFMPEG_DIR") {
        let p = PathBuf::from(custom);
        if p.join(exe_name("ffmpeg")).exists() {
            return Some(p);
        }
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(d) = exe_dir {
        candidates.push(d.to_path_buf());
    }
    if let Some(d) = resource_dir {
        candidates.push(d.to_path_buf());
        candidates.push(d.join("binaries"));
    }
    candidates
        .into_iter()
        .find(|d| d.join(exe_name("ffmpeg")).exists())
}

/// Best-effort check that ffprobe can run. Cheap, used to gate video features.
pub fn is_available() -> bool {
    let mut cmd = Command::new(ffprobe_path());
    cmd.arg("-version");
    hide_console_window(&mut cmd);
    matches!(cmd.output(), Ok(out) if out.status.success())
}

/// Video stream metadata extracted via ffprobe. Missing/garbled probes yield 0s;
/// the canvas self-corrects dimensions once the first frame decodes.
#[derive(Debug, Clone, Copy, Default)]
pub struct VideoMeta {
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    pub fps: f64,
}

/// Parse ffmpeg's `num/den` rational frame-rate string (e.g. `30000/1001`).
fn parse_rational(s: &str) -> f64 {
    match s.split_once('/') {
        Some((n, d)) => {
            let n: f64 = n.trim().parse().unwrap_or(0.0);
            let d: f64 = d.trim().parse().unwrap_or(0.0);
            if d != 0.0 {
                n / d
            } else {
                0.0
            }
        }
        None => s.trim().parse().unwrap_or(0.0),
    }
}

/// Run `ffprobe` to read width/height/duration/fps of the first video stream.
/// Returns a zeroed [`VideoMeta`] (never errors) so import never fails on a probe
/// hiccup — the renderer corrects dimensions from the decoded first frame.
pub fn probe_video(path: &Path) -> VideoMeta {
    match probe_video_inner(path) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(module = "ffmpeg", "probe_video failed for {}: {e:#}", path.display());
            VideoMeta::default()
        }
    }
}

fn probe_video_inner(path: &Path) -> Result<VideoMeta> {
    let mut cmd = Command::new(ffprobe_path());
    cmd.args([
        "-v",
        "quiet",
        "-print_format",
        "json",
        "-show_streams",
        "-show_format",
    ])
    .arg(path);
    hide_console_window(&mut cmd);
    let out = cmd.output().context("spawn ffprobe")?;
    if !out.status.success() {
        return Err(anyhow!("ffprobe exited {}", out.status));
    }
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).context("parse ffprobe json")?;

    let streams = json
        .get("streams")
        .and_then(|s| s.as_array())
        .ok_or_else(|| anyhow!("no streams"))?;
    let v = streams
        .iter()
        .find(|s| s.get("codec_type").and_then(|c| c.as_str()) == Some("video"))
        .ok_or_else(|| anyhow!("no video stream"))?;

    let width = v.get("width").and_then(|w| w.as_u64()).unwrap_or(0) as u32;
    let height = v.get("height").and_then(|h| h.as_u64()).unwrap_or(0) as u32;

    // fps: prefer avg_frame_rate, fall back to r_frame_rate.
    let fps = v
        .get("avg_frame_rate")
        .and_then(|r| r.as_str())
        .map(parse_rational)
        .filter(|f| *f > 0.0)
        .or_else(|| {
            v.get("r_frame_rate")
                .and_then(|r| r.as_str())
                .map(parse_rational)
        })
        .unwrap_or(0.0);

    // duration: stream first, then format.
    let duration = v
        .get("duration")
        .and_then(|d| d.as_str())
        .and_then(|d| d.parse::<f64>().ok())
        .or_else(|| {
            json.get("format")
                .and_then(|f| f.get("duration"))
                .and_then(|d| d.as_str())
                .and_then(|d| d.parse::<f64>().ok())
        })
        .unwrap_or(0.0);

    Ok(VideoMeta {
        width,
        height,
        duration,
        fps,
    })
}

/// Extract the first frame of `src` as a JPEG to `dest` (a poster for the canvas).
/// Best-effort: returns Err if ffmpeg is missing or the input is unreadable.
pub fn extract_poster(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut cmd = Command::new(ffmpeg_path());
    cmd.args(["-y", "-loglevel", "error", "-i"])
        .arg(src)
        .args(["-frames:v", "1", "-q:v", "3"])
        .arg(dest);
    hide_console_window(&mut cmd);
    let out = cmd.output().context("spawn ffmpeg for poster")?;
    if !out.status.success() {
        return Err(anyhow!(
            "ffmpeg poster extract exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

/// Encode a directory of zero-padded PNG frames (`frame_%05d.png`) into an H.264
/// MP4 at `fps`. Backs the motion-graphics render primitive: Codex authors an
/// HTML/canvas animation, Jasmine renders frames in a hidden webview, then this
/// stitches them. `-pix_fmt yuv420p` + `+faststart` for max WebKit/WebView2
/// compatibility (same codec the video prompt asks Codex to produce); the scale
/// filter rounds to even dimensions (libx264/yuv420p rejects odd width/height).
pub fn encode_frames(frame_dir: &Path, fps: f64, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let fps_arg = if fps > 0.0 { fps.to_string() } else { "30".to_string() };
    let pattern = frame_dir.join("frame_%05d.png");
    let mut cmd = Command::new(ffmpeg_path());
    cmd.args(["-y", "-loglevel", "error", "-framerate", &fps_arg, "-i"])
        .arg(&pattern)
        .args([
            "-vf",
            "scale=trunc(iw/2)*2:trunc(ih/2)*2",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
        ])
        .arg(dest);
    hide_console_window(&mut cmd);
    let out = cmd.output().context("spawn ffmpeg for frame encode")?;
    if !out.status.success() {
        return Err(anyhow!(
            "ffmpeg frame encode exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_parses() {
        assert!((parse_rational("30000/1001") - 29.97).abs() < 0.01);
        assert_eq!(parse_rational("30/1"), 30.0);
        assert_eq!(parse_rational("0/0"), 0.0);
        assert_eq!(parse_rational("25"), 25.0);
    }

    #[test]
    fn resolve_bundled_dir_probes_candidates() {
        let base = std::env::temp_dir().join(format!("jasmine_resolve_{}", std::process::id()));
        let exe = base.join("exe");
        let res = base.join("res");
        let res_bin = res.join("binaries");
        let empty = base.join("empty");
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::create_dir_all(&res_bin).unwrap();
        std::fs::create_dir_all(&empty).unwrap();

        // exe dir holds the binary → resolves to exe dir (Tauri externalBin layout).
        std::fs::write(exe.join(exe_name("ffmpeg")), b"x").unwrap();
        assert_eq!(resolve_bundled_dir(None, Some(&exe)).as_deref(), Some(exe.as_path()));

        // resource_dir/binaries fallback (bundle.resources layout).
        std::fs::write(res_bin.join(exe_name("ffmpeg")), b"x").unwrap();
        assert_eq!(
            resolve_bundled_dir(Some(&res), None).as_deref(),
            Some(res_bin.as_path())
        );

        // Nothing present → None (dev falls back to system PATH).
        assert_eq!(resolve_bundled_dir(Some(&empty), Some(&empty)), None);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn exe_name_platform() {
        let n = exe_name("ffmpeg");
        if cfg!(windows) {
            assert_eq!(n, "ffmpeg.exe");
        } else {
            assert_eq!(n, "ffmpeg");
        }
    }

    /// End-to-end: synthesize a real mp4 with ffmpeg, then verify ffprobe metadata
    /// extraction + poster frame extraction. Requires ffmpeg on PATH; run with
    /// `cargo test --lib ffmpeg -- --ignored`.
    #[test]
    #[ignore]
    fn probe_and_poster_roundtrip() {
        if !is_available() {
            eprintln!("ffmpeg/ffprobe not available; skipping");
            return;
        }
        let dir = std::env::temp_dir().join("jasmine_ffmpeg_test");
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("clip.mp4");
        // 2s, 320x240, 25fps test pattern.
        let mut gen = Command::new(ffmpeg_path());
        gen.args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=2",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&video);
        assert!(gen.output().unwrap().status.success(), "ffmpeg gen failed");

        let meta = probe_video(&video);
        assert_eq!(meta.width, 320);
        assert_eq!(meta.height, 240);
        assert!((meta.fps - 25.0).abs() < 0.5, "fps was {}", meta.fps);
        assert!(meta.duration > 1.5 && meta.duration < 2.5, "dur {}", meta.duration);

        let poster = dir.join("poster.jpg");
        extract_poster(&video, &poster).unwrap();
        assert!(poster.exists() && std::fs::metadata(&poster).unwrap().len() > 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
