//! RDR2 `.ytyp` (archetype definitions, RSC8 version 3) writer for plain
//! static drawable props: one `CMapTypes` holding N `CBaseArchetypeDef`.
//!
//! Layout worked out from vanilla ytyps and checked against
//! CodeX-written files (CodeX used only as a parsing oracle; no
//! CodeX code is used). The payload is an RSC "meta" file, virtual segment
//! only: a 0x60-byte root (magic `0DRP`, platform 0x79), embedded class and
//! enum definition tables (constant in every file seen, so written from the
//! tables below), and typed data blocks. Inside data blocks, pointers are
//! meta pointers `(offset << 20) | block_id` (1-based index into the data
//! block list), never container pointers.
//!
//! Archetype values copy vanilla `p_canteen02x` (a static prop without
//! collision): flags = loadFlags = 0x30C20000, avoidance 8, ASSET_TYPE_DRAWABLE,
//! MAP_ENTITY_TYPE_DUMMY_OBJECT, no extensions; bbMin.w / bbMax.w repeat x
//! as vanilla files do.

use super::rsc8;
use crate::gta5::rsc::{joaat, Builder, Seg, W};

const YTYP_VERSION: u8 = 3;
const META_MAGIC: u32 = 0x5052_4430; // "0DRP"
const META_PLATFORM: u16 = 0x79;

// case-sensitive joaat of the class / enum names (not `joaat()`, which lower-cases)
pub const CMAPTYPES: u32 = 0xD98B_B561;
pub const CBASEARCHETYPEDEF: u32 = 0x82D6_FC83;
const ENUM_ASSET_TYPE: u32 = 0x76BA_F7C7; // rage__fwArchetypeDef__eAssetType
const ENUM_MAP_ENTITY_TYPE: u32 = 0x0795_25DA; // MapEntityType
/// Data block type of StructPtr arrays (u64 meta pointers).
const BLOCK_PTRS: u32 = 7;
const CMAPTYPES_SIZE: usize = 96;
const ARCHETYPE_SIZE: usize = 160;

/// Archetype field values (vanilla p_canteen02x).
const PROP_FLAGS: u32 = 0x30C2_0000;
const PROP_AVOIDANCE: u32 = 8;
const ASSET_TYPE_DRAWABLE: u32 = 2;
const ENTITY_DUMMY_OBJECT: u32 = 3;

// member types
const T_STRUCT: u8 = 0x05;
const T_STRUCTPTR: u8 = 0x07;
const T_U32: u8 = 0x15;
const T_U64: u8 = 0x17;
const T_F32: u8 = 0x21;
const T_F3: u8 = 0x33;
const T_HASH: u8 = 0x4A;
const T_ARRAY: u8 = 0x52;
const T_ENUM32: u8 = 0x62;
/// Name of array element descriptors.
const ELEM: u32 = 0x100;

/// Class member: (name hash, offset, type, element ref index, type name hash).
type Member = (u32, i32, u8, u16, u32);

/// One class definition: name, structure key, unknown (+8), size, members.
struct ClassDef {
    pub name: u32,
    pub key: u32,
    pub unk8: u32,
    pub size: u32,
    pub members: &'static [Member],
}

const CMAPTYPES_DEF: ClassDef = ClassDef {
    name: CMAPTYPES,
    key: 0x0EE4_F93D,
    unk8: 0x300,
    size: CMAPTYPES_SIZE as u32,
    members: &[
        (ELEM, 0, T_STRUCTPTR, 0, 0),
        (0xF17E_7F28, 8, T_ARRAY, 0, 0), // extensions
        (ELEM, 0, T_STRUCTPTR, 0, 0),
        (0x018A_3B1B, 24, T_ARRAY, 2, 0), // archetypes
        (ELEM, 0, T_STRUCTPTR, 0, 0),
        (0x069A_DF93, 40, T_ARRAY, 4, 0), // txdRelationships
        (0xE6E1_7F14, 56, T_HASH, 0, 0),  // name
        (ELEM, 0, T_HASH, 0, 0),
        (0x3C6F_8844, 64, T_ARRAY, 7, 0), // dependencies
        (ELEM, 0, T_STRUCT, 0, 0x46AD_6DFF), // CCompositeEntityType
        (0x65E9_D4EB, 80, T_ARRAY, 9, 0), // compositeEntityTypes
    ],
};

