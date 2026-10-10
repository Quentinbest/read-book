//! The dictionaries' runtime state: open generations, the ones an open peek still
//! uses (DX2: a replaced generation is deleted only when no open entry uses it), and
//! the import that may be cancelled.

use super::generation;
use super::lookup::Opened;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub struct DictState {
    pub dir: PathBuf,
    opened: Mutex<HashMap<String, Arc<Opened>>>,
    pins: Mutex<HashMap<String, usize>>,
    pub cancel: Arc<AtomicBool>,
    /// One import at a time.
    pub importing: Mutex<()>,
}

impl DictState {
    pub fn new(dir: PathBuf) -> Self {
        DictState {
            dir,
            opened: Mutex::new(HashMap::new()),
            pins: Mutex::new(HashMap::new()),
            cancel: Arc::new(AtomicBool::new(false)),
            importing: Mutex::new(()),
        }
    }

    /// A generation, opened once and kept while it is in use.
    pub fn open(&self, generation: &str) -> Option<Arc<Opened>> {
        if let Some(o) = self.opened.lock().unwrap().get(generation) {
            return Some(o.clone());
        }
        let o = Arc::new(Opened::open(&self.dir, generation).ok()?);
        self.opened
            .lock()
            .unwrap()
            .insert(generation.to_string(), o.clone());
        Some(o)
    }

    pub fn pin(&self, generation: &str) {
        *self
            .pins
            .lock()
            .unwrap()
            .entry(generation.to_string())
            .or_default() += 1;
    }

    pub fn pinned(&self, generation: &str) -> bool {
        self.pins
            .lock()
            .unwrap()
            .get(generation)
            .is_some_and(|&n| n > 0)
    }

    /// Unpin; returns whether nothing uses it any more.
    pub fn unpin(&self, generation: &str) -> bool {
        let mut pins = self.pins.lock().unwrap();
        match pins.get_mut(generation) {
            Some(n) if *n > 1 => {
                *n -= 1;
                false
            }
            _ => {
                pins.remove(generation);
                true
            }
        }
    }

    /// Delete every generation that is not `active` and that no open peek uses; at
    /// launch (`staging`), interrupted imports too.
    pub fn collect(&self, active: &[String], staging: bool) {
        let kept: Vec<String> = self.pins.lock().unwrap().keys().cloned().collect();
        {
            let mut opened = self.opened.lock().unwrap();
            opened.retain(|g, _| active.contains(g) || kept.contains(g));
        }
        if let Err(e) = generation::recover(&self.dir, active, &kept, staging) {
            log::warn!("dictionaries: could not remove old generations: {e}");
        }
    }
}
