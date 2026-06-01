//! Motion-graphics render primitive (Phase 0 spike).
//!
//! ffmpeg covers mechanical edits but has no layout/animation engine. Rather than
//! bundle a headless Chromium (Remotion/HyperFrames), Jasmine reuses the engine it
//! already ships — its own WebView — plus the already-bundled ffmpeg. Codex authors
//! a self-contained HTML/canvas animation in the board folder; Jasmine opens a
//! HIDDEN webview pointed at it via `jasmine://`, injects a virtual-clock harness,
//! drives the animation frame-by-frame capturing each frame as a PNG data URL
//! (`canvas.toDataURL`), ships the base64 back to Rust, and stitches them with
//! `ffmpeg::encode_frames`. The resulting mp4 lands in the board folder and the
//! existing media watcher ingests it onto the canvas with lineage — no new ingest
//! code.
//!
//! Capture is decoupled from real compositing: the harness overrides
//! `requestAnimationFrame`/`performance.now` and flushes callbacks against a
//! virtual clock, so frame N is reproducible and the (hidden) webview never needs
//! to paint to screen — only the canvas backing store matters.
//!
//! **Why Rust drives the loop.** A hidden (`visible:false`) WKWebView throttles its
//! event loop after a few seconds, so a self-paced JS loop (`await invoke` /
//! `setTimeout` / `toBlob`) stalls partway through long renders. Instead Rust steps
//! frame-by-frame via `webview.eval(...)` — explicit script injection executes even
//! on an occluded webview — and the JS captures **synchronously** (`toDataURL`) and
//! fire-and-forget posts the frame. Rust paces by waiting for each frame file to
//! appear, so nothing depends on the webview's (throttled) self-scheduling.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Deserialize;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

/// Per-frame timeout while waiting for the eval'd capture to write its PNG.
const FRAME_TIMEOUT: Duration = Duration::from_secs(15);
/// Poll interval while waiting for a frame file.
const FRAME_POLL: Duration = Duration::from_millis(10);
/// Resource caps on a render request (guard against hangs / disk fill from a
/// runaway or malicious `.render.json`).
const MAX_FPS: f64 = 60.0;
const MAX_DURATION_S: f64 = 120.0;
const MAX_FRAMES: u32 = 3600;
/// Max decoded bytes for a single captured frame (~8MB covers 4K PNG).
const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
/// How long to wait for the page to signal `render_ready` before giving up.
const READY_TIMEOUT: Duration = Duration::from_secs(30);

/// Validate that `name` is a plain top-level `.mp4` filename safe to write into
/// the board folder: no path separators, no `..`, not hidden/absolute, `.mp4` ext.
fn safe_mp4_name(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    if !lower.ends_with(".mp4") {
        return None;
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") || name.starts_with('.') {
        return None;
    }
    // Reject anything Path would interpret as more than a single final component.
    if std::path::Path::new(name).components().count() != 1 {
        return None;
    }
    Some(name.to_string())
}

/// Suffix Codex uses to request a render (a top-level `<base>.render.json`).
pub const REQUEST_SUFFIX: &str = ".render.json";

/// One in-flight render job. Created in [`start_render`] before the hidden webview
/// is spawned; the harness calls back with the `job_id` so Rust can resolve where
/// frames go and what to encode.
pub struct RenderJob {
    pub board_id: String,
    pub folder: PathBuf,
    /// Scratch dir for captured PNG frames: `<folder>/.jasmine/render/<job>/`.
    pub frame_dir: PathBuf,
    pub fps: f64,
    /// Total frames to capture (`round(duration * fps)`, min 1).
    pub total: u32,
    /// Output file name relative to the board folder (e.g. `render_out.mp4`).
    pub out_rel: String,
    /// Request base name for the `<base>.render.done`/`.err` status file Codex
    /// polls. `None` for ad-hoc renders with no request handshake.
    pub status_base: Option<String>,
    /// Set once the driver loop starts, so a duplicate `render_ready` is a no-op.
    pub started: AtomicBool,
}

/// The render request Codex writes as `<base>.render.json` in the board folder.
#[derive(Deserialize)]
struct RenderRequest {
    /// Relative path to the canvas-target HTML animation (loaded via `jasmine://`).
    scene: String,
    #[serde(default)]
    fps: f64,
    #[serde(default)]
    duration: f64,
    /// Output mp4 file name (relative to the board folder). Defaults to `<base>.mp4`.
    #[serde(default)]
    out: String,
}

