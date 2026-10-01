//! Extension manifests (P1), permissions (P3) and API compatibility (P5).
//!
//! A manifest declares everything an extension does: what it contributes to the
//! slots, when it activates, and what it may access. Anything not declared is
//! refused. Validation is strict and every refusal says why, in words the install
//! sheet can show.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The Host API this build provides. The core supports this major version and the
/// previous one (P5); with API 1 there is no previous one yet.
pub const API_VERSION: &str = "1.0.0";
pub const SUPPORTED_MAJORS: &[u64] = &[1];

/// Limits that keep manifests small and slots tidy.
const MAX_CONTRIBUTIONS: usize = 32;
const MAX_TEXT: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub version: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub publisher: Option<String>,
    pub engines: Engines,
    /// The Worker script. Theme packs have none.
    #[serde(default)]
    pub main: Option<String>,
    /// `onCommand:<id>`, `onNavigatorTab:<id>`, `onExport:<id>`, `onAnnotations`,
    /// `onReadingSessions` (1.1).
    #[serde(default)]
    pub activation: Vec<String>,
    #[serde(default)]
    pub contributes: Contributes,
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Reserved (D4): localisation is English only in the MVP.
    #[serde(default)]
    pub locales: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Engines {
    /// A semver range of the Host API, e.g. "^1.0".
    pub linen: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Contributes {
    #[serde(default)]
    pub commands: Vec<Command>,
    #[serde(default)]
    pub selection_actions: Vec<SelectionAction>,
    #[serde(default)]
    pub navigator_tabs: Vec<NavigatorTab>,
    #[serde(default)]
    pub themes: Vec<ThemePack>,
    #[serde(default)]
    pub exporters: Vec<Exporter>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionAction {
    pub command: String,
    /// e.g. "selection.words <= 3" (see `when` in the host).
    #[serde(default)]
    pub when: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigatorTab {
    pub id: String,
    pub title: String,
    /// The tab's page in the package (a UI page, P4).
    pub page: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemePack {
    pub id: String,
    pub title: String,
    /// `light` or `dark`: which native controls and scrollbars to use.
    pub scheme: String,
    /// Token values (P9): the theme's colours, by token name.
    pub tokens: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exporter {
    pub id: String,
    pub title: String,
    /// The command that performs the export.
    pub command: String,
}

/// How a permission is consented to (P3), for the install sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Consent {
    AtInstall,
    Highlighted,
    StronglyWarned,
    EachUse,
}

/// The permissions P3 defines, with how each is consented to.
pub fn permission_consent(p: &str) -> Option<Consent> {
    Some(match p {
        "book.metadata" | "book.selection" | "library.read" | "annotations.read" => {
            Consent::AtInstall
        }
        // 1.1 (PROVISIONAL): when and how long the reader reads; no text, no title alone.
        "reading.sessions" => Consent::AtInstall,
        "book.text" | "annotations.write" => Consent::Highlighted,
        "files.import" | "files.export" => Consent::EachUse,
        "background" => Consent::StronglyWarned,
        _ => {
            let host = p.strip_prefix("network:")?;
            if !valid_host(host) {
                return None;
            }
            if host.starts_with("*.") {
                Consent::StronglyWarned
            } else {
                Consent::AtInstall
            }
        }
    })
}

/// A host for `network:<host>`: a DNS name (optionally `*.` for its subdomains),
/// with an optional port. No scheme, path, user or bare `*`.
pub fn valid_host(host: &str) -> bool {
    let (name, port) = match host.rsplit_once(':') {
        Some((n, p)) => (n, Some(p)),
        None => (host, None),
    };
    if let Some(p) = port {
        if p.parse::<u16>().map_or(true, |p| p == 0) {
            return false;
        }
    }
    let name = name.strip_prefix("*.").unwrap_or(name);
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

/// Whether `network:` permissions allow a URL (https or http; host and port must match).
pub fn network_allows(granted: &[String], url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if scheme != "https" && scheme != "http" {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let authority = authority.to_ascii_lowercase();
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.contains(']') => (h.to_string(), Some(p.to_string())),
        _ => (authority.clone(), None),
    };
    granted
        .iter()
        .filter_map(|p| p.strip_prefix("network:"))
        .any(|allowed| {
            let (a_host, a_port) = match allowed.rsplit_once(':') {
                Some((h, p)) => (h, Some(p)),
                None => (allowed, None),
            };
            if a_port.map(String::from) != port {
                return false;
            }
            match a_host.strip_prefix("*.") {
                Some(base) => host.ends_with(&format!(".{base}")),
                None => host == a_host,
            }
        })
}

/// Why an extension cannot run on this build, if it cannot (P5).
pub fn incompatibility(m: &Manifest) -> Option<String> {
    let Ok(req) = semver::VersionReq::parse(&m.engines.linen) else {
        return Some(format!(
            "its required Linen API “{}” is not a version range",
            m.engines.linen
        ));
    };
    let current = semver::Version::parse(API_VERSION).unwrap();
    // Current or previous major: the newest supported version of each major.
    let ok = SUPPORTED_MAJORS.iter().any(|&major| {
        let v = if major == current.major {
            current.clone()
        } else {
            semver::Version::new(major, u64::MAX >> 1, 0)
        };
        req.matches(&v)
    });
    (!ok).then(|| {
        format!(
            "it was built for Linen API {}; this version of Linen supports API {}",
            m.engines.linen,
            SUPPORTED_MAJORS
                .iter()
                .map(|m| format!("{m}.x"))
                .collect::<Vec<_>>()
                .join(" and ")
        )
    })
}

fn plain_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
        && p.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
}

fn text(field: &str, value: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{field} is empty"));
    } else if value.chars().count() > MAX_TEXT {
        errors.push(format!("{field} is longer than {MAX_TEXT} characters"));
    }
}

fn local_id(field: &str, id: &str, errors: &mut Vec<String>) {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
    {
        errors.push(format!("{field} “{id}” is not a valid id"));
    }
}

/// Theme tokens a pack may set (P9): colours only, from the theme's token names.
const THEME_TOKENS: &[&str] = &[
    "ground",
    "ink",
    "inkSecondary",
    "accent",
    "hairline",
    "chromeHairline",
    "panel",
    "controlTrack",
    "raised",
    "segmentRing",
    "trackInk",
    "popover",
    "popoverBorder",
    "tooltip",
    "tooltipInk",
];

fn colour(v: &str) -> bool {
    let hex = v
        .strip_prefix('#')
        .is_some_and(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()));
    hex
}

/// Parse and check a manifest. Every problem is listed.
pub fn parse(json: &str) -> Result<Manifest, Vec<String>> {
    let m: Manifest =
        serde_json::from_str(json).map_err(|e| vec![format!("manifest.json: {e}")])?;
    let mut errors = vec![];
    if !super::valid_id(&m.id) {
        errors.push(format!(
            "id “{}” must be lowercase letters, digits, dots and dashes",
            m.id
        ));
    }
    if semver::Version::parse(&m.version).is_err() {
        errors.push(format!("version “{}” is not a semantic version", m.version));
    }
    if semver::VersionReq::parse(&m.engines.linen).is_err() {
        errors.push(format!(
            "engines.linen “{}” is not a version range",
            m.engines.linen
        ));
    }
    text("name", &m.name, &mut errors);
    if m.description.chars().count() > MAX_TEXT {
        errors.push("description is too long".into());
    }
    let mut seen = HashSet::new();
    for p in &m.permissions {
        if permission_consent(p).is_none() {
            errors.push(format!("unknown permission “{p}”"));
        }
        if !seen.insert(p) {
            errors.push(format!("permission “{p}” is listed twice"));
        }
    }
    let c = &m.contributes;
    let count = c.commands.len()
        + c.selection_actions.len()
        + c.navigator_tabs.len()
        + c.themes.len()
        + c.exporters.len();
    if count > MAX_CONTRIBUTIONS {
        errors.push(format!("more than {MAX_CONTRIBUTIONS} contributions"));
    }
    let commands: HashSet<&str> = c.commands.iter().map(|x| x.id.as_str()).collect();
    for x in &c.commands {
        local_id("command id", &x.id, &mut errors);
        text("command title", &x.title, &mut errors);
    }
    for x in &c.selection_actions {
        if !commands.contains(x.command.as_str()) {
            errors.push(format!(
                "selection action uses undeclared command “{}”",
                x.command
            ));
        }
        if let Some(w) = &x.when {
            if let Err(e) = super::when::parse(w) {
                errors.push(format!("when “{w}”: {e}"));
            }
        }
    }
    for x in &c.navigator_tabs {
        local_id("navigator tab id", &x.id, &mut errors);
        text("navigator tab title", &x.title, &mut errors);
        if !plain_path(&x.page) || !x.page.ends_with(".html") {
            errors.push(format!(
                "navigator tab page “{}” must be an .html file in the package",
                x.page
            ));
        }
    }
    for x in &c.exporters {
        local_id("exporter id", &x.id, &mut errors);
        text("exporter title", &x.title, &mut errors);
        if !commands.contains(x.command.as_str()) {
            errors.push(format!("exporter uses undeclared command “{}”", x.command));
        }
    }
    for t in &c.themes {
        local_id("theme id", &t.id, &mut errors);
        text("theme title", &t.title, &mut errors);
        if t.scheme != "light" && t.scheme != "dark" {
            errors.push(format!("theme “{}” scheme must be light or dark", t.id));
        }
        for (k, v) in &t.tokens {
            if !THEME_TOKENS.contains(&k.as_str()) {
                errors.push(format!("theme “{}”: unknown token “{k}”", t.id));
            } else if !colour(v) {
                errors.push(format!("theme “{}”: {k} must be a #rrggbb colour", t.id));
            }
        }
        for required in ["ground", "ink", "inkSecondary", "accent", "hairline"] {
            if !t.tokens.contains_key(required) {
                errors.push(format!("theme “{}” needs {required}", t.id));
            }
        }
    }
    let has_code =
        !c.commands.is_empty() || !c.navigator_tabs.is_empty() || !m.activation.is_empty();
    match &m.main {
        Some(main) if !plain_path(main) || !main.ends_with(".js") => {
            errors.push(format!("main “{main}” must be a .js file in the package"))
        }
        None if has_code => errors.push("commands and tabs need a main script".into()),
        _ => {}
    }
    for a in &m.activation {
        let ok = match a.split_once(':') {
            Some(("onCommand", id)) => commands.contains(id),
            Some(("onNavigatorTab", id)) => c.navigator_tabs.iter().any(|t| t.id == id),
            Some(("onExport", id)) => c.exporters.iter().any(|t| t.id == id),
            None => a == "onAnnotations" || a == "onReadingSessions",
            _ => false,
        };
        if !ok {
            errors.push(format!(
                "activation event “{a}” does not match a contribution"
            ));
        }
    }
    if a_theme_pack_with_permissions(&m) {
        errors.push("a theme pack needs no permissions and has no code".into());
    }
    if errors.is_empty() {
        Ok(m)
    } else {
        Err(errors)
    }
}

fn a_theme_pack_with_permissions(m: &Manifest) -> bool {
    let c = &m.contributes;
    let only_themes = !c.themes.is_empty()
        && c.commands.is_empty()
        && c.navigator_tabs.is_empty()
        && c.exporters.is_empty()
        && c.selection_actions.is_empty();
    only_themes && (!m.permissions.is_empty() || m.main.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DICTIONARY: &str = r#"{
        "id": "org.example.dictionary", "version": "1.2.0", "name": "Dictionary",
        "description": "Look up the selected word without leaving the page.",
        "engines": { "linen": "^1.0" }, "main": "main.js",
        "activation": ["onCommand:define"],
        "contributes": {
            "commands": [{ "id": "define", "title": "Define" }],
            "selectionActions": [{ "command": "define", "when": "selection.words <= 3" }],
            "navigatorTabs": [{ "id": "definitions", "title": "Definitions", "page": "tab.html" }] },
        "permissions": ["book.selection", "network:api.dictionaryapi.dev"] }"#;

    #[test]
    fn the_proposals_example_is_valid() {
        let m = parse(DICTIONARY).unwrap();
        assert_eq!(
            m.contributes.selection_actions[0].when.as_deref(),
            Some("selection.words <= 3")
        );
        assert_eq!(incompatibility(&m), None);
    }

    #[test]
    fn every_problem_is_listed() {
        let bad = DICTIONARY
            .replace("org.example.dictionary", "Org/Example")
            .replace("\"1.2.0\"", "\"one\"")
            .replace("book.selection", "book.everything")
            .replace("\"command\": \"define\"", "\"command\": \"nope\"");
        let errors = parse(&bad).unwrap_err();
        assert!(
            errors.iter().any(|e| e.contains("id “Org/Example”")),
            "{errors:?}"
        );
        assert!(errors.iter().any(|e| e.contains("version “one”")));
        assert!(errors
            .iter()
            .any(|e| e.contains("unknown permission “book.everything”")));
        assert!(errors
            .iter()
            .any(|e| e.contains("undeclared command “nope”")));
        assert!(parse(
            r#"{"id":"a","version":"1.0.0","name":"x","engines":{"linen":"^1"},"shell":true}"#
        )
        .unwrap_err()[0]
            .contains("unknown field"));
    }

    #[test]
    fn permissions_and_their_consent() {
        assert_eq!(permission_consent("book.text"), Some(Consent::Highlighted));
        assert_eq!(
            permission_consent("reading.sessions"),
            Some(Consent::AtInstall)
        );
        assert_eq!(
            permission_consent("network:*.example.org"),
            Some(Consent::StronglyWarned)
        );
        assert_eq!(
            permission_consent("network:api.example.org:8443"),
            Some(Consent::AtInstall)
        );
        for bad in [
            "network:*",
            "network:https://a.org",
            "network:a.org/x",
            "network:user@a.org",
            "network:A.org",
            "shell",
        ] {
            assert_eq!(permission_consent(bad), None, "{bad}");
        }
    }

    #[test]
    fn network_permissions_match_host_and_port_exactly() {
        let g = vec![
            "network:api.dictionaryapi.dev".to_string(),
            "network:*.example.org".to_string(),
        ];
        assert!(network_allows(
            &g,
            "https://api.dictionaryapi.dev/api/v2/entries/en/whale"
        ));
        assert!(network_allows(&g, "https://a.b.example.org/x"));
        assert!(
            !network_allows(&g, "https://example.org/x"),
            "the wildcard is for subdomains"
        );
        assert!(!network_allows(
            &g,
            "https://api.dictionaryapi.dev.evil.org/"
        ));
        assert!(
            !network_allows(&g, "https://evil.org@api.dictionaryapi.dev/"),
            "no user info"
        );
        assert!(
            !network_allows(&g, "https://api.dictionaryapi.dev:8443/"),
            "the port is part of the host"
        );
        assert!(!network_allows(&g, "file:///etc/passwd"));
        assert!(!network_allows(&g, "ws://api.dictionaryapi.dev/"));
    }

    #[test]
    fn unsupported_majors_are_refused_with_a_reason() {
        let m = parse(&DICTIONARY.replace("^1.0", "^2.0")).unwrap();
        let why = incompatibility(&m).unwrap();
        assert!(why.contains("^2.0") && why.contains("1.x"), "{why}");
        assert!(
            incompatibility(&parse(&DICTIONARY.replace("^1.0", ">=1.0, <3")).unwrap()).is_none()
        );
    }

    #[test]
    fn theme_packs_are_data_only() {
        let theme = r##"{ "id": "org.example.night-owl", "version": "1.1.0", "name": "Night Owl",
            "engines": { "linen": "^1.0" },
            "contributes": { "themes": [{ "id": "night-owl", "title": "Night Owl", "scheme": "dark",
              "tokens": { "ground": "#0B1622", "ink": "#C9D4E0", "inkSecondary": "#8A9BB0",
                          "accent": "#7FB0E0", "hairline": "#1E2C3C" } }] } }"##;
        assert!(parse(theme).is_ok());
        assert!(parse(&theme.replace(
            "\"engines\"",
            "\"permissions\": [\"book.text\"], \"engines\""
        ))
        .is_err());
        assert!(parse(&theme.replace("#0B1622", "url(x)")).is_err());
        assert!(parse(&theme.replace("\"hairline\"", "\"background-image\"")).is_err());
    }
}
