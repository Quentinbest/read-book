//! IPC for the reader's dictionaries (DX1, DX2, DX4, DX11): Settings › Dictionaries
//! and the lookup peek. Entry text never leaves the core through these commands; the
//! peek shows entries in a `linen-dict://` frame (DX9).

use super::generation::{self, ImportError};
use super::mdx::MdxError;
use super::state::DictState;
use crate::commands::AppState;
use crate::store::DictionaryRow;
use std::sync::atomic::Ordering;
use tauri::State;

/// Why an import failed, in a form Settings can word (DX3).
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DictError {
    AlreadyInstalled { title: String },
    Registered,
    Lzo,
    Encoding { name: String },
    Damaged { message: String },
    TooLarge { message: String },
    Changed,
    Cancelled,
    NotMdx,
    Failed { message: String },
}

impl From<ImportError> for DictError {
    fn from(e: ImportError) -> Self {
        match e {
            ImportError::Mdx(MdxError::Registered) => DictError::Registered,
            ImportError::Mdx(MdxError::Lzo) => DictError::Lzo,
            ImportError::Mdx(MdxError::Encoding(name)) => DictError::Encoding { name },
            ImportError::Mdx(MdxError::NotMdx) | ImportError::NotMdx => DictError::NotMdx,
            ImportError::Mdx(MdxError::TooLarge(m)) => DictError::TooLarge { message: m.into() },
            ImportError::Mdx(e) => DictError::Damaged {
                message: e.to_string(),
            },
            ImportError::Changed => DictError::Changed,
            ImportError::Cancelled => DictError::Cancelled,
            e => DictError::Failed {
                message: e.to_string(),
            },
        }
    }
}

fn failed(e: impl std::fmt::Display) -> DictError {
    DictError::Failed {
        message: e.to_string(),
    }
}

fn active(state: &AppState) -> Vec<String> {
    state
        .store
        .lock()
        .unwrap()
        .dictionaries()
        .map(|d| d.into_iter().map(|d| d.generation).collect())
        .unwrap_or_default()
}

/// At launch: half-built and unused generations go (DX2).
pub fn recover(state: &AppState, dicts: &DictState) {
    dicts.collect(&active(state), true);
}

#[tauri::command]
pub fn dict_list(state: State<AppState>) -> Result<Vec<DictionaryRow>, DictError> {
    state.store.lock().unwrap().dictionaries().map_err(failed)
}

/// DX1, DX2: add a dictionary (and the MDD files beside it) as a new generation.
#[tauri::command]
pub async fn dict_import(app: tauri::AppHandle, path: String) -> Result<DictionaryRow, DictError> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let dicts = app.state::<DictState>();
        let _one = dicts.importing.lock().unwrap();
        dicts.cancel.store(false, Ordering::Relaxed);
        let src = generation::sources(std::path::Path::new(&path))?;
        // DX1: the same file again is already installed.
        let hash = crate::import::sha256_file(&src.mdx).map_err(failed)?;
        if let Some(d) = state
            .store
            .lock()
            .unwrap()
            .dictionaries()
            .map_err(failed)?
            .into_iter()
            .find(|d| d.source_hash == hash)
        {
            return Err(DictError::AlreadyInstalled { title: d.title });
        }
        let built = generation::build(&dicts.dir, &src, &dicts.cancel)?;
        // Kept until the store names it: a peek closing meanwhile must not remove it.
        dicts.pin(&built.generation);
        let activated = state.store.lock().unwrap().dictionary_activate(
            &built.name,
            &built.title,
            &built.source_hash,
            &built.generation,
            built.entries as i64,
            built.resources as i64,
        );
        dicts.unpin(&built.generation);
        match activated {
            Ok((row, _replaced)) => {
                // The replaced generation goes when no open entry uses it (DX2).
                dicts.collect(&active(&state), false);
                Ok(row)
            }
            Err(e) => {
                // Not activated: the new generation is not referenced; remove it.
                dicts.collect(&active(&state), false);
                Err(failed(e))
            }
        }
    })
    .await
    .map_err(failed)?
}

#[tauri::command]
pub fn dict_cancel_import(dicts: State<DictState>) {
    dicts.cancel.store(true, Ordering::Relaxed);
}

#[tauri::command]
pub fn dict_remove(
    state: State<AppState>,
    dicts: State<DictState>,
    id: String,
) -> Result<(), DictError> {
    state
        .store
        .lock()
        .unwrap()
        .dictionary_remove(&id)
        .map_err(failed)?;
    dicts.collect(&active(&state), false);
    Ok(())
}

#[tauri::command]
pub fn dict_enable(state: State<AppState>, id: String, enabled: bool) -> Result<(), DictError> {
    state
        .store
        .lock()
        .unwrap()
        .dictionary_set_enabled(&id, enabled)
        .map_err(failed)
}

#[tauri::command]
pub fn dict_reorder(state: State<AppState>, ids: Vec<String>) -> Result<(), DictError> {
    state
        .store
        .lock()
        .unwrap()
        .dictionary_reorder(&ids)
        .map_err(failed)
}

/// A dictionary with an entry for the selection (DX11), and where its entry is shown.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictHit {
    pub id: String,
    pub title: String,
    pub generation: String,
    pub headword: String,
    /// DX4: found for this form by a suffix rule.
    pub for_word: Option<String>,
}

/// DX4, DX11: the enabled dictionaries with an entry for `text`, in the reader's order.
/// Each returned generation is pinned until `dict_release` (DX2).
#[tauri::command]
pub async fn dict_lookup(app: tauri::AppHandle, text: String) -> Result<Vec<DictHit>, DictError> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let dicts = app.state::<DictState>();
        let rows = state.store.lock().unwrap().dictionaries().map_err(failed)?;
        let word: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let word: String = word.chars().take(80).collect();
        let mut out = vec![];
        for d in rows.into_iter().filter(|d| d.enabled) {
            let Some(o) = dicts.open(&d.generation) else {
                continue;
            };
            match o.look_up(&word) {
                Ok(entries) if !entries.is_empty() => {
                    dicts.pin(&d.generation);
                    out.push(DictHit {
                        id: d.id,
                        title: d.title,
                        generation: d.generation,
                        headword: entries[0].headword.clone(),
                        for_word: entries[0].for_word.clone(),
                    });
                }
                Ok(_) => {}
                // EP5: the reason, never the word.
                Err(e) => log::warn!("dictionary {}: {e}", d.title),
            }
        }
        Ok(out)
    })
    .await
    .map_err(failed)?
}

/// The peek closed: its generations may go if they were replaced meanwhile (DX2).
#[tauri::command]
pub fn dict_release(state: State<AppState>, dicts: State<DictState>, generations: Vec<String>) {
    let mut any = false;
    for g in &generations {
        any |= dicts.unpin(g);
    }
    if any {
        dicts.collect(&active(&state), false);
    }
}
