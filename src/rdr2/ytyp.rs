//! RDR2 `.ytyp` (archetype definitions, RSC8 version 3 meta) writer: one
//! `CMapTypes` holding static prop `CBaseArchetypeDef`s and optionally one
//! interior `CMloArchetypeDef` (rooms, portals, entities, entity sets),
//! through the generic meta writer (`meta.rs`, tables in `meta_defs.rs`).
//!
//! Layout worked out from vanilla ytyps and checked against
//! CodeX-written files (CodeX used only as a parsing oracle; no CodeX code
//! is used). Prop archetype values copy vanilla `p_canteen02x` (a static
//! prop without collision): flags = loadFlags = 0x30C20000, avoidance 8,
//! ASSET_TYPE_DRAWABLE, MAP_ENTITY_TYPE_DUMMY_OBJECT, no extensions;
//! bbMin.w / bbMax.w repeat x as vanilla files do. MLO values follow the
//! MLO format spec (vanilla small interiors and the user's working RedM
//! interiors): see `Mlo`.

pub use super::meta::meta_ptr;
use super::meta::{self, Meta, Obj};
use super::meta_defs as d;

pub const YTYP_VERSION: u8 = meta::META_VERSION;
pub const META_MAGIC: u32 = meta::META_MAGIC;
pub const META_PLATFORM: u16 = meta::META_PLATFORM;

// case-sensitive joaat of the class / enum names (not `joaat()`, which lower-cases)
pub const CMAPTYPES: u32 = d::CMAPTYPES.name;
pub const CBASEARCHETYPEDEF: u32 = d::CBASEARCHETYPEDEF.name;
pub const CMLOARCHETYPEDEF: u32 = d::CMLOARCHETYPEDEF.name;
/// Data block type of StructPtr arrays (u64 meta pointers).
pub const BLOCK_PTRS: u32 = meta::T_PTRS;
pub const CMAPTYPES_SIZE: usize = 96;
pub const ARCHETYPE_SIZE: usize = 160;

/// Archetype field values (vanilla p_canteen02x).
pub const PROP_FLAGS: u32 = 0x30C2_0000;
pub const PROP_AVOIDANCE: u32 = 8;
pub const ASSET_TYPE_DRAWABLE: u32 = 2;
pub const ENTITY_DUMMY_OBJECT: u32 = 3;
/// MapEntityType of a CMloArchetypeDef.
pub const ENTITY_INTERIOR_INSTANCE: u32 = 5;
/// CEntityDef flags of interior entities (CastStaticShadows |
/// CastDynamicShadows: the commonest vanilla value, 441 of 1047).
pub const INTERIOR_ENTITY_FLAGS: u32 = 0x18_0000;
/// rage__eLodType LODTYPES_DEPTH_ORPHANHD.
pub const LOD_ORPHANHD: u32 = 5;

