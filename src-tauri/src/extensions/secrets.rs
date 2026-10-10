//! Reading Lens LK7: extensions' keys. A key is typed into Linen's own native dialog,
//! kept in the macOS Keychain, and added by the core to a request only when the URL's
//! host is the one the key was saved for. No command returns a key; an extension can
//! only ask whether one exists. The store keeps names and hosts, never values.
//!
//! Spike J (docs/spikes/j-keychain.md): with ad-hoc signing, Keychain protects keys at
//! the level of the user account; extensions still can't reach it.

/// Keychain service for every extension key; the account is `<extension>/<name>`.
pub const SERVICE: &str = "app.linen.extension-key";

/// Where keys are kept: the Keychain in the app, memory in tests.
pub trait Vault: Send + Sync {
    fn get(&self, ext: &str, name: &str) -> Result<Option<String>, String>;
    fn set(&self, ext: &str, name: &str, value: &str) -> Result<(), String>;
    fn delete(&self, ext: &str, name: &str) -> Result<(), String>;
}

fn account(ext: &str, name: &str) -> String {
    format!("{ext}/{name}")
}

pub struct Keychain;

impl Vault for Keychain {
    fn get(&self, ext: &str, name: &str) -> Result<Option<String>, String> {
        match security_framework::passwords::get_generic_password(SERVICE, &account(ext, name)) {
            Ok(v) => Ok(Some(String::from_utf8_lossy(&v).into_owned())),
            // errSecItemNotFound
            Err(e) if e.code() == -25300 => Ok(None),
            Err(e) => Err(format!("the Keychain refused ({})", e.code())),
        }
    }
    fn set(&self, ext: &str, name: &str, value: &str) -> Result<(), String> {
        security_framework::passwords::set_generic_password(
            SERVICE,
            &account(ext, name),
            value.as_bytes(),
        )
        .map_err(|e| format!("the Keychain refused ({})", e.code()))
    }
    fn delete(&self, ext: &str, name: &str) -> Result<(), String> {
        match security_framework::passwords::delete_generic_password(SERVICE, &account(ext, name)) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == -25300 => Ok(()),
            Err(e) => Err(format!("the Keychain refused ({})", e.code())),
        }
    }
}

/// A key's name: letters, digits, dashes and underscores.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// How a key goes into a request (LK7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scheme {
    Bearer,
    XApiKey,
    XGoogApiKey,
}

impl Scheme {
    pub fn header(self, value: &str) -> (String, String) {
        match self {
            Scheme::Bearer => ("Authorization".into(), format!("Bearer {value}")),
            Scheme::XApiKey => ("x-api-key".into(), value.to_string()),
            Scheme::XGoogApiKey => ("x-goog-api-key".into(), value.to_string()),
        }
    }
}

/// The host and port of an http(s) URL, lowercase, as `host` or `host:port`.
pub fn url_host(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "https" && scheme != "http" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    Some(authority.to_ascii_lowercase())
}

/// LK7: a key goes only to the host it was saved for, exactly; over https, except to
/// this Mac (`localhost`, `127.0.0.1`).
pub fn may_send(secret_host: &str, url: &str) -> bool {
    let Some(host) = url_host(url) else {
        return false;
    };
    if host != secret_host.to_ascii_lowercase() {
        return false;
    }
    let local = host
        .split(':')
        .next()
        .is_some_and(|h| h == "localhost" || h == "127.0.0.1");
    url.starts_with("https://") || local
}

/// Headers an extension may not set: the key schemes and what identifies or routes a request.
const FORBIDDEN: &[&str] = &[
    "authorization",
    "cookie",
    "host",
    "x-api-key",
    "x-goog-api-key",
    "content-length",
    "connection",
    "transfer-encoding",
    "te",
    "upgrade",
];