const CBASEARCHETYPEDEF_DEF: ClassDef = ClassDef {
    name: CBASEARCHETYPEDEF,
    key: 0x6DBC_483F,
    unk8: 0x400,
    size: ARCHETYPE_SIZE as u32,
    members: &[
        (0xC067_A638, 8, T_U64, 0, 0),    // loadFlags
        (0xBF74_DFA7, 16, T_F32, 0, 0),   // lodDist
        (0x67D2_6872, 20, T_U32, 0, 0),   // flags
        (0x8F21_5E15, 24, T_U32, 0, 0),   // avoidanceflags
        (0x6C15_23E4, 28, T_U32, 0, 0),   // specialAttribute
        (0xD9EF_8236, 32, T_F3, 0, 0),    // bbMin
        (0xE78A_A618, 48, T_F3, 0, 0),    // bbMax
        (0xD618_72FF, 64, T_F3, 0, 0),    // bsCentre
        (0x82E8_C5DE, 80, T_F32, 0, 0),   // bsRadius
        (0xAD5D_5B4C, 84, T_F32, 0, 0),   // hdTextureDist
        (0xE6E1_7F14, 88, T_HASH, 0, 0),  // name
        (0x75D2_15A1, 92, T_HASH, 0, 0),  // textureDictionary
        (0x1947_1791, 96, T_HASH, 0, 0),  // clipDictionary
        (0xFC1F_B6FB, 100, T_HASH, 0, 0), // drawableDictionary
        (0xD3C7_17FC, 104, T_HASH, 0, 0), // physicsDictionary
        (0x0690_04B2, 108, T_ENUM32, 0, ENUM_ASSET_TYPE), // assetType
        (0x9A77_C7C5, 112, T_HASH, 0, 0), // assetName
        (ELEM, 0, T_STRUCTPTR, 0, 0),
        (0xF17E_7F28, 120, T_ARRAY, 17, 0), // extensions
        (0x9A7B_6E6D, 136, T_U64, 0, 0),    // guid
        (0x07D1_64A8, 144, T_ENUM32, 0, ENUM_MAP_ENTITY_TYPE), // entity type (name unresolved)
    ],
};

/// One enum definition: name, key, (value, name hash).
struct EnumDef {
    pub name: u32,
    pub key: u32,
    pub values: &'static [(i64, u32)],
}

const ASSET_TYPE_DEF: EnumDef = EnumDef {
    name: ENUM_ASSET_TYPE,
    key: 0x5D19_BE6B,
    values: &[(0, 0x0B4F_1FED), (1, 0x2209_7FE7), (2, 0x07C0_CB71), (3, 0x5E2F_6A14), (4, 0xF805_1E71)],
};

const MAP_ENTITY_TYPE_DEF: EnumDef = EnumDef {
    name: ENUM_MAP_ENTITY_TYPE,
    key: 0xB80E_C510,
    values: &[(0, 0x9B93_1532), (1, 0xAD1E_ED63), (2, 0x1934_1C87), (3, 0x7CD1_64E4), (4, 0x7006_9A13), (5, 0x2BE5_C1FB), (6, 0x65D4_F74A), (7, 0x8301_70E5)],
};

/// One static prop archetype. Names are stored as joaat hashes (lower case).
#[derive(Debug, Clone, PartialEq)]
pub struct Archetype {
    /// model name: archetype `name` and `assetName` (= the .ydr file stem)
    pub name: String,
    /// .ytd file stem
    pub texture_dictionary: String,
    pub lod_dist: f32,
    pub hd_texture_dist: f32,
    pub bb_min: [f32; 3],
    pub bb_max: [f32; 3],
    pub bs_centre: [f32; 3],
    pub bs_radius: f32,
    /// set (to the model name) only when the .ydr embeds collision bounds
    pub physics_dictionary: Option<String>,
}

impl Archetype {
    /// Archetype from a model box: texture dictionary = model name, sphere
    /// = box centre and half diagonal (as vanilla), hdTextureDist 5.
    pub fn from_box(name: &str, lod_dist: f32, bb_min: [f32; 3], bb_max: [f32; 3]) -> Archetype {
        let c = std::array::from_fn(|k| (bb_min[k] + bb_max[k]) / 2.0);
        Archetype {
            name: name.to_string(),
            texture_dictionary: name.to_string(),
            lod_dist,
            hd_texture_dist: 5.0,
            bb_min,
            bb_max,
            bs_centre: c,
            bs_radius: half_diagonal(bb_min, bb_max),
            physics_dictionary: None,
        }
    }
}