// field name hashes (joaat of the CodeX / vanilla field names)
pub(crate) mod f {
    pub const LOAD_FLAGS: u32 = 0xC067_A638;
    pub const LOD_DIST: u32 = 0xBF74_DFA7;
    pub const FLAGS: u32 = 0x67D2_6872;
    pub const AVOIDANCE: u32 = 0x8F21_5E15;
    pub const BB_MIN: u32 = 0xD9EF_8236;
    pub const BB_MAX: u32 = 0xE78A_A618;
    pub const BS_CENTRE: u32 = 0xD618_72FF;
    pub const BS_RADIUS: u32 = 0x82E8_C5DE;
    pub const HD_TEXTURE_DIST: u32 = 0xAD5D_5B4C;
    pub const NAME: u32 = 0xE6E1_7F14;
    pub const TXD: u32 = 0x75D2_15A1;
    pub const PHYSICS_DICTIONARY: u32 = 0xD3C7_17FC;
    pub const ASSET_TYPE: u32 = 0x0690_04B2;
    pub const ASSET_NAME: u32 = 0x9A77_C7C5;
    pub const ENTITY_TYPE: u32 = 0x07D1_64A8;
    pub const ARCHETYPES: u32 = 0x018A_3B1B;
    pub const ENTITIES: u32 = 0xCCAB_9307;
    pub const ROOMS: u32 = 0xCD20_D7A8;
    pub const PORTALS: u32 = 0x89F7_E992;
    pub const ENTITY_SETS: u32 = 0x45BC_B930;
    pub const ARCHETYPE_NAME: u32 = 0xA023_A02C;
    pub const ID: u32 = 0x1B60_404D;
    pub const POSITION: u32 = 0x0116_6164;
    pub const ROTATION: u32 = 0x77DD_4010;
    pub const SCALE_XY: u32 = 0x9CA3_2637;
    pub const SCALE_Z: u32 = 0x10FB_7C42;
    pub const PARENT_INDEX: u32 = 0xD892_31BD;
    pub const LOD_LEVEL: u32 = 0x6C8F_1715;
    pub const BLEND_AGE_LAYER: u32 = 0x0DC5_F270;
    pub const BLEND_AGE_DIRT: u32 = 0x540C_A436;
    pub const BLEND: u32 = 0xEA73_A60B;
    pub const TIMECYCLE: u32 = 0xA261_E0A9;
    pub const PORTAL_COUNT: u32 = 0x41E2_25B3;
    pub const EXTERIOR_VISIBILITY_DEPTH: u32 = 0x20F3_D23E;
    pub const ATTACHED: u32 = 0x8E05_312C;
    pub const ROOM_FROM: u32 = 0xF470_D2FD;
    pub const ROOM_TO: u32 = 0x9B64_9621;
    pub const PORTAL_F1: u32 = 0x7C09_6C2E;
    pub const PORTAL_F2: u32 = 0x9117_1666;
    pub const PORTAL_F3: u32 = 0x9681_6DDF;
    pub const PORTAL_F4: u32 = 0x0854_FDEE;
    pub const CORNERS: u32 = 0x4F42_AEB3;
    pub const LOCATIONS: u32 = 0xDDFC_2211;
}

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

