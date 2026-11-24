//! Source textures in memory (replaces <work>/fivem_tex_cache_v*i).
//!
//! A texture is addressed as before by a virtual path `<dir>/<id>.dds`
//! (texture `r6m_<id>`), keyed here by file name. Each one has a record:
//! where it comes from (`Src`, enough to decode it again from the game
//! files), its DDS size and dimensions. The records and the slot owners
//! (fivem/texslot.rs) are the only thing kept on disk
//! (<work>/index/fivem_textures_v<N>.json, small); the bytes are decoded
//! when needed and held in memory up to a budget.

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

/// How to make a texture's DDS again.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Src {
    /// ttdb pool entry by asset key
    Pool { key: u64 },
    /// TTPF store entry by 64-bit hash
    Ttpf { h64: u64 },
    /// tangent-space normal map of a texture
    Normal { hash: u32, key: u64, h64: u64 },
    /// colour texture `base` (stem, or "w" for none) times `rgb`
    Tint { base: String, rgb: [u8; 3] },
    FlatBump,
    FlatSpec,
    /// RDR2 `_ma` made from a 6m speculartex (`redm prop` only)
    Ma { hash: u32, key: u64, h64: u64 },
    /// only in memory (vehicle / weapon runs, never persisted)
    Mem,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Rec {
    pub src: Src,
    pub len: u64,
    pub w: u32,
    pub h: u32,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    recs: BTreeMap<String, Rec>,
    ids: BTreeMap<String, String>,
}

pub struct Store {
    recs: Mutex<BTreeMap<String, Rec>>,
    ids: Mutex<BTreeMap<String, String>>,
    mem: Mutex<HashMap<String, Arc<Vec<u8>>>>,
    used: AtomicU64,
    budget: u64,
    file: Option<PathBuf>,
}

fn key(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// (width, height) from a DDS header.
pub fn dds_dims(b: &[u8]) -> Option<(u32, u32)> {
    (b.len() >= 20 && &b[..4] == b"DDS ").then(|| (u32::from_le_bytes([b[16], b[17], b[18], b[19]]), u32::from_le_bytes([b[12], b[13], b[14], b[15]])))
}

impl Store {
    /// Persistent store (records in `file`), bytes held up to `budget`.
    pub fn open(file: PathBuf, budget: u64) -> Store {
        let s: Saved = fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Store { recs: Mutex::new(s.recs), ids: Mutex::new(s.ids), mem: Default::default(), used: AtomicU64::new(0), budget, file: Some(file) }
    }

    /// Memory-only store for one run (every texture kept).
    pub fn temp() -> Store {
        Store { recs: Default::default(), ids: Default::default(), mem: Default::default(), used: AtomicU64::new(0), budget: u64::MAX, file: None }
    }

    pub fn exists(&self, p: &Path) -> bool {
        self.recs.lock().unwrap().contains_key(&key(p))
    }

    pub fn rec(&self, p: &Path) -> Option<Rec> {
        self.recs.lock().unwrap().get(&key(p)).cloned()
    }

    pub fn len(&self, p: &Path) -> u64 {
        self.rec(p).map_or(0, |r| r.len)
    }

    /// Bytes when they are in memory.
    pub fn cached(&self, p: &Path) -> Option<Arc<Vec<u8>>> {
        self.mem.lock().unwrap().get(&key(p)).cloned()
    }

    /// Keep `b` in memory if the budget allows.
    pub fn hold(&self, p: &Path, b: Arc<Vec<u8>>) {
        let n = b.len() as u64;
        if self.used.fetch_add(n, Ordering::Relaxed) + n <= self.budget {
            self.mem.lock().unwrap().insert(key(p), b);
        } else {
            self.used.fetch_sub(n, Ordering::Relaxed);
        }
    }

    /// Record a texture (and hold its bytes).
    pub fn put(&self, p: &Path, b: Vec<u8>, src: Src) -> Arc<Vec<u8>> {
        let (w, h) = dds_dims(&b).unwrap_or((0, 0));
        self.recs.lock().unwrap().insert(key(p), Rec { src, len: b.len() as u64, w, h });
        let b = Arc::new(b);
        self.hold(p, b.clone());
        b
    }

    pub fn remove(&self, p: &Path) {
        let k = key(p);
        self.recs.lock().unwrap().remove(&k);
        if let Some(b) = self.mem.lock().unwrap().remove(&k) {
            self.used.fetch_sub(b.len() as u64, Ordering::Relaxed);
        }
    }

    /// Slot owner (texslot), by slot file name `<id><suffix>`.
    pub fn id(&self, slot: &str) -> Option<String> {
        self.ids.lock().unwrap().get(slot).cloned()
    }

    /// Owner of `slot`, recording `ident` first when it has none, under one
    /// lock (texslot: two textures must never both take a slot).
    pub fn claim(&self, slot: &str, ident: String) -> String {
        self.ids.lock().unwrap().entry(slot.to_string()).or_insert(ident).clone()
    }

    /// Write the records (persistent stores only).
    pub fn save(&self) -> Result<(), String> {
        let Some(f) = &self.file else { return Ok(()) };
        let s = Saved { recs: self.recs.lock().unwrap().clone(), ids: self.ids.lock().unwrap().clone() };
        super::write_atomic(f, &serde_json::to_vec(&s).unwrap()).map_err(|e| format!("{}: {e}", f.display()))
    }
}