/// Active render jobs, keyed by job id. One hidden webview per job.
#[derive(Default)]
pub struct RenderRegistry {
    jobs: Mutex<HashMap<String, Arc<RenderJob>>>,
}

impl RenderRegistry {
    pub fn insert(&self, id: String, job: Arc<RenderJob>) {
        self.jobs.lock().insert(id, job);
    }
    pub fn get(&self, id: &str) -> Option<Arc<RenderJob>> {
        self.jobs.lock().get(id).cloned()
    }
    pub fn remove(&self, id: &str) -> Option<Arc<RenderJob>> {
        self.jobs.lock().remove(id)
    }
}

fn render_label(job_id: &str) -> String {
    format!("render-{job_id}")
}

/// Harness injected into the hidden webview before any page script runs. Overrides
/// the animation clock and exposes `window.__jcap(i)` — a SYNCHRONOUS capture Rust
/// drives one frame at a time via `eval`. On load it signals `render_ready` so Rust
/// can start stepping. Uses `__TAURI_INTERNALS__.invoke` (`withGlobalTauri` is off).
const HARNESS_JS: &str = r#"(function(){
  var JOB="__JOB__"; var FPS_DEFAULT=__FPS__;
  var vt=0; var rafCbs=[]; var epoch=Date.now(); var CANVAS=null; var FPS=FPS_DEFAULT||30;
  performance.now=function(){return vt;};
  try{Date.now=function(){return epoch+vt;};}catch(e){}
  window.requestAnimationFrame=function(cb){rafCbs.push(cb);return rafCbs.length;};
  window.cancelAnimationFrame=function(){};
  function inv(cmd,args){return window.__TAURI_INTERNALS__.invoke(cmd,args);}
  function flush(){var cbs=rafCbs;rafCbs=[];for(var i=0;i<cbs.length;i++){try{cbs[i](vt);}catch(e){}}}
  // SYNCHRONOUS per-frame capture, driven by Rust via eval (no self-paced loop —
  // a hidden webview throttles its event loop). toDataURL is sync (no toBlob/
  // FileReader callbacks); the frame is fire-and-forget posted as a compact base64
  // string (a JSON number array chokes IPC at 1080p). Returns nothing to Rust.
  window.__jcap=function(i){
    try{
      if(!CANVAS){inv('render_fail',{jobId:JOB,error:'no canvas'});return;}
      vt=i*(1000/FPS); flush();
      var d=CANVAS.toDataURL('image/png');
      inv('render_write_frame',{jobId:JOB,index:i,b64:d.slice(d.indexOf(',')+1)});
    }catch(e){inv('render_fail',{jobId:JOB,error:'cap '+i+': '+e});}
  };
  function ready(){
    if(!window.__TAURI_INTERNALS__){return;}
    if(window.JASMINE_FPS){FPS=window.JASMINE_FPS;} // scene may refine; request wins if set
    if(FPS_DEFAULT){FPS=FPS_DEFAULT;}
    CANVAS=document.querySelector('canvas');
    if(!CANVAS){inv('render_fail',{jobId:JOB,error:'no <canvas> element found in scene'});return;}
    inv('render_ready',{jobId:JOB});
  }
  if(document.readyState==='complete'){ready();}
  else{window.addEventListener('load',ready);}
})();"#;

fn harness_script(job_id: &str, fps: f64) -> String {
    HARNESS_JS
        .replace("__JOB__", job_id)
        .replace("__FPS__", &fps.to_string())
}