pub fn half_diagonal(lo: [f32; 3], hi: [f32; 3]) -> f32 {
    (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f32>().sqrt() / 2.0
}

/// Meta pointer to `offset` inside data block `id` (1-based).
pub fn meta_ptr(id: u32, offset: usize) -> u64 {
    (offset as u64) << 20 | id as u64
}

/// Hash field: joaat of the lower-cased name, 0 for none.
fn name_hash(s: &str) -> u32 {
    if s.is_empty() { 0 } else { joaat(s) }
}

fn class_entries(c: &ClassDef) -> W {
    let mut w = W::new();
    for &(name, off, ty, rf, tn) in c.members {
        w.u32(name).i32(off).u8(ty).u8(0).u16(rf).u32(tn);
    }
    w
}

fn enum_values(e: &EnumDef) -> W {
    let mut w = W::new();
    for &(v, h) in e.values {
        w.u64(v as u64).u32(h).u32(0);
    }
    w
}

fn archetype(w: &mut W, a: &Archetype) {
    w.u64(0).u64(PROP_FLAGS as u64).f32(a.lod_dist).u32(PROP_FLAGS).u32(PROP_AVOIDANCE).u32(0);
    w.v3(a.bb_min).f32(a.bb_min[0]).v3(a.bb_max).f32(a.bb_max[0]).v3(a.bs_centre).f32(0.0);
    w.f32(a.bs_radius).f32(a.hd_texture_dist);
    w.u32(name_hash(&a.name)).u32(name_hash(&a.texture_dictionary)).u32(0).u32(0);
    w.u32(a.physics_dictionary.as_deref().map(name_hash).unwrap_or(0));
    w.u32(ASSET_TYPE_DRAWABLE).u32(name_hash(&a.name)).u32(0);
    w.zeros(16).u64(0).u32(ENTITY_DUMMY_OBJECT).zeros(12);
}

/// Build a .ytyp named `name` (CMapTypes `name` field) holding `archs`.
pub fn build_ytyp(name: &str, archs: &[Archetype]) -> Result<Vec<u8>, String> {
    let n = archs.len();
    if n == 0 {
        return Err("ytyp: no archetypes".into());
    }
    // 20-bit offsets inside one data block (a second block is not written)
    if n * ARCHETYPE_SIZE >= 0xF_FFFF {
        return Err(format!("ytyp: {n} archetypes do not fit one data block (max {})", 0xF_FFFF / ARCHETYPE_SIZE));
    }
    for a in archs {
        if a.name.is_empty() || a.texture_dictionary.is_empty() {
            return Err("ytyp: archetype with an empty name or texture dictionary".into());
        }
        let v = [a.lod_dist, a.hd_texture_dist, a.bs_radius].into_iter().chain(a.bb_min).chain(a.bb_max).chain(a.bs_centre);
        if v.into_iter().any(|x| !x.is_finite()) {
            return Err(format!("ytyp: {}: non-finite bounds or distances", a.name));
        }
    }
    let mut b = Builder::new();
    // creation order: same-size blocks keep it (BlockMap, EnumRefs, DataBlocks
    // land as in CodeX output)
    let root = b.reserve(Seg::Sys);
    let arch_entries = b.add(Seg::Sys, class_entries(&CBASEARCHETYPEDEF_DEF));
    let map_entries = b.add(Seg::Sys, class_entries(&CMAPTYPES_DEF));
    let asset_vals = b.add(Seg::Sys, enum_values(&ASSET_TYPE_DEF));
    let entity_vals = b.add(Seg::Sys, enum_values(&MAP_ENTITY_TYPE_DEF));
    let bm = b.sys_bytes(vec![0u8; 48]);
    b.pages_info = Some(bm);

    let mut classes = W::new();
    for (c, e) in [(&CMAPTYPES_DEF, map_entries), (&CBASEARCHETYPEDEF_DEF, arch_entries)] {
        classes.u32(c.name).u32(c.key).u32(c.unk8).u32(0).ptr(Some(e)).u32(c.size).u16(0).u16(c.members.len() as u16);
    }
    let classes = b.add(Seg::Sys, classes);
    let mut enums = W::new();
    for (e, v) in [(&ASSET_TYPE_DEF, asset_vals), (&MAP_ENTITY_TYPE_DEF, entity_vals)] {
        enums.u32(e.name).u32(e.key).ptr(Some(v)).u32(e.values.len() as u32).u32(0);
    }
    let enums = b.add(Seg::Sys, enums);

    // data blocks: 1 CMapTypes (root object), 2 archetypes, 3 pointer array
    let mut m = W::new();
    m.u64(0).zeros(16);
    m.u64(meta_ptr(3, 0)).u16(n as u16).u16(n as u16).u32(0);
    m.zeros(16).u32(name_hash(&name.to_ascii_lowercase())).u32(0).zeros(32);
    debug_assert_eq!(m.len(), CMAPTYPES_SIZE);
    let mut aw = W::new();
    archs.iter().for_each(|a| archetype(&mut aw, a));
    debug_assert_eq!(aw.len(), n * ARCHETYPE_SIZE);
    let mut pw = W::new();
    (0..n).for_each(|i| {
        pw.u64(meta_ptr(2, i * ARCHETYPE_SIZE));
    });
    let data = [(CMAPTYPES, m), (CBASEARCHETYPEDEF, aw), (BLOCK_PTRS, pw)];
    let mut list = W::new();
    for (ty, w) in data {
        let len = w.len() as u32;
        list.u32(ty).u32(len).ptr(Some(b.add(Seg::Sys, w)));
    }
    let nblocks = list.len() / 16;
    let list_id = b.add(Seg::Sys, list);
    let unk = b.sys_bytes(vec![0u8; 12]);

    let mut r = W::new();
    r.u64(0).ptr(Some(bm)).u32(META_MAGIC).u16(META_PLATFORM).u8(0).u8(0).u64(0);
    r.u32(1).u32(0); // RootBlockId: the CMapTypes block
    r.ptr(Some(classes)).ptr(Some(enums)).ptr(Some(list_id)).ptr(Some(unk));
    r.u16(2).u16(2).u16(nblocks as u16).u16(0).u64(0).u64(0);
    debug_assert_eq!(r.len(), 0x60);
    b.set(root, r);
    rsc8::build(b, YTYP_VERSION)
}
