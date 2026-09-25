//! Extension packages over `linen-ext://<extension id>/<path>` (§7.2, Spike G).
//!
//! Every extension gets its own origin. Its code runs in a Worker created by a
//! host frame of ours on that origin; the Worker's script is served with a CSP
//! that allows no network at all (`connect-src 'none'`), scripts from its own
//! package only, and no nested workers. Network access, when declared, goes
//! through the host (`net.fetch`, Phase 7). Extension UI pages (Navigator tabs,
//! Phase 7) get their own locked-down CSP.

use std::path::{Component, Path, PathBuf};
use tauri::http::{Response, StatusCode};

pub const EXT_SCHEME: &str = "linen-ext";

const HOST_HTML: &str = include_str!("ext/host.html");
const HOST_JS: &str = include_str!("ext/host.js");

/// The host frame: scripts and workers from this origin only; nothing else.
const HOST_CSP: &str = "default-src 'none'; script-src 'self'; worker-src 'self'; \
     connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'";
/// Extension code in its Worker.
const WORKER_CSP: &str = "default-src 'none'; script-src 'self'; connect-src 'none'; \
     worker-src 'none'; font-src 'none'; img-src 'none'";
/// Extension UI pages (Phase 7 Navigator tabs): their own scripts and styles, no network.
const UI_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
     img-src 'self' data:; font-src 'self'; connect-src 'none'; frame-src 'none'; \
     child-src 'none'; worker-src 'none'; form-action 'none'; base-uri 'none'";

type Reply = Response<std::borrow::Cow<'static, [u8]>>;

fn reply(status: StatusCode) -> Reply {
    Response::builder()
        .status(status)
        .body(std::borrow::Cow::Borrowed(&[][..]))
        .unwrap()
}

fn ok(mime: &str, csp: &str, body: std::borrow::Cow<'static, [u8]>) -> Reply {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", mime)
        .header("Content-Security-Policy", csp)
        .header("X-Content-Type-Options", "nosniff")
        .header("Cross-Origin-Resource-Policy", "same-origin")
        .header("Cache-Control", "no-store")
        .body(body)
        .unwrap()
}

/// An extension id: lowercase letters, digits, dots and dashes (reverse-DNS style).
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
        && !id.starts_with('.')
}

/// A path inside a package: plain components only.
fn package_path(root: &Path, id: &str, path: &str) -> Option<PathBuf> {
    let rel = Path::new(path.trim_start_matches('/'));
    if rel.as_os_str().is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    Some(root.join(id).join(rel))
}

/// Serve one request. `root` is the Extensions folder; `id` the URL's host.
pub fn serve(root: &Path, id: &str, path: &str) -> Reply {
    if !valid_id(id) {
        return reply(StatusCode::BAD_REQUEST);
    }
    match path {
        "/_host.html" => {
            return ok(
                "text/html; charset=utf-8",
                HOST_CSP,
                HOST_HTML.as_bytes().into(),
            )
        }
        "/_host.js" => {
            return ok(
                "text/javascript; charset=utf-8",
                HOST_CSP,
                HOST_JS.as_bytes().into(),
            )
        }
        _ => {}
    }
    let Some(file) = package_path(root, id, path) else {
        return reply(StatusCode::FORBIDDEN);
    };
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let (mime, csp) = match ext.as_str() {
        "js" | "mjs" => ("text/javascript; charset=utf-8", WORKER_CSP),
        "html" => ("text/html; charset=utf-8", UI_CSP),
        "css" => ("text/css; charset=utf-8", UI_CSP),
        "json" => ("application/json", UI_CSP),
        "png" => ("image/png", UI_CSP),
        "svg" => ("image/svg+xml", UI_CSP),
        "woff2" => ("font/woff2", UI_CSP),
        _ => return reply(StatusCode::FORBIDDEN),
    };
    // The package folder must really contain the file (no symlinks out of it).
    let (Ok(real), Ok(base)) = (file.canonicalize(), root.join(id).canonicalize()) else {
        return reply(StatusCode::NOT_FOUND);
    };
    if !real.starts_with(&base) {
        return reply(StatusCode::FORBIDDEN);
    }
    match std::fs::read(&real) {
        Ok(bytes) => ok(mime, csp, bytes.into()),
        Err(_) => reply(StatusCode::NOT_FOUND),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_paths_stay_inside_the_package() {
        assert!(valid_id("org.example.dictionary"));
        assert!(!valid_id("../x") && !valid_id("A") && !valid_id(""));
        let root = Path::new("/ext");
        assert!(package_path(root, "a", "/main.js").is_some());
        assert!(package_path(root, "a", "/../b/main.js").is_none());
        assert!(package_path(root, "a", "/").is_none());
    }

    #[test]
    fn scripts_get_the_worker_policy_and_unknown_types_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a")).unwrap();
        std::fs::write(dir.path().join("a/main.js"), "1").unwrap();
        std::fs::write(dir.path().join("a/x.exe"), "1").unwrap();
        let r = serve(dir.path(), "a", "/main.js");
        assert_eq!(r.status(), StatusCode::OK);
        let csp = r.headers()["Content-Security-Policy"].to_str().unwrap();
        assert!(csp.contains("connect-src 'none'") && csp.contains("worker-src 'none'"));
        assert_eq!(
            serve(dir.path(), "a", "/x.exe").status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            serve(dir.path(), "a", "/_host.html").status(),
            StatusCode::OK
        );
    }
}
