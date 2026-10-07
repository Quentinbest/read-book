# Scroll bars: the overlay behaviour in the owner's GIF (2026-10-07)

- **Asked for:** the owner, 2026-10-07, with a screen recording of a scroll bar: identify the behaviour, recommend how Linen should reproduce it, then “Go ahead”.
- **Result:** no product change. Linen already draws WebKit's native scroll bars, and they behave as in the recording whenever macOS uses overlay scroll bars, Scroll mode included. On the Mac used here they are always visible because a USB mouse is attached and **Show scroll bars** is **Automatically**: macOS then asks every app for always-visible (“legacy”) bars, and Linen follows it. Item 55 in `docs/pending-approvals.md` asked whether Linen should depart from that; **approved 2026-10-07: option A**, follow macOS.

## The recording

A 162 × 760 crop of a window's right edge, 54 frames at 5 fps (10.8 s). Measured per frame from the pixels:

| Frames | Pointer | Thumb |
| --- | --- | --- |
| 0–3 (0–0.6 s) | moves towards the edge | not drawn |
| 4–5 (0.8–1.0 s) | arrives (frame 4 shows the left-right resize cursor) | appears, dim, 6 px wide |
| 6–18 (1.2–3.6 s) | rests on it | brighter; same width, no track; its top moves (the content scrolls) |
| 19–22 (3.8–4.4 s) | leaves | dims at once, fades, gone |
| 28–40 (5.6–8.0 s) | returns, then leaves | bright at once; after leaving, dim for about 1 s, then gone |
| 45–53 (9.0–10.6 s) | away from the edge | appears dim without the pointer while its top slides (a scroll), then fades |

This is an **overlay scroll bar** (AppKit's `NSScroller.Style.overlay`; also called auto-hiding or transient scroll bars): hidden at rest, shown while scrolling, brighter under the pointer, faded after a short delay.

## How it was measured

`scrollbar-survey`, an opt-in measurement in the in-app suite (no verdict; captures `scrollbar-*`). It reports what this process's AppKit makes of the setting (`spike_scroller_style`) and the gutter of an `overflow: scroll` probe (0 with overlay bars, the bar's width with legacy ones), before any posted input. Then, with real posted input (`spike_scroll_wheel { hid }`, `spike_mouse`), it captures Scroll mode at rest, just after a trackpad scroll, at rest again, with the pointer on the knob, and the Contents list after a scroll.

```sh
LINEN_SPIKES=1 pnpm tauri build --no-bundle --features spikes --config src-tauri/tauri.spikes.conf.json
scripts/e2e.sh r '^scrollbar-survey$'                                      # this Mac's own setting
scripts/e2e.sh r '^scrollbar-survey$' -AppleShowScrollBars WhenScrolling   # overlay bars for this run only
```

Arguments after the check's regex go to the app; AppKit reads `-AppleShowScrollBars …` as a default for that process only, so the system setting is not touched. Captures land in `docs/visual/app/`; the ones kept are in `docs/visual/survey/scrollbar-*.png`.

## Results

| What | Result | Evidence |
| --- | --- | --- |
| The setting as Linen sees it | `preferredScrollerStyle` 0 (legacy) with **Show scroll bars: Automatically** and a USB mouse attached. A minimal AppKit app on the same Mac reports 1 at launch and 0 two seconds later, once AppKit has looked at the pointing devices; `osascript` reports 1 because it asks before that. | `scrollbar-survey`; `ioreg` lists “USB Gaming Mouse” |
| Legacy bars (this Mac's setting) | Always visible, 15 px gutter, in the library, Contents and Scroll mode alike. Brighter under the pointer. This is how every Mac app looks with these settings. | `scrollbar-legacy-rest.png` |
| Overlay bars, Scroll mode at rest | **Pass.** Nothing drawn; gutter 0 px. | `scrollbar-overlay-1-rest.png` |
| Overlay bars, Scroll mode after a scroll | **Pass.** The knob appears, although the engine scrolls the host itself from AppKit's scroll stream (`scrollPixels`) rather than WebKit scrolling it. | `scrollbar-overlay-2-after-wheel.png` |
| Overlay bars, Scroll mode at rest again (2.5 s) | **Pass.** Gone. | `scrollbar-overlay-3-rest-again.png` |
| Overlay bars, Contents after a scroll | **Pass** (the control: WebKit scrolls this list itself). | `scrollbar-overlay-contents-after-wheel.png` |
| Overlay bars, the pointer held on the knob after a scroll (the recording's frames 6–18) | **Not verified.** With posted moves the knob faded as usual, in Scroll mode and on the Contents list alike, so the harness cannot tell whether WebKit keeps a hovered knob up. Needs a hand on a real trackpad. | captures 5 and 8 of the run |
| Overlay bars, the pointer on the knob with no scroll | Nothing drawn (posted moves). | capture 4 of the run |

## What would change it

- **To see the recording's behaviour on this Mac:** System Settings › Appearance › Show scroll bars › **When scrolling**. Linen follows it at once, as above.
- **To force overlay bars in Linen whatever the setting:** that overrides a choice macOS makes for mouse users and the people who set **Always**. It would need a custom indicator drawn by the reader (WebKit's bars follow the system), with its own hover, drag and timing, hidden from assistive technology since I9's keys and the scrubber already scroll. Not recommended; see item 55.