/// Kick off a render: register the job, then spawn a hidden webview (on the main
/// thread — required for window creation on macOS) pointed at the scene HTML via
/// `jasmine://`, with the harness injected. Fire-and-forget; the harness drives the
/// rest and calls back into [`render_write_frame`]/[`render_finalize`].
#[allow(clippy::too_many_arguments)]
pub fn start_render(
    app: &AppHandle,
    board_id: &str,
    folder: PathBuf,
    scene_rel: &str,
    fps: f64,
    duration: f64,
    out_rel: &str,
    status_base: Option<String>,
) {
    let job_id = nanoid::nanoid!(8);
    let frame_dir = folder.join(".jasmine").join("render").join(&job_id);
    let total = ((fps * duration).round().max(1.0) as u32).min(MAX_FRAMES);
    let job = Arc::new(RenderJob {
        board_id: board_id.to_string(),
        folder: folder.clone(),
        frame_dir,
        fps,
        total,
        out_rel: out_rel.to_string(),
        status_base,
        started: AtomicBool::new(false),
    });
    app.state::<Arc<RenderRegistry>>()
        .insert(job_id.clone(), job);

    // Percent-encode each scene path segment so spaces / `#` / `?` / non-ASCII
    // names don't corrupt the jasmine:// URL.
    let scene_enc = scene_rel
        .split('/')
        .map(|seg| urlencoding::encode(seg).into_owned())
        .collect::<Vec<_>>()
        .join("/");
    let url = match tauri::Url::parse(&format!("jasmine://localhost/{board_id}/{scene_enc}")) {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(module = "render", "bad scene url: {e}");
            app.state::<Arc<RenderRegistry>>().remove(&job_id);
            return;
        }
    };
    let script = harness_script(&job_id, fps);
    let label = render_label(&job_id);
    let app2 = app.clone();
    let job2 = job_id.clone();
    let res = app.run_on_main_thread(move || {
        match WebviewWindowBuilder::new(&app2, &label, WebviewUrl::CustomProtocol(url))
            .visible(false)
            .initialization_script(&script)
            .build()
        {
            Ok(_) => {
                tracing::info!(module = "render", job = %job2, "hidden render webview created")
            }
            Err(e) => {
                tracing::error!(module = "render", job = %job2, "render webview build failed: {e}");
                app2.state::<Arc<RenderRegistry>>().remove(&job2);
            }
        }
    });
    if let Err(e) = res {
        tracing::error!(module = "render", "run_on_main_thread failed: {e}");
        app.state::<Arc<RenderRegistry>>().remove(&job_id);
        return;
    }

    // Startup watchdog: if the page never loads / the harness never signals
    // `render_ready`, the job + hidden webview would leak. After READY_TIMEOUT,
    // if the driver hasn't started, fail and clean up.
    let app4 = app.clone();
    let job4 = job_id.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(READY_TIMEOUT).await;
        let reg = app4.state::<Arc<RenderRegistry>>();
        if let Some(j) = reg.get(&job4) {
            if !j.started.load(Ordering::SeqCst) {
                reg.remove(&job4);
                cleanup_job(&app4, &job4, &j.frame_dir);
                if let Some(b) = &j.status_base {
                    write_status(&j.folder, b, false, "render timed out before the scene loaded");
                }
                tracing::warn!(module = "render", job = %job4, "ready timeout — scene never loaded");
            }
        }
    });
}

/// Handle a `<base>.render.json` request that settled in the board folder: parse,
/// validate, kick off the render. The request file is consumed (deleted) on a
/// successful parse; status comes back via `<base>.render.done`/`.render.err`.
pub fn handle_render_request(app: &AppHandle, board_id: &str, folder: &Path, req_name: &str) {
    let base = req_name
        .strip_suffix(REQUEST_SUFFIX)
        .unwrap_or(req_name)
        .to_string();
    let req_path = folder.join(req_name);
    // Atomically claim the request: the watcher can deliver duplicate events for
    // one write, so rename it to a dotfile first — only one handler wins; the rest
    // bail silently (no spurious `.err`). The claim is a dotfile so the watcher
    // ignores it, and doesn't end in REQUEST_SUFFIX so it isn't re-detected.
    let claim = folder.join(format!(".{req_name}.claimed"));
    if std::fs::rename(&req_path, &claim).is_err() {
        return;
    }
    let parsed = std::fs::read_to_string(&claim)
        .ok()
        .and_then(|s| serde_json::from_str::<RenderRequest>(&s).ok());
    let _ = std::fs::remove_file(&claim); // consume the request either way
    let Some(req) = parsed else {
        write_status(folder, &base, false, "invalid or unreadable .render.json");
        return;
    };
    if req.scene.trim().is_empty() || req.scene.contains("..") {
        write_status(folder, &base, false, "missing/invalid `scene` path");
        return;
    }
    if !folder.join(&req.scene).is_file() {
        write_status(folder, &base, false, &format!("scene not found: {}", req.scene));
        return;
    }
    let fps = if req.fps > 0.0 { req.fps.min(MAX_FPS) } else { 30.0 };
    let duration = req.duration;
    if duration.is_nan() || duration <= 0.0 {
        write_status(folder, &base, false, "`duration` must be > 0 seconds");
        return;
    }
    if duration > MAX_DURATION_S {
        write_status(folder, &base, false, &format!("`duration` too long (max {MAX_DURATION_S}s)"));
        return;
    }
    // `out` must be a plain top-level `.mp4` filename — never a path. Blocks
    // `../`, absolute paths, and writing outside the board folder.
    let out = if req.out.trim().is_empty() {
        format!("{base}.mp4")
    } else {
        match safe_mp4_name(req.out.trim()) {
            Some(n) => n,
            None => {
                write_status(folder, &base, false, "`out` must be a plain *.mp4 filename (no path, no ..)");
                return;
            }
        }
    };
    tracing::info!(module = "render", board = %board_id, "render request: scene={} fps={fps} dur={duration} out={out}", req.scene);
    start_render(
        app,
        board_id,
        folder.to_path_buf(),
        &req.scene,
        fps,
        duration,
        &out,
        Some(base),
    );
}

