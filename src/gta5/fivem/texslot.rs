//! Source-texture slots: `<id>.dds` in the texture store (fivem/texmem.rs,
//! texture `r6m_<id>`).
//!
//! The id is the 32-bit texture hash, but different 6m textures share 32-bit
//! hashes: the cache used to hand one texture's DDS to every later texture
//! with the same hash (a weapon's 4 x 4 placeholder came out as a building
//! wall). The texture's identity is its 64-bit hash (TTPF / texture record
//! +8) or its pool asset key, so each slot now records the identity that
//! owns it (store slot owners). A texture whose 32-bit slot belongs to another
//! identity moves to a slot derived from its 64-bit hash.
//!
//! Slots cached before this check have no `.id`: their owner is the
//! identity in the slot's record (`Src`), never a size match (a 1024 x 1024
//! data map "fitted" a 1024 x 1024 wall colour). Claims are atomic
//! (`Store::claim`): an unlocked check-then-set let two textures with the same
//! 32-bit hash both take the slot in a parallel build, and one got the
//! other's DDS. 6m ap_06_terminal_d alone binds 40 different textures under
//! `447e24d9` (the base colour of its two-layer walls, packed-data and normal
//! maps), so terminal walls drew a data map: white with dark blotches.

use super::texmem::{Src, Store};
use std::{
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

/// Lookups that moved off their 32-bit slot (collision), this process.
pub(super) static MOVED: AtomicUsize = AtomicUsize::new(0);

fn alt(ident: u64) -> u32 {
    ((ident ^ (ident >> 32)) as u32) ^ 0x9e37_79b9
}

fn same(stored: &str, key: u64, h64: u64) -> bool {
    let mut it = stored.split_whitespace().map(|x| u64::from_str_radix(x, 16).unwrap_or(0));
    let (k, h) = (it.next().unwrap_or(0), it.next().unwrap_or(0));
    if h64 != 0 && h != 0 {
        h == h64
    } else {
        key != 0 && k == key
    }
}

/// Identity recorded with a slot's DDS (slots made before owners were kept).
fn rec_ident(store: &Store, side: &str) -> Option<String> {
    Some(match store.rec(Path::new(&format!("{side}.dds")))?.src {
        Src::Pool { key } => format!("{key:016x} {:016x}", 0),
        Src::Ttpf { h64 } => format!("{:016x} {h64:016x}", 0),
        Src::Normal { key, h64, .. } | Src::Ma { key, h64, .. } => format!("{key:016x} {h64:016x}"),
        _ => return None,
    })
}

/// Cache id for a texture. `suffix` is the file suffix of the slot (`""`
/// colour, `"_n"` normal).
pub(super) fn slot(store: &Store, suffix: &str, hash: u32, key: u64, h64: u64) -> u32 {
    if key == 0 && h64 == 0 {
        return hash;
    }
    let ident = format!("{key:016x} {h64:016x}");
    let ids = [hash, alt(if h64 != 0 { h64 } else { key })];
    for (i, &id) in ids.iter().enumerate() {
        let side = format!("{id:08x}{suffix}");
        let owner = store.id(&side).unwrap_or_else(|| store.claim(&side, rec_ident(store, &side).unwrap_or_else(|| ident.clone())));
        if same(&owner, key, h64) {
            return id;
        }
        if i == 0 {
            MOVED.fetch_add(1, Ordering::Relaxed);
        }
    }
    ids[1]
}
