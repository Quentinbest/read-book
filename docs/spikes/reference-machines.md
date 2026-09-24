# Reference machines

Plan §5 Phase 0 asks for one mid-range machine per OS plus the oldest supported macOS. Under the macOS-only scope (`docs/decisions.md`), one machine is in use.

| Role | Machine | OS | WebView | Notes |
|---|---|---|---|---|
| macOS reference | Apple M2, 8 GB RAM, APFS SSD, external 2560 × 1440 display | macOS 14.6.1 | WKWebView, WebKit 19618.3.11.11.5 | The owner's development machine; all Phase 0 spikes ran here |
| Oldest supported macOS (13) | **Not available** | macOS 13 | — | Needed before the budgets are signed off for release (§6.4) |
| Trackpad test machine (not a reference machine) | MacBook Air 2015, Intel Core i5-5250U, 8 GB, built-in trackpad | macOS 12.7.4 (below the D7 minimum) | WebKit 17613.3.9.1.16 (Safari 15.6.1) | Used only for Spike B's trackpad criteria (run 3); too old a WebKit for foliate-js |
| Windows, Linux | Deferred | — | — | macOS-only scope |

Spike runs use a release build with the `spikes` feature and `src-tauri/tauri.spikes.conf.json`, which disables WebKit background throttling and keeps the window on top. Without that, an occluded window is throttled by macOS and runs stall (observed on 2026-09-24).
