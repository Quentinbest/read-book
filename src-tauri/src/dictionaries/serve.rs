//! DX6: the `linen-dict://<generation>/` scheme. Each generation is its own origin;
//! entries are served as whole sanitised documents (`_entry?q=…`), resources from the
//! MDD files by their normalised path, and every response carries the CSP. Only an
//! active generation, or one an open peek still uses (DX2), is served. Audio is never
//! served (DX8).

use super::lookup::Opened;
use super::sanitise::{entry_document, safe_path, sanitise_css, CARD_CSS, CSP};
use std::borrow::Cow;
use tauri::http::{Response, StatusCode};

pub const DICT_SCHEME: &str = "linen-dict";

type Reply = Response<Cow<'static, [u8]>>;

fn reply(status: StatusCode) -> Reply {
    Response::builder()
        .status(status)
        .header("Content-Security-Policy", CSP)
        .body(Cow::Borrowed(&b""[..]))
        .unwrap()
}

fn ok(mime: &str, body: Vec<u8>) -> Reply {
    Response::builder()
        .header("Content-Type", mime)
        .header("Content-Security-Policy", CSP)
        .header("X-Content-Type-Options", "nosniff")
        .header("Cache-Control", "no-store")
        .body(Cow::Owned(body))
        .unwrap()
}

fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == name).then(|| {
            let v = v.replace('+', " ");
            let b = v.as_bytes();
            let mut out = Vec::with_capacity(b.len());
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'%' && i + 2 < b.len() {
                    if let Ok(x) = u8::from_str_radix(&v[i + 1..i + 3], 16) {
                        out.push(x);
                        i += 3;
                        continue;
                    }
                }
                out.push(b[i]);
                i += 1;
            }
            String::from_utf8_lossy(&out).into_owned()
        })
    })
}

/// Serve one request for an opened, allowed generation.
pub fn serve(opened: Option<&Opened>, path: &str, query: &str) -> Reply {
    let Some(o) = opened else {
        return reply(StatusCode::NOT_FOUND);
    };
    match path {
        "/_card.css" => return ok("text/css; charset=utf-8", CARD_CSS.as_bytes().to_vec()),
        "/_entry" => {
            let q = query_param(query, "q").unwrap_or_default();
            let audio =
                query_param(query, "audio").unwrap_or_else(|| "Audio isn't available".into());
            let entries = match o.look_up(&q) {
                Ok(e) => e,
                Err(e) => {
                    log::warn!("dictionary entry: {e}");
                    return reply(StatusCode::UNPROCESSABLE_ENTITY);
                }
            };
            if entries.is_empty() {
                return reply(StatusCode::NOT_FOUND);
            }
            let for_word = entries[0].for_word.clone();
            let parts: Vec<(String, String)> =
                entries.into_iter().map(|e| (e.headword, e.html)).collect();
            let doc = entry_document(&parts, &o.generation, &audio, for_word.as_deref());
            return ok("text/html; charset=utf-8", doc.into_bytes());
        }
        _ => {}
    }
    let Some(p) = safe_path(path) else {
        return reply(StatusCode::FORBIDDEN);
    };
    let ext = p.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let mime = match ext.as_str() {
        "css" => "text/css; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        // DX8 and everything else: not served.
        _ => return reply(StatusCode::FORBIDDEN),
    };
    match o.resource(&p) {
        Ok(Some(bytes)) if ext == "css" => {
            let css = String::from_utf8_lossy(&bytes);
            ok(mime, sanitise_css(&css, "").into_bytes())
        }
        Ok(Some(bytes)) => ok(mime, bytes),
        Ok(None) => reply(StatusCode::NOT_FOUND),
        Err(e) => {
            log::warn!("dictionary resource: {e}");
            reply(StatusCode::UNPROCESSABLE_ENTITY)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionaries::generation;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;

    fn opened() -> (tempfile::TempDir, Opened) {
        let tmp = tempfile::tempdir().unwrap();
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mdx/basic.mdx");
        let b = generation::build(
            tmp.path(),
            &generation::sources(&src).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let o = Opened::open(tmp.path(), &b.generation).unwrap();
        (tmp, o)
    }

    #[test]
    fn entries_resources_and_refusals() {
        let (_t, o) = opened();
        let r = serve(Some(&o), "/_entry", "q=invalidates&audio=No%20audio");
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers()["Content-Security-Policy"]
            .to_str()
            .unwrap()
            .contains("default-src 'none'"));
        let body = String::from_utf8(r.body().to_vec()).unwrap();
        assert!(
            body.contains("in·val·i·date") && body.contains("for “invalidates”"),
            "{body}"
        );
        assert_eq!(serve(Some(&o), "/test.css", "").status(), StatusCode::OK);
        assert_eq!(serve(Some(&o), "/dot.png", "").status(), StatusCode::OK);
        assert_eq!(
            serve(Some(&o), "/../x.png", "").status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            serve(Some(&o), "/%2e%2e/x.png", "").status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            serve(Some(&o), "/pronounce.mp3", "").status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            serve(Some(&o), "/_entry", "q=zzzz").status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            serve(None, "/_entry", "q=cache").status(),
            StatusCode::NOT_FOUND
        );
    }
}
