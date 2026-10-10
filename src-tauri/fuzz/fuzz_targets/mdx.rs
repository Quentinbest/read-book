#![no_main]
//! Reading Lens §6.1, DX13: any bytes as an MDX (or, with a leading 1, an MDD) file.
//! Every outcome is Ok or an error; no panic, no hang, no record past its cap.

use libfuzzer_sys::fuzz_target;
#[path = "../../src/dictionaries/mdx.rs"]
#[allow(dead_code)]
mod mdx;
use mdx::{with_ends, Mdx, MAX_ENTRY, MAX_RESOURCE};

fuzz_target!(|data: &[u8]| {
    let Some((&kind, bytes)) = data.split_first() else { return };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(if kind & 1 == 1 { "f.mdd" } else { "f.mdx" });
    std::fs::write(&path, bytes).unwrap();
    if let Ok(m) = Mdx::open(&path) {
        let mut keys = vec![];
        if m.keys(|k| keys.push(k)).is_ok() {
            for (k, end) in with_ends(&m, keys).into_iter().take(64) {
                if let Ok(r) = m.record(k.offset, end) {
                    let cap = if m.is_mdd { MAX_RESOURCE } else { MAX_ENTRY };
                    assert!(r.len() as u64 <= cap);
                }
            }
        }
    }
});
