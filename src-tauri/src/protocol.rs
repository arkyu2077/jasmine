//! Jasmine image protocol handler. Serves image bytes from a Board folder.
//!
//! URL shape is platform-dependent: WebKit-style webviews request
//! `jasmine://localhost/<boardId>/<rel-path>`, while WebView2 requests
//! `http://jasmine.localhost/<boardId>/<rel-path>`.
//!
//! Path canonicalization + traversal guard ported from Riff's `riff://` scheme:
//! reject `..`/absolute components, then verify the canonical path stays inside
//! the Board folder.

use crate::board::BoardRegistry;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, PathBuf};
use std::sync::Arc;
use tauri::http::{header, Request, Response, StatusCode, Uri};
use tauri::{Manager, UriSchemeContext};

fn parse_jasmine_uri(uri: &Uri) -> Result<(String, String), (StatusCode, &'static str)> {
    let host = uri.host().unwrap_or_default();
    let path = uri.path().trim_start_matches('/');

    // Tauri/WebView2 represents custom protocols as `http://<scheme>.localhost/...`
    // on Windows. Keep board routing in the path and only support host-as-board
    // for legacy host-scoped `<scheme>://<boardId>/<rel-path>` URLs.
    if !host.is_empty() && host != "localhost" && !host.ends_with(".localhost") {
        if path.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                "missing image path in jasmine:// URL",
            ));
        }
        return Ok((host.to_string(), path.to_string()));
    }

    let Some((board_raw, rel_raw)) = path.split_once('/') else {
        return Err((StatusCode::BAD_REQUEST, "missing boardId in jasmine:// URL"));
    };
    if board_raw.is_empty() || rel_raw.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "missing boardId or image path in jasmine:// URL",
        ));
    }
    let board_id = urlencoding::decode(board_raw)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| board_raw.to_string());
    Ok((board_id, rel_raw.to_string()))
}

pub fn handle_jasmine_uri<R: tauri::Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let uri = request.uri();

    let (board_id, rel_raw) = match parse_jasmine_uri(uri) {
        Ok(parsed) => parsed,
        Err((code, msg)) => return error_response(code, msg),
    };

    let app = ctx.app_handle();
    let registry = app.state::<Arc<BoardRegistry>>();
    let folder = match registry.folder(&board_id) {
        Some(f) => f,
        None => return error_response(StatusCode::NOT_FOUND, "unknown board"),
    };

    let rel = urlencoding::decode(&rel_raw)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| rel_raw.to_string());

    let rel_path = PathBuf::from(&rel);
    for comp in rel_path.components() {
        if !matches!(comp, Component::Normal(_)) {
            return error_response(StatusCode::FORBIDDEN, "path traversal blocked");
        }
    }

    let abs = folder.join(&rel_path);
    let canonical = match std::fs::canonicalize(&abs) {
        Ok(p) => p,
        Err(_) => return error_response(StatusCode::NOT_FOUND, "file not found"),
    };
    let base_canonical = match std::fs::canonicalize(&folder) {
        Ok(p) => p,
        Err(_) => return error_response(StatusCode::NOT_FOUND, "board folder gone"),
    };
    if !canonical.starts_with(&base_canonical) {
        return error_response(StatusCode::FORBIDDEN, "escape attempt blocked");
    }

    let mime = mime_guess::from_path(&canonical)
        .first_or_octet_stream()
        .to_string();

    let total = match std::fs::metadata(&canonical) {
        Ok(m) => m.len(),
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    // Range support: video elements (WebKit + WebView2) request byte ranges to
    // seek/scrub and to avoid buffering the whole file. Honor `Range` with a 206;
    // a malformed/unsatisfiable range gets a 416 with `Content-Range: bytes */len`.
    let range_header = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok());

    if let Some(rh) = range_header {
        return match parse_range(rh, total) {
            Some((start, end)) => match read_slice(&canonical, start, end) {
                Ok(bytes) => Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_TYPE, mime)
                    .header(header::ACCEPT_RANGES, "bytes")
                    .header(
                        header::CONTENT_RANGE,
                        format!("bytes {start}-{end}/{total}"),
                    )
                    .header(header::CONTENT_LENGTH, (end - start + 1).to_string())
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Cache-Control", "no-store")
                    .body(bytes)
                    .unwrap_or_else(|_| {
                        error_response(StatusCode::INTERNAL_SERVER_ERROR, "build response")
                    }),
                Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
            },
            None => Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{total}"))
                .header("Access-Control-Allow-Origin", "*")
                .body(Vec::new())
                .unwrap_or_else(|_| {
                    error_response(StatusCode::INTERNAL_SERVER_ERROR, "build response")
                }),
        };
    }

    let bytes = match std::fs::read(&canonical) {
        Ok(b) => b,
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        // Advertise range support so the webview's video element knows it can
        // range-request subsequent reads instead of pulling the whole file.
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, total.to_string())
        // The webview origin (http://localhost:1420 in dev, tauri://localhost in
        // prod) fetches this cross-origin for textures.
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-store")
        .body(bytes)
        .unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "build response"))
}