/// LK7: the extension's own headers, checked: names are plain tokens, values one line,
/// at most 20 headers and 8 KB in all.
pub fn checked_headers(headers: &[(String, String)]) -> Result<Vec<(String, String)>, String> {
    if headers.len() > 20 {
        return Err("more than 20 headers".into());
    }
    let mut total = 0;
    let mut out = vec![];
    for (k, v) in headers {
        let lower = k.to_ascii_lowercase();
        if k.is_empty() || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(format!("“{k}” is not a header name"));
        }
        if FORBIDDEN.contains(&lower.as_str())
            || lower.starts_with("proxy-")
            || lower.starts_with("sec-")
        {
            return Err(format!("extensions can't set {k}"));
        }
        if v.contains(['\r', '\n', '\0']) {
            return Err(format!("{k} has more than one line"));
        }
        total += k.len() + v.len();
        out.push((k.clone(), v.clone()));
    }
    if total > 8 * 1024 {
        return Err("the headers are larger than 8 KB".into());
    }
    Ok(out)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct Memory(pub Mutex<HashMap<String, String>>);
    impl Vault for Memory {
        fn get(&self, ext: &str, name: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(&account(ext, name)).cloned())
        }
        fn set(&self, ext: &str, name: &str, value: &str) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert(account(ext, name), value.into());
            Ok(())
        }
        fn delete(&self, ext: &str, name: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(&account(ext, name));
            Ok(())
        }
    }

    #[test]
    fn a_key_goes_only_to_its_own_host_over_https() {
        assert!(may_send(
            "api.deepseek.com",
            "https://api.deepseek.com/chat/completions"
        ));
        assert!(may_send("API.DeepSeek.com", "https://api.deepseek.com/x"));
        assert!(
            !may_send("api.deepseek.com", "http://api.deepseek.com/x"),
            "not over http"
        );
        assert!(!may_send(
            "api.deepseek.com",
            "https://api.deepseek.com.evil.org/x"
        ));
        assert!(!may_send(
            "api.deepseek.com",
            "https://evil.org/?u=https://api.deepseek.com"
        ));
        assert!(
            !may_send("api.deepseek.com", "https://x@api.deepseek.com/"),
            "no user info"
        );
        assert!(
            !may_send("api.deepseek.com", "https://api.deepseek.com:8443/"),
            "the port counts"
        );
        assert!(
            may_send("localhost:11434", "http://localhost:11434/api/chat"),
            "this Mac over http"
        );
        assert!(!may_send("api.deepseek.com", "file:///etc/passwd"));
    }

    #[test]
    fn extensions_cannot_set_keys_cookies_or_routing() {
        for h in [
            "Authorization",
            "cookie",
            "Host",
            "Proxy-Authorization",
            "x-api-key",
            "X-Goog-Api-Key",
            "Content-Length",
            "Sec-Fetch-Mode",
        ] {
            assert!(checked_headers(&[(h.into(), "x".into())]).is_err(), "{h}");
        }
        assert!(checked_headers(&[("Content-Type".into(), "application/json".into())]).is_ok());
        assert!(checked_headers(&[("X-Trace".into(), "a\r\nb".into())]).is_err());
        assert!(checked_headers(&[("Bad Name".into(), "x".into())]).is_err());
        assert!(checked_headers(&vec![("A".into(), "b".into()); 21]).is_err());
    }

    #[test]
    fn schemes_and_names() {
        assert_eq!(
            Scheme::Bearer.header("k"),
            ("Authorization".into(), "Bearer k".into())
        );
        assert_eq!(Scheme::XApiKey.header("k").0, "x-api-key");
        assert_eq!(Scheme::XGoogApiKey.header("k").0, "x-goog-api-key");
        assert!(valid_name("deepseek") && valid_name("api_key-2"));
        assert!(!valid_name("a/b") && !valid_name("") && !valid_name(&"a".repeat(65)));
    }

    #[test]
    fn the_memory_vault_keeps_keys_apart_per_extension() {
        let v = Memory::default();
        v.set("a", "k", "1").unwrap();
        v.set("b", "k", "2").unwrap();
        assert_eq!(v.get("a", "k").unwrap().as_deref(), Some("1"));
        v.delete("a", "k").unwrap();
        assert_eq!(v.get("a", "k").unwrap(), None);
        assert_eq!(v.get("b", "k").unwrap().as_deref(), Some("2"));
    }
}
