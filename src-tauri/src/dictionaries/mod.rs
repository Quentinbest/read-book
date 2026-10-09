//! Reading Lens Stage 2b: the reader's own MDX/MDD dictionaries, read in the core and
//! shown only in the lookup peek (DX1–DX15, docs/reading-lens-plan.md). Display only:
//! no Host API returns their text (DX9). Linen ships no dictionary data (DX12).
//!
//! Not to be confused with `crate::dictionary`, 1.1's Look Up in this Mac's dictionaries.

pub mod commands;
pub mod fold;
pub mod generation;
pub mod index;
pub mod lookup;
pub mod mdx;
pub mod sanitise;
pub mod serve;
pub mod state;
