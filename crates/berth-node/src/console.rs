use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "$OUT_DIR/console"]
struct ConsoleAssets;

const CSP_LOOPBACK: &str = "default-src 'self'; connect-src 'self'; img-src 'self' data: blob:; style-src 'self'; script-src 'self'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'; frame-src http://127.0.0.1:* http://localhost:* http://[::1]:*";
const CSP_REMOTE: &str = "default-src 'self'; connect-src 'self'; img-src 'self' data: blob:; style-src 'self'; script-src 'self'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'; frame-src 'none'";

pub(crate) fn content_security_policy(loopback: bool) -> &'static str {
    if loopback { CSP_LOOPBACK } else { CSP_REMOTE }
}

pub(crate) fn response(request_path: &str, loopback: bool) -> Response {
    let path = request_path.trim_start_matches('/');
    if path.contains("..") {
        return send_index(loopback);
    }
    if path.is_empty() || path == "index.html" {
        return send_index(loopback);
    }
    match ConsoleAssets::get(path) {
        Some(file) => send_file(path, file.data.as_ref(), false, loopback),
        None => send_index(loopback),
    }
}

fn send_index(loopback: bool) -> Response {
    match ConsoleAssets::get("index.html") {
        Some(file) => send_file("index.html", file.data.as_ref(), true, loopback),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn send_file(path: &str, bytes: &[u8], is_index: bool, loopback: bool) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let content_type = HeaderValue::from_str(mime.essence_str())
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    let cache = if is_index {
        HeaderValue::from_static("no-cache")
    } else {
        HeaderValue::from_static("public, max-age=31536000, immutable")
    };
    let csp = HeaderValue::from_static(content_security_policy(loopback));
    (
        [
            (CONTENT_TYPE, content_type),
            (CACHE_CONTROL, cache),
            (HeaderName::from_static("content-security-policy"), csp),
        ],
        bytes.to_vec(),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csp_loopback_allows_local_frames() {
        let csp = content_security_policy(true);
        assert!(csp.contains("default-src 'self'"), "{csp}");
        assert!(
            csp.contains("frame-src http://127.0.0.1:* http://localhost:* http://[::1]:*"),
            "{csp}"
        );
        assert!(!csp.contains("unsafe-eval"), "{csp}");
        assert!(!csp.contains("unsafe-inline"), "{csp}");
    }

    #[test]
    fn csp_remote_blocks_frames() {
        let csp = content_security_policy(false);
        assert!(csp.contains("default-src 'self'"), "{csp}");
        assert!(csp.contains("frame-src 'none'"), "{csp}");
        assert!(!csp.contains("127.0.0.1:*"), "{csp}");
        assert!(!csp.contains("unsafe-eval"), "{csp}");
    }
}