/// Write the `<base>.render.done` (ok=true, body = output filename) or
/// `<base>.render.err` (ok=false, body = reason) status file Codex polls.
fn write_status(folder: &Path, base: &str, ok: bool, body: &str) {
    let name = if ok {
        format!("{base}.render.done")
    } else {
        format!("{base}.render.err")
    };
    if let Err(e) = std::fs::write(folder.join(&name), body) {
        tracing::warn!(module = "render", "write status {name} failed: {e}");
    }
}

/// Tear down the hidden webview for a job (best-effort) and drop its scratch frames.
fn cleanup_job(app: &AppHandle, job_id: &str, frame_dir: &PathBuf) {
    let _ = std::fs::remove_dir_all(frame_dir);
    if let Some(w) = app.get_webview_window(&render_label(job_id)) {
        let _ = w.close();
    }
}

/// Harness callback: persist one captured PNG frame. Bytes arrive base64-encoded
/// (compact string transfer; a raw JSON number array chokes IPC at 1080p).
#[tauri::command]
pub fn render_write_frame(
    job_id: String,
    index: u32,
    b64: String,
    reg: State<Arc<RenderRegistry>>,
) -> Result<(), String> {
    use base64::Engine;
    let job = reg.get(&job_id).ok_or("unknown render job")?;
    if index >= job.total {
        return Err(format!("frame index {index} out of range (total {})", job.total));
    }
    // Cap the encoded size before decoding so a malicious/huge payload can't be
    // expanded into memory (base64 is ~4/3 of the decoded bytes).
    if b64.len() / 4 * 3 > MAX_FRAME_BYTES {
        return Err("frame payload too large".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .map_err(|e| format!("frame b64 decode: {e}"))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err("frame payload too large".into());
    }
    std::fs::create_dir_all(&job.frame_dir).map_err(|e| e.to_string())?;
    let p = job.frame_dir.join(format!("frame_{index:05}.png"));
    std::fs::write(p, &bytes).map_err(|e| e.to_string())?;
    Ok(())
}

/// Harness callback (fired once on load): the scene is loaded and has a canvas, so
/// Rust can start stepping frames. Spawns the driver loop (idempotent per job).
#[tauri::command]
pub fn render_ready(app: AppHandle, job_id: String, reg: State<Arc<RenderRegistry>>) {
    let Some(job) = reg.get(&job_id) else {
        return;
    };
    if job.started.swap(true, Ordering::SeqCst) {
        return; // already driving
    }
    tracing::info!(module = "render", job = %job_id, "ready: driving {} frames", job.total);
    tauri::async_runtime::spawn(drive_render(app, job_id));
}

/// Step the render frame-by-frame from Rust (throttle-immune): eval `__jcap(i)` on
/// the main thread, then wait for `frame_i.png` to appear before the next. Encodes
/// + finalizes when all frames are captured.
async fn drive_render(app: AppHandle, job_id: String) {
    let reg = app.state::<Arc<RenderRegistry>>().inner().clone();
    let Some(job) = reg.get(&job_id) else { return };

    for i in 0..job.total {
        // Bail if the job was torn down meanwhile (e.g. the harness reported a failure).
        if reg.get(&job_id).is_none() {
            return;
        }
        let app_eval = app.clone();
        let label = render_label(&job_id);
        let _ = app.run_on_main_thread(move || {
            if let Some(w) = app_eval.get_webview_window(&label) {
                let _ = w.eval(format!("window.__jcap({i})"));
            }
        });

        let frame = job.frame_dir.join(format!("frame_{i:05}.png"));
        let mut waited = Duration::ZERO;
        while !frame.exists() {
            if reg.get(&job_id).is_none() {
                return; // failed/cancelled
            }
            tokio::time::sleep(FRAME_POLL).await;
            waited += FRAME_POLL;
            if waited >= FRAME_TIMEOUT {
                reg.remove(&job_id);
                cleanup_job(&app, &job_id, &job.frame_dir);
                if let Some(b) = &job.status_base {
                    write_status(&job.folder, b, false, &format!("timed out waiting for frame {i}"));
                }
                tracing::warn!(module = "render", job = %job_id, "timed out at frame {i}");
                return;
            }
        }
    }

    reg.remove(&job_id);
    finish_render(&app, &job_id, &job);
}

/// Encode captured frames → mp4 in the board folder (temp + atomic rename so the
/// media watcher only ingests the finished file), write the status file, clean up.
fn finish_render(app: &AppHandle, job_id: &str, job: &Arc<RenderJob>) {
    let tmp = job.folder.join(format!(".{job_id}.render.mp4"));
    let out_name = unique_mp4_name(&job.folder, &job.out_rel);
    let out = job.folder.join(&out_name);
    if let Err(e) = crate::ffmpeg::encode_frames(&job.frame_dir, job.fps, &tmp) {
        let _ = std::fs::remove_file(&tmp);
        cleanup_job(app, job_id, &job.frame_dir);
        if let Some(b) = &job.status_base {
            write_status(&job.folder, b, false, &format!("ffmpeg encode failed: {e}"));
        }
        tracing::warn!(module = "render", job = %job_id, "encode failed: {e}");
        return;
    }
    let renamed = std::fs::rename(&tmp, &out);
    cleanup_job(app, job_id, &job.frame_dir);
    if let Err(e) = renamed {
        let _ = std::fs::remove_file(&tmp);
        if let Some(b) = &job.status_base {
            write_status(&job.folder, b, false, &format!("rename failed: {e}"));
        }
        return;
    }
    if let Some(b) = &job.status_base {
        write_status(&job.folder, b, true, &out_name);
    }
    tracing::info!(module = "render", job = %job_id, "finalized -> {}", out.display());
}

/// Pick a non-colliding `.mp4` name in `folder` (appends `-2`, `-3`… if taken) so
/// a render never overwrites an existing board file.
fn unique_mp4_name(folder: &Path, name: &str) -> String {
    if !folder.join(name).exists() {
        return name.to_string();
    }
    let stem = name.strip_suffix(".mp4").unwrap_or(name);
    for n in 2..10_000 {
        let cand = format!("{stem}-{n}.mp4");
        if !folder.join(&cand).exists() {
            return cand;
        }
    }
    name.to_string()
}

/// Harness callback: render failed in the page (no canvas, toDataURL threw, JS
/// error). Logged so the reason surfaces; tears down the job (the driver detects
/// the job is gone and aborts) + cleanup.
#[tauri::command]
pub fn render_fail(
    app: AppHandle,
    job_id: String,
    error: String,
    reg: State<Arc<RenderRegistry>>,
) {
    tracing::warn!(module = "render", job = %job_id, "render failed: {error}");
    if let Some(job) = reg.remove(&job_id) {
        cleanup_job(&app, &job_id, &job.frame_dir);
        if let Some(b) = &job.status_base {
            write_status(&job.folder, b, false, &error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::safe_mp4_name;

    #[test]
    fn accepts_plain_mp4_filenames() {
        assert_eq!(safe_mp4_name("intro.mp4").as_deref(), Some("intro.mp4"));
        assert_eq!(safe_mp4_name("a-b_1.MP4").as_deref(), Some("a-b_1.MP4"));
    }

    #[test]
    fn rejects_path_escapes_and_non_mp4() {
        for bad in [
            "../x.mp4",
            "/abs/x.mp4",
            "sub/x.mp4",
            "a\\b.mp4",
            ".hidden.mp4",
            "x.txt",
            "noext",
            "..mp4", // starts with '.'
        ] {
            assert!(safe_mp4_name(bad).is_none(), "should reject {bad:?}");
        }
    }
}
