//! DX6, DX8, DX15: entry HTML made safe before it reaches WebKit.
//!
//! Dictionary content is untrusted input, like book content. The core rebuilds each
//! entry from an allowlist (`ammonia`): no scripts, event handlers, forms, frames,
//! `<base>`, `<meta>`, `<style>` or link prefetching survive. URLs are rewritten:
//! a relative path becomes `linen-dict://<generation>/<path>` (traversal refused),
//! `entry://word` becomes the frame's own entry address, sound links lose their target
//! (DX8), and web links lose theirs (DX15): they stay as text and never open anything.
//! CSS, in style attributes and in the dictionary's style sheets, keeps only local
//! `url()`s; `@import`, remote URLs (escaped or not) and legacy script hooks are removed.
//! The document is served with `default-src 'none'`, so a miss here still loads nothing.

use std::borrow::Cow;
use std::collections::HashSet;

/// The CSP an entry document and its resources are served with.
pub const CSP: &str = "default-src 'none'; style-src linen-dict: 'unsafe-inline'; \
                       img-src linen-dict: data:; font-src linen-dict:; \
                       base-uri 'none'; form-action 'none'; frame-ancestors tauri: http://tauri.localhost";

/// DX6: a resource path, normalised, or None when it could leave the dictionary.
pub fn safe_path(raw: &str) -> Option<String> {
    let decoded = percent_decode(raw.trim());
    if decoded.contains(':') || decoded.starts_with("//") || decoded.starts_with("\\\\") {
        return None;
    }
    let path = decoded.replace('\\', "/");
    let path = path.split(['?', '#']).next().unwrap_or("");
    let mut parts = vec![];
    for seg in path.split('/') {
        match seg {
            "" if parts.is_empty() => continue, // a leading slash: the dictionary's root
            "" | "." | ".." => return None,
            s if s.chars().any(|c| c.is_control()) => return None,
            s => parts.push(s),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn encode_component(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The address of an entry inside a generation (DX5: `entry://` links open in the peek).
pub fn entry_url(generation: &str, word: &str) -> String {
    format!(
        "linen-dict://{generation}/_entry?q={}",
        encode_component(word)
    )
}

/// CSS escapes decoded (`u\72 l(` is `url(`), so nothing hides from the checks below.
fn unescape_css(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let mut hex = String::new();
        while hex.len() < 6 && chars.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
            hex.push(chars.next().unwrap());
        }
        if hex.is_empty() {
            if let Some(n) = chars.next() {
                if n != '\n' {
                    out.push(n);
                }
            }
        } else {
            if chars.peek().is_some_and(|c| c.is_whitespace()) {
                chars.next();
            }
            out.push(
                u32::from_str_radix(&hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .filter(|&c| c != '\0')
                    .unwrap_or('\u{FFFD}'),
            );
        }
    }
    out
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        match rest[i + 2..].find("*/") {
            Some(j) => rest = &rest[i + 2 + j + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// DX6: CSS with only local `url()`s, rewritten to `prefix` (empty: left relative).
pub fn sanitise_css(css: &str, prefix: &str) -> String {
    let css = strip_comments(&unescape_css(css));
    // @import and @charset rules go, up to their semicolon.
    let mut s = String::with_capacity(css.len());
    let mut rest = css.as_str();
    loop {
        let lower = rest.to_ascii_lowercase();
        let at = ["@import", "@charset", "@namespace"]
            .iter()
            .filter_map(|k| lower.find(k))
            .min();
        match at {
            Some(i) => {
                s.push_str(&rest[..i]);
                match rest[i..].find(';') {
                    Some(j) => rest = &rest[i + j + 1..],
                    None => {
                        rest = "";
                    }
                }
            }
            None => {
                s.push_str(rest);
                break;
            }
        }
    }
    // url(…): local paths are kept, anything else becomes nothing.
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    loop {
        let lower = rest.to_ascii_lowercase();
        let Some(i) = lower.find("url(") else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..i]);
        let after = &rest[i + 4..];
        let end = after.find(')').unwrap_or(after.len());
        let inner = after[..end].trim().trim_matches(|c| c == '"' || c == '\'');
        match safe_path(inner) {
            Some(p) if !prefix.is_empty() => out.push_str(&format!("url(\"{prefix}{p}\")")),
            Some(p) => out.push_str(&format!("url(\"{p}\")")),
            None => out.push_str("none"),
        }
        rest = after.get(end + 1..).unwrap_or("");
    }
    // Quoted remote URLs outside url() (image-set, src lists) and old script hooks.
    let mut cleaned = String::with_capacity(out.len());
    let mut chars = out.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' || c == '\'' {
            let mut lit = String::new();
            for n in chars.by_ref() {
                if n == c {
                    break;
                }
                lit.push(n);
            }
            let remote = lit.contains("://")
                || lit.starts_with("//")
                || lit.to_ascii_lowercase().starts_with("javascript:");
            if remote && !lit.starts_with("linen-dict://") {
                cleaned.push_str("\"\"");
            } else {
                cleaned.push(c);
                cleaned.push_str(&lit);
                cleaned.push(c);
            }
        } else {
            cleaned.push(c);
        }
    }
    let lower = cleaned.to_ascii_lowercase();
    if ["expression(", "behavior:", "-moz-binding", "javascript:"]
        .iter()
        .any(|k| lower.contains(k))
    {
        let mut c = cleaned;
        for k in ["expression(", "behavior:", "-moz-binding", "javascript:"] {
            while let Some(i) = c.to_ascii_lowercase().find(k) {
                c.replace_range(i..i + k.len(), "x-blocked");
            }
        }
        return c;
    }
    cleaned
}

/// Style sheets an entry links to (`<link rel="stylesheet" href="x.css">`), as safe paths.
fn stylesheet_links(html: &str) -> Vec<String> {
    let mut out = vec![];
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<link") {
        let start = from + i;
        let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
        let tag = &html[start..end];
        let tag_lower = &lower[start..end];
        if tag_lower.contains("stylesheet") {
            if let Some(h) = tag_lower.find("href") {
                let v = tag[h + 4..]
                    .trim_start()
                    .trim_start_matches('=')
                    .trim_start();
                let quote = v.chars().next().filter(|c| *c == '"' || *c == '\'');
                let value = match quote {
                    Some(q) => v[1..].split(q).next().unwrap_or(""),
                    None => v.split(char::is_whitespace).next().unwrap_or(""),
                };
                if let Some(p) = safe_path(value) {
                    if !out.contains(&p) {
                        out.push(p);
                    }
                }
            }
        }
        from = end.max(start + 5);
    }
    out
}

/// DX8: sound links say audio isn't available; nothing plays.
fn mark_audio(html: &str, label: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find("sound://") {
        // Back up to the attribute's opening quote and name.
        let before = &rest[..i];
        let q = before.rfind(['"', '\'']);
        let eq = q.and_then(|q| before[..q].rfind('='));
        let name_start = eq.and_then(|e| before[..e].trim_end().rfind(|c: char| c.is_whitespace()));
        match (q, name_start) {
            (Some(q), Some(n)) => {
                let quote = before.as_bytes()[q] as char;
                let close = rest[i..].find(quote).map_or(rest.len(), |j| i + j + 1);
                out.push_str(&rest[..n + 1]);
                out.push_str(&format!(
                    "title=\"{}\" data-audio=\"\"",
                    label.replace('"', "&quot;")
                ));
                rest = &rest[close..];
            }
            _ => {
                out.push_str(&rest[..i + 8]);
                rest = &rest[i + 8..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The entry body, sanitised; `generation` names the frame's own origin.
pub fn sanitise_html(raw: &str, generation: &str, audio_label: &str) -> String {
    let raw = mark_audio(raw, audio_label);
    let prefix = format!("linen-dict://{generation}/");
    let generation = generation.to_string();
    let mut b = ammonia::Builder::default();
    b.add_tags(&[
        "font", "big", "tt", "u", "s", "strike", "details", "summary", "ruby", "rt", "rp", "wbr",
        "center",
    ])
    .add_generic_attributes(&[
        "class",
        "id",
        "lang",
        "dir",
        "style",
        "title",
        "align",
        "data-audio",
    ])
    .add_tag_attributes("font", &["color", "size", "face"])
    .add_tag_attributes("img", &["src", "alt", "width", "height"])
    .add_tag_attributes("td", &["colspan", "rowspan"])
    .add_tag_attributes("th", &["colspan", "rowspan"])
    .url_schemes(
        ["http", "https", "entry", "linen-dict", "mailto"]
            .into_iter()
            .collect::<HashSet<_>>(),
    )
    .url_relative(ammonia::UrlRelative::PassThrough)
    .link_rel(None)
    .attribute_filter(
        move |element, attribute, value| match (element, attribute) {
            (_, "style") => Some(Cow::Owned(sanitise_css(value, &prefix))),
            ("a", "href") => {
                let lower = value.trim().to_ascii_lowercase();
                if let Some(word) = lower.strip_prefix("entry://").map(|_| &value.trim()[8..]) {
                    let word = percent_decode(word.trim_end_matches('/'));
                    Some(Cow::Owned(entry_url(&generation, &word)))
                } else {
                    // DX15: web links stay as text; nothing opens a browser.
                    None
                }
            }
            (_, "src") => safe_path(value).map(|p| Cow::Owned(format!("{prefix}{p}"))),
            (_, "href") => None,
            _ => Some(Cow::Borrowed(value)),
        },
    );
    b.clean(&raw).to_string()
}

/// A whole entry document: the dictionary's style sheets (sanitised when served), the
/// light card (DX7) and the entries, separated by rules.
pub fn entry_document(
    entries: &[(String, String)],
    generation: &str,
    audio_label: &str,
    for_word: Option<&str>,
) -> String {
    let mut sheets = vec![];
    for (_, html) in entries {
        for s in stylesheet_links(html) {
            if !sheets.contains(&s) {
                sheets.push(s);
            }
        }
    }
    let links: String = sheets
        .iter()
        .map(|s| format!("<link rel=\"stylesheet\" href=\"linen-dict://{generation}/{s}\">"))
        .collect();
    let body: Vec<String> = entries
        .iter()
        .map(|(_, html)| {
            format!(
                "<section class=\"linen-entry\">{}</section>",
                sanitise_html(html, generation, audio_label)
            )
        })
        .collect();
    let label = for_word
        .map(|w| {
            format!(
                "<p class=\"linen-for\">for “{}”</p>",
                ammonia::clean_text(w)
            )
        })
        .unwrap_or_default();
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">\
         <link rel=\"stylesheet\" href=\"linen-dict://{generation}/_card.css\">{links}</head>\
         <body>{label}{}</body></html>",
        body.join("<hr class=\"linen-between\">")
    )
}

/// DX7: the light card the entry sits on in every theme (O4, approved 2026-10-10, item 79).
pub const CARD_CSS: &str =
    "html{color-scheme:light}body{margin:0;padding:10px 12px;background:#fffdf9;\
color:#22201c;font:15px/1.5 -apple-system,system-ui,sans-serif;overflow-wrap:anywhere}\
img{max-width:100%;height:auto}.linen-for{margin:0 0 6px;font-size:12px;color:#5c574f}\
hr.linen-between{border:0;border-top:1px solid #e4ddd1;margin:12px 0}\
[data-audio]{cursor:default;opacity:.6;text-decoration:none}";

#[cfg(test)]
mod tests {
    use super::*;

    const G: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn paths_stay_inside_the_dictionary() {
        assert_eq!(safe_path("img/a.png").as_deref(), Some("img/a.png"));
        assert_eq!(safe_path("\\img\\a.png").as_deref(), Some("img/a.png"));
        assert_eq!(safe_path("/abs.css").as_deref(), Some("abs.css"));
        for bad in [
            "../../../etc/passwd",
            "%2e%2e/%2e%2e/secret.png",
            "a/../b",
            "http://x/y",
            "//x/y",
            "javascript:alert(1)",
            "",
            "a//b",
        ] {
            assert_eq!(safe_path(bad), None, "{bad}");
        }
    }

    #[test]
    fn nothing_runs_and_nothing_remote_survives() {
        let hostile = [
            r#"<p>before</p><script>fetch("http://canary/script")</script><p>after</p>"#,
            r#"<p onclick="alert(1)" onmouseover="x()">click</p><img src=x onerror="fetch('http://canary/onerror')">"#,
            r#"<a href="javascript:alert(1)">js</a><a href="http://canary/web">web</a>"#,
            r#"<base href="http://canary/base/"><img src="a.png">"#,
            r#"<meta http-equiv="refresh" content="0;url=http://canary/refresh">"#,
            r#"<link rel="prefetch" href="http://canary/prefetch"><link rel="preload" href="http://canary/preload">"#,
            r#"<img src="http://canary/img"><img srcset="http://canary/srcset 2x">"#,
            r#"<iframe src="http://canary/frame"></iframe><object data="x"></object><form action="http://canary/form"><input></form>"#,
            r#"<svg><script>fetch("http://canary/svg")</script><circle r="2"/></svg>"#,
            r#"<p style="background:url(http://canary/style)">s</p><style>@import "http://canary/import";</style>"#,
        ];
        for h in hostile {
            let out = sanitise_html(h, G, "Audio isn't available");
            let lower = out.to_ascii_lowercase();
            for bad in [
                "<script",
                "onclick",
                "onerror",
                "onmouseover",
                "javascript:",
                "canary",
                "<iframe",
                "<object",
                "<form",
                "<base",
                "<meta",
                "<style",
                "srcset",
                "<link",
            ] {
                assert!(!lower.contains(bad), "{bad} survived in {out}");
            }
        }
        assert!(
            sanitise_html("<p>before</p><script>x</script><p>after</p>", G, "")
                .contains("<p>after</p>")
        );
    }

    #[test]
    fn links_inside_the_dictionary_navigate_and_web_links_are_text() {
        let out = sanitise_html(
            r#"<a href="entry://invalidate">see</a> <a href="https://example.org">site</a>"#,
            G,
            "",
        );
        assert!(
            out.contains(&format!("href=\"linen-dict://{G}/_entry?q=invalidate\"")),
            "{out}"
        );
        assert!(
            out.contains("<a>site</a>")
                || out.contains(">site</a>") && !out.contains("example.org"),
            "{out}"
        );
        let img = sanitise_html(r#"<img src="dot.png"><img src="../x.png">"#, G, "");
        assert!(img.contains(&format!("src=\"linen-dict://{G}/dot.png\"")));
        assert!(!img.contains("x.png"));
    }

    #[test]
    fn sound_links_say_audio_is_not_available() {
        let out = sanitise_html(
            r#"<a href="sound://pronounce.mp3">&#9654;</a>"#,
            G,
            "Audio isn't available",
        );
        assert!(
            out.contains("title=\"Audio isn't available\"") && out.contains("data-audio"),
            "{out}"
        );
        assert!(!out.contains("sound://") && !out.contains("mp3"), "{out}");
    }

    #[test]
    fn css_keeps_local_urls_only_however_written() {
        let css = r#"@import "http://canary/import"; .a { background: url(img/a.png) }
            p { background: u\72 l(http://canary/escaped) }
            div { background-image: image-set("http://canary/imageset" 1x) }
            @font-face { font-family: x; src: url(http://canary/font) }
            .b { width: expression(alert(1)) } /* url(http://canary/comment) */"#;
        let out = sanitise_css(css, "linen-dict://g/");
        assert!(
            !out.contains("canary") && !out.contains("@import") && !out.contains("expression("),
            "{out}"
        );
        assert!(out.contains("url(\"linen-dict://g/img/a.png\")"), "{out}");
        assert!(sanitise_css("a{b:url('x.png')}", "").contains("url(\"x.png\")"));
    }

    #[test]
    fn the_document_links_its_style_sheets_and_the_card() {
        let doc = entry_document(
            &[(
                "a".into(),
                r#"<link rel="stylesheet" href="test.css"><div class="hw">a</div>"#.into(),
            )],
            G,
            "",
            Some("as"),
        );
        assert!(doc.contains(&format!("href=\"linen-dict://{G}/test.css\"")));
        assert!(doc.contains(&format!("href=\"linen-dict://{G}/_card.css\"")));
        assert!(doc.contains("default-src 'none'"));
        assert!(doc.contains("for “as”"));
        assert!(doc.contains("class=\"hw\""));
    }
}