/// An entity inside an MLO (interior-local CEntityDef).
#[derive(Debug, Clone, PartialEq)]
pub struct MloEntity {
    pub archetype: String,
    pub position: [f32; 3],
    /// x, y, z, w as stored: the CONJUGATE of the entity's local orientation
    pub rotation: [f32; 4],
    pub scale_xy: f32,
    pub scale_z: f32,
    /// -1 = the archetype's lodDist
    pub lod_dist: f32,
    pub flags: u32,
    /// 64-bit entity id (unique per file)
    pub id: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MloRoom {
    /// stored as a string; room 0 must be `limbo`
    pub name: String,
    pub bb_min: [f32; 3],
    pub bb_max: [f32; 3],
    /// timecycle name hash (0 = none)
    pub timecycle: u32,
    pub flags: u32,
    /// indices into `Mlo::entities`
    pub attached: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MloPortal {
    pub from: u32,
    pub to: u32,
    pub flags: u32,
    /// c0 top, c1 bottom, c2 bottom across, c3 top across; the normal
    /// (c1-c0)x(c2-c1) points away from `from`
    pub corners: [[f32; 3]; 4],
    pub attached: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MloSet {
    pub name: String,
    pub entities: Vec<MloEntity>,
    /// one room index per set entity
    pub locations: Vec<u32>,
}

/// One interior archetype (CMloArchetypeDef). Fixed values: flags 0,
/// bounds 0 (the rooms carry them), physicsDictionary = name (the
/// `<name>.ybn` / `hi@<name>.ybn` pair), assetType -1, entity type 5.
#[derive(Debug, Clone, PartialEq)]
pub struct Mlo {
    pub name: String,
    pub lod_dist: f32,
    pub mlo_flags: u32,
    pub entities: Vec<MloEntity>,
    pub rooms: Vec<MloRoom>,
    pub portals: Vec<MloPortal>,
    pub sets: Vec<MloSet>,
}

impl Mlo {
    /// Index rules of the format: room 0 named limbo, every entity index in
    /// exactly one room's or portal's attachedObjects, portal rooms in
    /// range, one location per set entity (a room index).
    pub fn check(&self) -> Result<(), String> {
        let n = self.entities.len();
        if self.rooms.first().map(|r| r.name.as_str()) != Some("limbo") {
            return Err(format!("{}: room 0 must be limbo", self.name));
        }
        let mut seen = vec![0u32; n];
        for i in self.rooms.iter().flat_map(|r| &r.attached).chain(self.portals.iter().flat_map(|p| &p.attached)) {
            *seen.get_mut(*i as usize).ok_or(format!("{}: attached entity {i} of {n}", self.name))? += 1;
        }
        if let Some(i) = seen.iter().position(|&c| c != 1) {
            return Err(format!("{}: entity {i} is in {} room/portal lists (want exactly 1)", self.name, seen[i]));
        }
        let nr = self.rooms.len() as u32;
        if let Some(p) = self.portals.iter().find(|p| p.from >= nr || p.to >= nr) {
            return Err(format!("{}: portal {}->{} outside {nr} rooms", self.name, p.from, p.to));
        }
        for s in &self.sets {
            if s.locations.len() != s.entities.len() || s.locations.iter().any(|&l| l >= nr) {
                return Err(format!("{}: entity set {}: {} locations for {} entities (rooms < {nr})", self.name, s.name, s.locations.len(), s.entities.len()));
            }
        }
        Ok(())
    }

    /// Portals touching each room (as roomFrom or roomTo).
    pub fn portal_counts(&self) -> Vec<u32> {
        (0..self.rooms.len() as u32).map(|r| self.portals.iter().filter(|p| p.from == r || p.to == r).count() as u32).collect()
    }

    /// Portals to limbo (CMloInstanceDef numExitPortals).
    pub fn exit_portals(&self) -> u32 {
        self.portals.iter().filter(|p| p.to == 0).count() as u32
    }
}

fn archetype(a: &Archetype) -> Obj {
    let mut o = Obj::new(&d::CBASEARCHETYPEDEF);
    o.u64(f::LOAD_FLAGS, PROP_FLAGS as u64).f32(f::LOD_DIST, a.lod_dist).u32(f::FLAGS, PROP_FLAGS).u32(f::AVOIDANCE, PROP_AVOIDANCE);
    o.f3(f::BB_MIN, a.bb_min, a.bb_min[0]).f3(f::BB_MAX, a.bb_max, a.bb_max[0]).f3(f::BS_CENTRE, a.bs_centre, 0.0);
    o.f32(f::BS_RADIUS, a.bs_radius).f32(f::HD_TEXTURE_DIST, a.hd_texture_dist);
    o.hash(f::NAME, &a.name).hash(f::TXD, &a.texture_dictionary).hash(f::PHYSICS_DICTIONARY, a.physics_dictionary.as_deref().unwrap_or(""));
    o.u32(f::ASSET_TYPE, ASSET_TYPE_DRAWABLE).hash(f::ASSET_NAME, &a.name).u32(f::ENTITY_TYPE, ENTITY_DUMMY_OBJECT);
    o
}

/// CEntityDef bytes (ymap or interior entity; ORPHANHD, PRI_REQUIRED,
/// parentIndex -1, blend ages 255).
pub fn entity_obj(class: &'static meta::ClassDef, e: &MloEntity) -> Obj {
    let mut o = Obj::new(class);
    o.hash(f::ARCHETYPE_NAME, &e.archetype).u32(f::FLAGS, e.flags).u64(f::ID, e.id);
    o.f3(f::POSITION, e.position, 0.0).f4(f::ROTATION, e.rotation).f32(f::SCALE_XY, e.scale_xy).f32(f::SCALE_Z, e.scale_z);
    o.i32(f::PARENT_INDEX, -1).f32(f::LOD_DIST, e.lod_dist).u32(f::LOD_LEVEL, LOD_ORPHANHD);
    o.u8(f::BLEND_AGE_LAYER, 255).u8(f::BLEND_AGE_DIRT, 255);
    o
}

fn entities(m: &mut Meta, es: &[MloEntity]) -> meta::Arr {
    let ptrs: Vec<u64> = es.iter().map(|e| m.object(&entity_obj(&d::CENTITYDEF, e))).collect();
    m.ptrs(&ptrs)
}

fn mlo(m: &mut Meta, x: &Mlo) -> Result<u64, String> {
    x.check()?;
    // the object first (its block precedes its arrays'), filled at the end
    let at = m.reserve(&d::CMLOARCHETYPEDEF, 1);
    let mut o = Obj::new(&d::CMLOARCHETYPEDEF);
    o.f32(f::LOD_DIST, x.lod_dist).f32(f::HD_TEXTURE_DIST, 5.0).hash(f::NAME, &x.name).hash(f::PHYSICS_DICTIONARY, &x.name);
    o.u32(f::ASSET_TYPE, u32::MAX).u32(f::ENTITY_TYPE, ENTITY_INTERIOR_INSTANCE).u32(0xD607_DE68, x.mlo_flags);
    let ents = entities(m, &x.entities);
    o.arr(f::ENTITIES, ents);
    let counts = x.portal_counts();
    if !x.rooms.is_empty() {
        let rp = m.reserve(&d::CMLOROOMDEF, x.rooms.len());
        for (k, r) in x.rooms.iter().enumerate() {
            let mut ro = Obj::new(&d::CMLOROOMDEF);
            let name = m.string(&r.name);
            let att = m.u32s(&r.attached);
            ro.arr(f::NAME, name).f3(f::BB_MIN, r.bb_min, 0.0).f3(f::BB_MAX, r.bb_max, 0.0).f32(f::BLEND, 1.0).u32(f::TIMECYCLE, r.timecycle);
            ro.u32(f::FLAGS, r.flags).u32(f::PORTAL_COUNT, counts[k]).i32(f::EXTERIOR_VISIBILITY_DEPTH, -1).arr(f::ATTACHED, att);
            m.set(rp + ((k * d::CMLOROOMDEF.size as usize) << 20) as u64, &ro.b);
        }
        o.arr(f::ROOMS, (rp, x.rooms.len() as u16));
    }
    if !x.portals.is_empty() {
        let pp = m.reserve(&d::CMLOPORTALDEF, x.portals.len());
        for (k, p) in x.portals.iter().enumerate() {
            let mut po = Obj::new(&d::CMLOPORTALDEF);
            // 4th lane = x, as CodeX and vanilla write corners
            let cs: Vec<[f32; 4]> = p.corners.iter().map(|c| [c[0], c[1], c[2], c[0]]).collect();
            let corners = m.f4s(&cs);
            let att = m.u32s(&p.attached);
            po.u32(f::ROOM_FROM, p.from).u32(f::ROOM_TO, p.to).u32(f::FLAGS, p.flags);
            po.f32(f::PORTAL_F1, 1.0).f32(f::PORTAL_F2, 1.0).f32(f::PORTAL_F3, 1.0).f32(f::PORTAL_F4, 1.0);
            po.arr(f::CORNERS, corners).arr(f::ATTACHED, att);
            m.set(pp + ((k * d::CMLOPORTALDEF.size as usize) << 20) as u64, &po.b);
        }
        o.arr(f::PORTALS, (pp, x.portals.len() as u16));
    }
    if !x.sets.is_empty() {
        let sp = m.reserve(&d::CMLOENTITYSET, x.sets.len());
        for (k, s) in x.sets.iter().enumerate() {
            let mut so = Obj::new(&d::CMLOENTITYSET);
            let loc = m.u32s(&s.locations);
            let es = entities(m, &s.entities);
            so.hash(f::NAME, &s.name).arr(f::LOCATIONS, loc).arr(f::ENTITIES, es);
            m.set(sp + ((k * d::CMLOENTITYSET.size as usize) << 20) as u64, &so.b);
        }
        o.arr(f::ENTITY_SETS, (sp, x.sets.len() as u16));
    }
    m.set(at, &o.b);
    Ok(at)
}

/// Build a .ytyp named `name` (CMapTypes `name` field) holding `archs`.
pub fn build_ytyp(name: &str, archs: &[Archetype]) -> Result<Vec<u8>, String> {
    build_ytyp_mlo(name, archs, None)
}

/// A .ytyp with prop archetypes and optionally one interior archetype
/// (listed after the props).
pub fn build_ytyp_mlo(name: &str, archs: &[Archetype], mlo_def: Option<&Mlo>) -> Result<Vec<u8>, String> {
    let n = archs.len() + usize::from(mlo_def.is_some());
    if n == 0 {
        return Err("ytyp: no archetypes".into());
    }
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
    let mut m = Meta::default();
    // data blocks in first-use order: CMapTypes, archetypes, (MLO and its
    // arrays), the archetypes pointer array
    let root = m.reserve(&d::CMAPTYPES, 1);
    let mut ptrs: Vec<u64> = archs.iter().map(|a| m.object(&archetype(a))).collect();
    if let Some(x) = mlo_def {
        ptrs.push(mlo(&mut m, x)?);
    }
    let list = m.ptrs(&ptrs);
    let mut r = Obj::new(&d::CMAPTYPES);
    r.arr(f::ARCHETYPES, list).hash(f::NAME, &name.to_ascii_lowercase());
    m.set(root, &r.b);
    m.build(1)
}