/// Parse a single HTTP byte range against a known total length. Handles
/// `bytes=start-end`, `bytes=start-` (open-ended), and `bytes=-suffix` (last N).
/// Returns an inclusive `(start, end)`, or `None` if malformed/unsatisfiable.
fn parse_range(header_val: &str, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let spec = header_val.strip_prefix("bytes=")?;
    // We serve only the first range if a multi-range request arrives.
    let first = spec.split(',').next()?.trim();
    let (a, b) = first.split_once('-')?;
    let (start, end) = if a.is_empty() {
        // suffix range: last N bytes
        let n: u64 = b.trim().parse().ok()?;
        if n == 0 {
            return None;
        }
        let n = n.min(total);
        (total - n, total - 1)
    } else {
        let start: u64 = a.trim().parse().ok()?;
        let end = if b.trim().is_empty() {
            total - 1
        } else {
            b.trim().parse::<u64>().ok()?.min(total - 1)
        };
        (start, end)
    };
    if start > end || start >= total {
        return None;
    }
    Some((start, end))
}

/// Read an inclusive byte range `[start, end]` from a file without loading the
/// whole file into memory.
fn read_slice(path: &std::path::Path, start: u64, end: u64) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let len = (end - start + 1) as usize;
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

fn error_response(code: StatusCode, msg: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header("Access-Control-Allow-Origin", "*")
        .body(msg.as_bytes().to_vec())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::parse_jasmine_uri;
    use tauri::http::Uri;

    #[test]
    fn parses_path_scoped_jasmine_url() {
        let uri: Uri = "jasmine://localhost/board-1/gen-20260526.png"
            .parse()
            .unwrap();
        let (board, rel) = parse_jasmine_uri(&uri).unwrap();
        assert_eq!(board, "board-1");
        assert_eq!(rel, "gen-20260526.png");
    }

    #[test]
    fn parses_windows_webview2_custom_protocol_shape() {
        let uri: Uri = "http://jasmine.localhost/board-1/gen-20260526.png"
            .parse()
            .unwrap();
        let (board, rel) = parse_jasmine_uri(&uri).unwrap();
        assert_eq!(board, "board-1");
        assert_eq!(rel, "gen-20260526.png");
    }

    #[test]
    fn keeps_legacy_host_scoped_urls_working() {
        let uri: Uri = "jasmine://board-1/gen-20260526.png".parse().unwrap();
        let (board, rel) = parse_jasmine_uri(&uri).unwrap();
        assert_eq!(board, "board-1");
        assert_eq!(rel, "gen-20260526.png");
    }

    use super::parse_range;

    #[test]
    fn range_parses_closed_open_and_suffix() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=100-", 1000), Some((100, 999))); // open-ended
        assert_eq!(parse_range("bytes=-200", 1000), Some((800, 999))); // suffix
        // end past EOF clamps to last byte.
        assert_eq!(parse_range("bytes=900-5000", 1000), Some((900, 999)));
        // suffix longer than file clamps to whole file.
        assert_eq!(parse_range("bytes=-5000", 1000), Some((0, 999)));
    }

    #[test]
    fn range_rejects_unsatisfiable() {
        assert_eq!(parse_range("bytes=1000-1001", 1000), None); // start >= total
        assert_eq!(parse_range("bytes=500-100", 1000), None); // start > end
        assert_eq!(parse_range("bytes=0-0", 0), None); // empty file
        assert_eq!(parse_range("items=0-1", 1000), None); // wrong unit
        assert_eq!(parse_range("bytes=-0", 1000), None); // zero-length suffix
    }
}
