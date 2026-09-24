# Spike B: input — needs a person at the machine

**Status (2026-09-24): wheel and activating click pass with the native bridge. Trackpad criteria (I2, I3, I5) not tested: no trackpad available (owner).** Pass criteria as below. Pass criteria (plan §5): over 50 scripted and 50 manual gestures, exactly one page turn per gesture on both trackpad axes (I2, I3) and no extra turns from momentum (I5). Also record whether the window-activating click can be told apart (I11).

## Why it is not automated

WKWebView's wheel events carry no gesture or momentum phase. Scripted gestures would have to be posted as `CGEvent` scroll events with phase fields, which needs Accessibility permission for the posting process; that is a permission grant only the owner can give. The 50 manual gestures need a person in any case.

## What is needed from the owner

About 15 minutes at the reference machine with its trackpad. The page counts page turns per gesture and records wheel deltas, so the momentum tail can be analysed. If the WebView alone cannot separate momentum, the fallback is the native bridge (`NSEvent` `phase` and `momentumPhase` through a local event monitor in the Rust core), measured against the same bar.

## How to run it (about 15 minutes)

```sh
scripts/run-spikes.sh b
```

A window opens with Moby-Dick and an instruction panel on the right:

1. 25 vertical swipes;
2. 25 horizontal swipes;
3. 10 quick swipes started during the previous swipe's coast;
4. 20 mouse-wheel notches (optional);
5. one window-activating click.

The panel shows the page turns detected. The results, including every raw wheel event for tuning the provisional thresholds in `src/lib/input/wheel.ts`, go to `raw/b-input.json`.

## Run 1 — 2026-09-24, mouse scroll wheel

The owner ran the harness with a **mouse scroll wheel**, not a trackpad (confirmed after the run). Raw events are in `raw/b-input.json`.

| Harness step | Detector result | What the trace shows |
|---|---|---|
| “Vertical swipes” (25 asked) | 150 turns: fail | 134 separate wheel rolls (bursts more than 150 ms apart) |
| “Horizontal swipes” (25 asked) | 25 turns | 25 rolls; all deltas vertical (a wheel has no horizontal axis) |
| “Quick swipes” (10 asked) | 19 turns: fail | 12 rolls |
| Activating click (I11) | fail | `document.hasFocus()` was already true at `pointerdown` |

**Findings**

1. **With macOS acceleration, a scroll wheel does not look notched to the WebView.** Each roll arrives as a burst of 3–7 accelerating pixel deltas (4 → 39 → 160 → 225 → 256 px, up to 1,100 px) about 10–40 ms apart, with no momentum tail. `wheelDeltaY` is not a multiple of 120, so `isNotchedWheel` failed. The detector then treated every burst as a trackpad gesture and every jump in delta as a new swipe.
2. **The I1 rule itself works on this device.** Replaying the trace with “one roll = one page, a roll ends after 150 ms of quiet” gives 25 turns for 25 rolls (horizontal step) and 12 for 12 (quick step).
3. **The WebView cannot classify the device reliably.** It gets neither `hasPreciseScrollingDeltas` nor gesture phases. This is the case the plan's fallback covers: a native bridge using `NSEvent` `hasPreciseScrollingDeltas`, `phase` and `momentumPhase` through a local event monitor in the Rust core.
4. **I11 needs another signal.** WebKit reports the document as focused before the activating click's `pointerdown` arrives. Candidates: the time between the window's `focus` event and the click, or the native `NSApplication` activation.

**Next:** build the native scroll bridge and re-run (done: run 2).

## Run 2 — 2026-09-24, mouse scroll wheel with the native bridge

Page turns now come from an `NSEvent` local monitor (`src-tauri/src/native_input.rs`) through `NativeTurns` (`src/lib/input/native.ts`). The WebView-only detector ran alongside for comparison. Raw events are in `raw/b-input.json`.

| Criterion | Result | Verdict |
|---|---|---|
| Device classification | AppKit reported `hasPreciseScrollingDeltas = false` for all 253 wheel events: always recognised as a wheel | pass |
| I1: one roll = one page in its direction | **81/81 clean rolls** gave exactly 1 + ⌊duration / 250 ms⌋ turns, all in the roll's direction. One two-second back-and-forth rocking roll was not graded (it turned pages both ways, as I1's cooldown allows). | **pass** |
| I11: activating click | AppKit reported the app **inactive on all 4 clicks that brought the window back** and active on the 1 ordinary click. `document.hasFocus()` was true on all 5, so the WebView alone cannot tell. | **pass** (native signal) |
| I2, I3, I5: trackpad | Not tested: no trackpad available | **open** |
| 50 scripted gestures | Not run (needs Accessibility permission) | open |

Notes:

- The harness's instructed counts (“25 rolls down”) did not match what was rolled (the first step contained 26 rolls down, then 26 up). The harness now grades each roll against the rule instead of against the instruction.
- On this wheel each roll arrives as 2–5 native events (AppKit `scrollingDeltaY` −0.1 → −3.5) about 20–50 ms apart, with 300–900 ms between rolls. `ROLL_GAP_MS = 150` separates them cleanly.
- Direction: a negative `scrollingDeltaY` (content moving up) is forward, and AppKit's sign already includes the natural-scrolling setting.
- The WebView-only detector gave 20 turns for step 1's 52 rolls. It is not used for page turns.

**Decision:** the native bridge is the page-turning input path on macOS (the plan's Spike B fallback, now proven for the wheel). I2, I3 and I5 stay open until a trackpad is available; `NativeTurns` implements them from AppKit's gesture and momentum phases and is unit-tested with synthetic phase streams.
