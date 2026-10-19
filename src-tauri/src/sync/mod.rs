//! Sync across devices (`docs/sync-plan.md`, SY1–SY34), in the byte-level form
//! `docs/sync-protocol.md` gives it (SP1–SP37).
//!
//! Stage 1 builds the core as pure code, with no storage, network or UI: these
//! modules read and check what other devices wrote, and decide what it means. Nothing
//! here opens a file in iCloud Drive or Google Drive, and nothing is wired into the
//! app yet (Stage 2 adds the providers, Stage 3 turns sync on).

pub mod a9;
pub mod apply;
pub mod clock;
pub mod guard;
pub mod identity;
pub mod ids;
pub mod journal;
pub mod json;
pub mod local;
pub mod merge;
pub mod record;
pub mod retention;
