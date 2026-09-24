# Spike B: input — needs a person at the machine

**Status: not run.** Pass criteria (plan §5): over 50 scripted and 50 manual gestures, exactly one page turn per gesture on both trackpad axes (I2, I3) and no extra turns from momentum (I5). Also record whether the window-activating click can be told apart (I11).

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
