//! Native input signals for page turning (plan I1–I5, I11; Spike B fallback).
//!
//! WKWebView wheel events carry neither the device kind nor gesture phases, so an
//! accelerated mouse wheel looks like a trackpad (Spike B run 1). A local NSEvent
//! monitor forwards what AppKit knows to the frontend:
//! - `native-scroll`: `precise` (trackpad / Magic Mouse vs. a line-based wheel),
//!   `phase` and `momentum` (NSEventPhase bits), deltas, and the position.
//! - `native-mouse-down`: whether the app was already active, so the click that
//!   activates an inactive window can be ignored (I11).
//!
//! Events pass through unchanged; the frontend decides what they mean.

#[derive(Clone, serde::Serialize)]
pub struct NativeScroll {
    pub precise: bool,
    /// NSEventPhase bits: 1 began, 2 stationary, 4 changed, 8 ended, 16 cancelled, 32 may begin.
    pub phase: u64,
    pub momentum: u64,
    pub dx: f64,
    pub dy: f64,
    /// Position in the window's content, in points, origin top-left (CSS pixels at 100% zoom).
    pub x: f64,
    pub y: f64,
    /// AppKit timestamp in ms (seconds since boot × 1000).
    pub t: f64,
}

#[derive(Clone, serde::Serialize)]
pub struct NativeMouseDown {
    pub app_active: bool,
    pub key_window: bool,
    pub t: f64,
}

#[cfg(target_os = "macos")]
pub fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEvent, NSEventMask, NSEventType};
    use std::ptr::NonNull;
    use tauri::Emitter;

    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("native input: not on the main thread; monitor not installed");
        return;
    };
    let app = app.clone();
    let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit passes a valid event for the duration of the handler.
        let e = unsafe { event.as_ref() };
        let t = e.timestamp() * 1000.0;
        let ty = e.r#type();
        if ty == NSEventType::ScrollWheel {
            let p = e.locationInWindow();
            let height = e
                .window(mtm)
                .and_then(|w| w.contentView())
                .map(|v| v.frame().size.height)
                .unwrap_or(0.0);
            let _ = app.emit(
                "native-scroll",
                NativeScroll {
                    precise: e.hasPreciseScrollingDeltas(),
                    phase: e.phase().0 as u64,
                    momentum: e.momentumPhase().0 as u64,
                    dx: e.scrollingDeltaX(),
                    dy: e.scrollingDeltaY(),
                    x: p.x,
                    y: height - p.y,
                    t,
                },
            );
        } else if ty == NSEventType::LeftMouseDown {
            let _ = app.emit(
                "native-mouse-down",
                NativeMouseDown {
                    app_active: NSApplication::sharedApplication(mtm).isActive(),
                    key_window: e.window(mtm).map(|w| w.isKeyWindow()).unwrap_or(false),
                    t,
                },
            );
        }
        event.as_ptr()
    });
    // SAFETY: the handler returns the event it was given, which is a valid pointer.
    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::ScrollWheel | NSEventMask::LeftMouseDown,
            &handler,
        )
    };
    // The monitor lives as long as the app.
    std::mem::forget(monitor);
    std::mem::forget(handler);
}

#[cfg(not(target_os = "macos"))]
pub fn install<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) {}
