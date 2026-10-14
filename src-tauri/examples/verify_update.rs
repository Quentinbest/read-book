//! Release acceptance (docs/release-acceptance.md): checks a release's updater archive
//! the way the installed app's updater does (D6; tauri-plugin-updater's
//! `verify_signature`): the minisign signature against the public key in
//! tauri.conf.json, then the version that the signed trusted comment names.
//!
//!   cargo run --example verify_update -- <Linen_<arch>.app.tar.gz> <its .sig> <version>

use std::process::ExitCode;

use base64::Engine;
use minisign_verify::{PublicKey, Signature};

fn unbase64(s: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|e| format!("not base64: {e}"))?;
    String::from_utf8(bytes).map_err(|_| "not UTF-8 once decoded".into())
}

fn verify(archive: &str, sig: &str, version: &str) -> Result<(), String> {
    let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
        .map_err(|e| format!("tauri.conf.json: {e}"))?;
    let key = conf["plugins"]["updater"]["pubkey"]
        .as_str()
        .ok_or("tauri.conf.json has no plugins.updater.pubkey")?;
    let key = PublicKey::decode(&unbase64(key)?).map_err(|e| format!("the key: {e}"))?;
    let sig = std::fs::read_to_string(sig).map_err(|e| format!("{sig}: {e}"))?;
    let sig = Signature::decode(&unbase64(&sig)?).map_err(|e| format!("the signature: {e}"))?;
    let data = std::fs::read(archive).map_err(|e| format!("{archive}: {e}"))?;
    key.verify(&data, &sig, true)
        .map_err(|e| format!("the signature does not verify: {e}"))?;
    // The trusted comment is covered by the signature just checked, so it can be read now.
    let signed = sig
        .trusted_comment()
        .split('\t')
        .find_map(|f| f.strip_prefix("version:"))
        .ok_or("the signature names no version")?;
    if signed.trim_start_matches('v') != version.trim_start_matches('v') {
        return Err(format!("signed for {signed}, not {version}"));
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [archive, sig, version] = args.as_slice() else {
        eprintln!("usage: verify_update <archive> <archive.sig> <version>");
        return ExitCode::from(2);
    };
    match verify(archive, sig, version) {
        Ok(()) => {
            println!("{archive}: signed with the update key for {version}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{archive}: {e}");
            ExitCode::FAILURE
        }
    }
}
