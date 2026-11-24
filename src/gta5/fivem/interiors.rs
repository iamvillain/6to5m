//! Interiors: sixng interior instances (<placements>/interiors.jsonl plus
//! the interior-space entity files) -> GTA V MLOs. One CMloArchetypeDef per
//! 6m interior archetype in t6m_int.ytyp (decoded rooms, entity sets, the
//! interior entities) and CMloInstanceDef placements in 512 m
//! t6m_int_X_Y.ymap cells. Entity archetypes come from t6m_bld.ytyp
//! (converted by buildings.rs).
//!
//! Rooms and portals are read from the 6m CMloArchetypeDef (`995072ca`
//! portals: roomFrom/roomTo indices, 4 corners) and each entity goes in its
//! 6m room. Interiors whose portals do not decode keep every entity in
//! limbo (room 0, drawn from outside); T6M_MLO_LIMBO=1 forces that for all.
//! Interior collision (the bounds sixng links to the interior) is written
//! per instance as t6m_intcol_<hash>_<n>.ybn in world space, each polygon
//! carrying the room id of the room whose bounds contain it (6m bounds have
//! no room ids; limbo when none), so the game knows the player is inside.
//! Lights: the 6m light sets of each interior entity's archetype and of
//! the entity itself become a CExtensionDefLightEffect on that MLO entity.
//! They are decoded from the interiors' own meta files during the build
//! (no extra pass). T6M_NO_LIGHTS=1 skips lights.
//! Entity-set entities go in CMloEntitySets, off by default
//! (ActivateInteriorEntitySet turns them on).

use super::super::Ctx;
use super::emit_meta;
use crate::gta5::meta::{self, arr, Archetype, EInfo, SInfo, MB};
use crate::gta5::rsc::W;
use serde_json::{json, Value};
use sixng_core::{meta7a, resource::Resource, scan::{ArchiveIndex, Entry}};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
    sync::Arc,
};

type V3 = [f32; 3];
type Q = [f32; 4];

pub struct Child {
    pub name: String,
    pub off: usize,
    pos: V3,
    /// as stored in CEntityDef (inverse of the interior-space orientation)
    rot: Q,
    sxy: f32,
    sz: f32,
    lod: f32,
    room: u32,
    set: Option<u32>,
    /// 6m lights (sixng `lights` sets: the archetype's plus the entity's),
    /// entity-local
    lights: Vec<Value>,
    /// (meta file offset, entity index in that file): finds entity lights
    src: (String, usize),
}

pub struct Interior {
    hash: u32,
    bbox: [V3; 2],
    /// (name hash, name) in file order; limbo first
    rooms: Vec<(u32, Option<String>)>,
    pub children: Vec<Child>,
    /// (position, rotation as stored: world orientation, not inverted)
    instances: Vec<(V3, Q)>,
    /// decoded 6m portals; None = not decoded (entities stay in limbo)
    portals: Option<Vec<Portal>>,
    /// interior-space collision bounds linked by sixng (exact or high/medium)
    col: Option<Entry>,
}

#[derive(Clone, serde::Serialize)]
pub struct Portal {
    /// room indices (into the interior's rooms, limbo = 0)
    pub from: u32,
    pub to: u32,
    /// interior space
    pub corners: [V3; 4],
    /// 6m portal flags (`67d26872`, GTA V CMloPortalDef flags; meaning not
    /// checked for 6m). `emit` writes 0, as before.
    pub flags: u32,
}

/// What one 6m interior archetype's meta object holds beyond the sixng
/// placement JSON: portals and per-room bounds and flags.
struct MloMeta {
    /// Some when the archetype has a portal list and every portal decoded
    portals: Option<Vec<Portal>>,
    /// per room in file order (same order as the placement JSON's rooms)
    rooms: Vec<RoomMeta>,
}

#[derive(Clone, Copy, Default)]
struct RoomMeta {
    bounds: Option<[V3; 2]>,
    flags: Option<u32>,
}

/// Add another archive's interiors: an interior archetype already loaded
/// (same name hash) gains the new instances, others are appended.
pub fn merge(into: &mut Vec<Interior>, more: Vec<Interior>) {
    for m in more {
        match into.iter_mut().find(|i| i.hash == m.hash) {
            Some(i) => i.instances.extend(m.instances),
            None => into.push(m),
        }
    }
}

/// Portals of every interior archetype in one 6m meta file, by name hash
/// (archetypes whose portals do not all decode are left out).
fn file_portals(levels: &Path, index: &ArchiveIndex, off: u64) -> HashMap<u32, Vec<Portal>> {
    file_mlos(levels, index, off).into_iter().filter_map(|(h, m)| Some((h, m.portals?))).collect()
}

/// Portals and room bounds of every interior archetype in one 6m meta
/// file (entry at `off`), by name hash.
fn file_mlos(levels: &Path, index: &ArchiveIndex, off: u64) -> HashMap<u32, MloMeta> {
    let mut out = HashMap::new();
    let Some(e) = index.entries.iter().find(|e| e.off == off) else { return out };
    let Ok(r) = Resource::load_sys(levels, e) else { return out };
    let Some(m) = meta7a::Meta::parse(&r) else { return out };
    let get = |v: &Value, h: u32| v.get(meta7a::field_name(h)).or_else(|| v.get(format!("{h:08x}"))).cloned();
    for (mlo, o) in meta7a::mlos(&m).iter().zip(m.objects(meta7a::MLO_ARCHETYPE)) {
        let Some(v) = m.object(meta7a::MLO_ARCHETYPE, o, 4) else { continue };
        // rooms (`cd20d7a8`): bbMin / bbMax / flags as GTA V CMloRoomDef
        let rooms = get(&v, 0xcd20_d7a8)
            .and_then(|x| x.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|r| {
                let v3 = |h| get(r, h).and_then(|x| x.as_array().filter(|a| a.len() >= 3).map(|_| f3(&x)));
                RoomMeta { bounds: v3(0xd9ef_8236).zip(v3(0xe78a_a618)).map(|(a, b)| [a, b]), flags: get(r, 0x67d2_6872).and_then(|x| x.as_u64()).map(|x| x as u32) }
            })
            .collect();
        let portals = get(&v, 0x89f7_e992).and_then(|x| x.as_array().cloned()).and_then(|list| {
            let mut ps = Vec::new();
            for p in &list {
                let n = |h| get(p, h).and_then(|x| x.as_u64()).map(|x| x as u32);
                let cs: Vec<V3> = get(p, 0x4f42_aeb3).and_then(|x| x.as_array().cloned()).unwrap_or_default().iter().map(f3).collect();
                if let (Some(from), Some(to), Ok(corners)) = (n(0xf470_d2fd), n(0x9b64_9621), <[V3; 4]>::try_from(cs)) {
                    ps.push(Portal { from, to, corners, flags: n(0x67d2_6872).unwrap_or(0) });
                }
            }
            (ps.len() == list.len()).then_some(ps)
        });
        out.insert(mlo.name, MloMeta { portals, rooms });
    }
    out
}

/// An interior entity's fields as the sixng placement JSON stores them
/// (shared by `load` and `source`).
struct RawEnt<'a> {
    name: Option<&'a str>,
    model_id: Option<&'a str>,
    pos: V3,
    /// as stored in CEntityDef (inverse of the interior-space orientation)
    rot: Q,
    sxy: f32,
    sz: f32,
    lod: f32,
    room: Option<u32>,
    set: Option<u32>,
}

fn raw_ent(e: &Value) -> RawEnt<'_> {
    RawEnt {
        name: e["name"].as_str(),
        model_id: e["model_id"].as_str(),
        pos: f3(&e["position"]),
        rot: q4(&e["rotation_world"]).map(|q| [-q[0], -q[1], -q[2], q[3]]).or_else(|| q4(&e["rotation"])).unwrap_or([0.0, 0.0, 0.0, 1.0]),
        sxy: e["scale_xy"].as_f64().unwrap_or(1.0) as f32,
        sz: e["scale_z"].as_f64().unwrap_or(1.0) as f32,
        lod: e["lod"]["lod_dist"].as_f64().unwrap_or(-1.0) as f32,
        room: hx(&e["interior"]["room"]),
        set: hx(&e["interior"]["entity_set"]),
    }
}

/// Entity indices (into the interior file's entity list) of one interior
/// instance line: its entities, then its entity-set entities.
fn inst_entities(inst: &Value) -> impl Iterator<Item = usize> + '_ {
    inst["entities"].as_array().into_iter().flatten().chain(inst["entity_set_entities"].as_array().into_iter().flatten()).filter_map(|v| v.as_u64()).map(|i| i as usize)
}

/// (name hash, name) of the rooms of interior `h` in its file's placement
/// JSON, in file order; a lone limbo when it lists none.
fn rooms_of(src: &Value, h: u32) -> Vec<(u32, Option<String>)> {
    let info = src["interiors"].as_array().into_iter().flatten().find(|v| hx(&v["interior"]) == Some(h));
    let mut rooms: Vec<(u32, Option<String>)> =
        info.and_then(|v| v["rooms"].as_array()).into_iter().flatten().filter_map(|r| Some((hx(&r["room"])?, r["name"].as_str().map(str::to_string)))).collect();
    if rooms.is_empty() {
        rooms.push((crate::gta5::rsc::joaat("limbo"), Some("limbo".into())));
    }
    rooms
}

/// Collision bounds id used for an interior instance: an exact bbox match,
/// else sixng's linked bounds when the link is high or medium confidence.
fn collision_id(inst: &Value) -> Option<&str> {
    let l = &inst["collision_linked"];
    inst["collision"].as_array().and_then(|a| a.first()).and_then(Value::as_str).or_else(|| l["id"].as_str().filter(|_| matches!(l["confidence"].as_str(), Some("high" | "medium"))))
}

fn f3(v: &Value) -> V3 {
    std::array::from_fn(|k| v[k].as_f64().unwrap_or(0.0) as f32)
}
fn q4(v: &Value) -> Option<Q> {
    let a = v.as_array().filter(|a| a.len() == 4)?;
    Some(std::array::from_fn(|k| a[k].as_f64().unwrap_or(0.0) as f32))
}
fn hx(v: &Value) -> Option<u32> {
    v.as_str().and_then(|s| u32::from_str_radix(s, 16).ok())
}
fn qrot(q: Q, v: V3) -> V3 {
    let [x, y, z, w] = q;
    let t = [2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0])];
    [v[0] + w * t[0] + (y * t[2] - z * t[1]), v[1] + w * t[1] + (z * t[0] - x * t[2]), v[2] + w * t[2] + (x * t[1] - y * t[0])]
}

/// Interiors whose instances fall in the region, one per archetype.
/// Interiors of one archive (`archive_id`, e.g. "levels.rpf"): its
/// placement dir and index. Child model offsets are virtual (Ctx::voff_of).
pub fn load(ctx: &Ctx, dir: &Path, index: &ArchiveIndex, archive_id: &str) -> Vec<Interior> {
    let levels = if archive_id == "levels.rpf" { ctx.o.game.join("levels.rpf") } else { ctx.o.game.join(archive_id) };
    let prefix = format!("{archive_id}@");
    let limbo = std::env::var_os("T6M_MLO_LIMBO").is_some();
    let mut meta: HashMap<String, HashMap<u32, Vec<Portal>>> = HashMap::new();
    let by_id: HashMap<&str, &Entry> = index.entries.iter().map(|e| (e.id.as_str(), e)).collect();
    let Ok(text) = sixng_core::vfs::read_to_string(dir.join("interiors.jsonl")) else { return Vec::new() };
    let mut files: HashMap<String, Option<Arc<Value>>> = HashMap::new();
    let mut by_hash: BTreeMap<u32, Interior> = BTreeMap::new();
    let mut missing = 0;
    for l in text.lines() {
        let Ok(inst) = serde_json::from_str::<Value>(l) else { continue };
        let ip = f3(&inst["position"]);
        if !ctx.o.in_bbox(ip, ip) {
            continue;
        }
        let ip = ctx.o.shift(ip);
        let Some(h) = hx(&inst["archetype"]) else { continue };
        let ri = q4(&inst["rotation_world"]).unwrap_or([0.0, 0.0, 0.0, 1.0]);
        if let Some(i) = by_hash.get_mut(&h) {
            i.instances.push((ip, ri));
            continue;
        }
        let Some(file) = inst["interior_file"].as_str().and_then(|f| f.strip_prefix(prefix.as_str())) else {
            missing += 1;
            continue;
        };
        let src = files.entry(file.to_string()).or_insert_with(|| sixng_core::vfs::read(dir.join(format!("{file}.json"))).ok().and_then(|b| serde_json::from_slice(&b).ok()).map(Arc::new)).clone();
        let Some(src) = src else {
            missing += 1;
            continue;
        };
        let ents = src["entities"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let rooms = rooms_of(&src, h);
        let mut children = Vec::new();
        for ei in inst_entities(&inst) {
            let Some(e) = ents.get(ei) else { continue };
            let r = raw_ent(e);
            let (Some(name), Some(off)) = (r.name, r.model_id.and_then(|m| ctx.voff_of(m))) else {
                continue;
            };
            children.push(Child {
                name: name.to_string(),
                off,
                pos: r.pos,
                rot: r.rot,
                sxy: r.sxy,
                sz: r.sz,
                lod: r.lod,
                room: r.room.unwrap_or(0),
                set: r.set,
                lights: Vec::new(),
                src: (file.to_string(), ei),
            });
        }
        let bbox = serde_json::from_value(inst["bbox"].clone()).unwrap_or([[-1.0; 3], [1.0; 3]]);
        let portals = if limbo {
            None
        } else {
            let off = u64::from_str_radix(file, 16).unwrap_or(0);
            meta.entry(file.to_string()).or_insert_with(|| file_portals(&levels, index, off)).get(&h).cloned().filter(|p| p.iter().all(|p| (p.from as usize) < rooms.len() && (p.to as usize) < rooms.len()))
        };
        let col = collision_id(&inst).and_then(|id| by_id.get(id)).map(|e| (*e).clone());
        by_hash.insert(h, Interior { hash: h, bbox, rooms, children, instances: vec![(ip, ri)], portals, col });
    }
    if missing > 0 {
        ctx.skip(&format!("({missing} interior instances)"), "interior archetype file missing", json!({"count": missing}));
    }
    let mut out: Vec<Interior> = by_hash.into_values().collect();
    let (arch_lights, ent_lights) = light_sets(ctx, index, &out, &levels);
    for c in out.iter_mut().flat_map(|i| i.children.iter_mut()) {
        c.lights = arch_lights.get(&crate::gta5::rsc::joaat(&c.name)).into_iter().chain(ent_lights.get(&c.src)).flatten().cloned().collect();
    }
    if !out.is_empty() {
        let n: usize = out.iter().map(|i| i.instances.len()).sum();
        let np = out.iter().filter(|i| i.portals.is_some()).count();
        eprintln!("[t6m:interiors] {} MLO archetypes ({np} with portals), {n} instances, {} entities", out.len(), out.iter().map(|i| i.children.len()).sum::<usize>());
    }
    out
}

// CodeWalker MetaTypes.GetStructureInfo, verbatim.
const MLOINST: SInfo = SInfo { name: 0x09cc28be, key: 0x803e74b0, unk8: 0x400, size: 160, entries: MLOINST_E };
const MLOINST_E: &[(u32, i32, u8, u8, i16, u32)] = &[
    (0xa023a02c, 8, 0x4a, 0, 0, 0),
    (0x67d26872, 12, 0x15, 0, 0, 0),
    (0x9a7b6e6d, 16, 0x15, 0, 0, 0),
    (0x01166164, 32, 0x33, 0, 0, 0),
    (0x77dd4010, 48, 0x34, 0, 0, 0),
    (0x9ca32637, 64, 0x21, 0, 0, 0),
    (0x10fb7c42, 68, 0x21, 0, 0, 0),
    (0xd89231bd, 72, 0x14, 0, 0, 0),
    (0xbf74dfa7, 76, 0x21, 0, 0, 0),
    (0xca974bcd, 80, 0x21, 0, 0, 0),
    (0x6c8f1715, 84, 0x62, 0, 0, 0x4b5acc2f),
    (0xa687ac89, 88, 0x15, 0, 0, 0),
    (0x2691f019, 92, 0x62, 0, 0, 0x26a60207),
    (0x00000100, 0, 0x07, 0, 0, 0),
    (0xf17e7f28, 96, 0x52, 0, 13, 0),
    (0x18c1d587, 112, 0x14, 0, 0, 0),
    (0x23c0e543, 116, 0x14, 0, 0, 0),
    (0x3c852527, 120, 0x15, 0, 0, 0),
    (0x951bdd14, 128, 0x15, 0, 0, 0),
    (0x8264e631, 132, 0x15, 0, 0, 0),
    (0x00000100, 0, 0x4a, 0, 0, 0),
    (0x53df8649, 136, 0x52, 0, 20, 0),
    (0x1f837fb7, 152, 0x15, 0, 0, 0),
    (0xe03b0caa, 156, 0x15, 0, 0, 0),
];
const MLOARCH: SInfo = SInfo { name: 0x10506455, key: 0x37e3a0f2, unk8: 0x400, size: 240, entries: MLOARCH_E };
const MLOARCH_E: &[(u32, i32, u8, u8, i16, u32)] = &[
    (0xbf74dfa7, 8, 0x21, 0, 0, 0),
    (0x67d26872, 12, 0x15, 0, 0, 0),
    (0x6c1523e4, 16, 0x15, 0, 0, 0),
    (0xd9ef8236, 32, 0x33, 0, 0, 0),
    (0xe78aa618, 48, 0x33, 0, 0, 0),
    (0xd61872ff, 64, 0x33, 0, 0, 0),
    (0x82e8c5de, 80, 0x21, 0, 0, 0),
    (0xad5d5b4c, 84, 0x21, 0, 0, 0),
    (0xe6e17f14, 88, 0x4a, 0, 0, 0),
    (0x75d215a1, 92, 0x4a, 0, 0, 0),
    (0x19471791, 96, 0x4a, 0, 0, 0),
    (0xfc1fb6fb, 100, 0x4a, 0, 0, 0),
    (0xd3c717fc, 104, 0x4a, 0, 0, 0),
    (0x069004b2, 108, 0x62, 0, 0, 0x76baf7c7),
    (0x9a77c7c5, 112, 0x4a, 0, 0, 0),
    (0x00000100, 0, 0x07, 0, 0, 0),
    (0xf17e7f28, 120, 0x52, 0, 15, 0),
    (0xd607de68, 144, 0x15, 0, 0, 0),
    (0x00000100, 0, 0x07, 0, 0, 0),
    (0xccab9307, 152, 0x52, 0, 18, 0),
    (0x00000100, 0, 0x05, 0, 0, 0x0b1811f1),
    (0xcd20d7a8, 168, 0x52, 0, 20, 0),
    (0x00000100, 0, 0x05, 0, 0, 0x995072ca),
    (0x89f7e992, 184, 0x52, 0, 22, 0),
    (0x00000100, 0, 0x05, 0, 0, 0xd6a799f9),
    (0x45bcb930, 200, 0x52, 0, 24, 0),
    (0x00000100, 0, 0x05, 0, 0, 0x301d99a8),
    (0xaf9c3bd9, 216, 0x52, 0, 26, 0),
];
const ROOM: SInfo = SInfo { name: 0x0b1811f1, key: 0xe796ee15, unk8: 0x400, size: 112, entries: ROOM_E };
const ROOM_E: &[(u32, i32, u8, u8, i16, u32)] = &[
    (0xe6e17f14, 8, 0x44, 0, 0, 0),
    (0xd9ef8236, 32, 0x33, 0, 0, 0),
    (0xe78aa618, 48, 0x33, 0, 0, 0),
    (0xea73a60b, 64, 0x21, 0, 0, 0),
    (0xa261e0a9, 68, 0x4a, 0, 0, 0),
    (0xc208509c, 72, 0x4a, 0, 0, 0),
    (0x67d26872, 76, 0x15, 0, 0, 0),
    (0x41e225b3, 80, 0x15, 0, 0, 0),
    (0x8264e631, 84, 0x14, 0, 0, 0),
    (0x20f3d23e, 88, 0x14, 0, 0, 0),
    (0x00000100, 0, 0x15, 0, 0, 0),
    (0x8e05312c, 96, 0x52, 0, 10, 0),
];
const PORTAL: SInfo = SInfo { name: 0x995072ca, key: 0x422ca2c9, unk8: 0x300, size: 64, entries: PORTAL_E };
const PORTAL_E: &[(u32, i32, u8, u8, i16, u32)] = &[
    (0xf470d2fd, 8, 0x15, 0, 0, 0),
    (0x9b649621, 12, 0x15, 0, 0, 0),
    (0x67d26872, 16, 0x15, 0, 0, 0),
    (0x46a92719, 20, 0x15, 0, 0, 0),
    (0xd9394f28, 24, 0x15, 0, 0, 0),
    (0x4131e934, 28, 0x15, 0, 0, 0),
    (0x00000100, 0, 0x33, 0, 0, 0),
    (0x4f42aeb3, 32, 0x52, 0, 6, 0),
    (0x00000100, 0, 0x15, 0, 0, 0),
    (0x8e05312c, 48, 0x52, 0, 8, 0),
];
const ESET: SInfo = SInfo { name: 0xd6a799f9, key: 0xf928f783, unk8: 0x300, size: 48, entries: ESET_E };
const ESET_E: &[(u32, i32, u8, u8, i16, u32)] = &[
    (0xe6e17f14, 8, 0x4a, 0, 0, 0),
    (0x00000100, 0, 0x15, 0, 0, 0),
    (0xddfc2211, 16, 0x52, 0, 1, 0),
    (0x00000100, 0, 0x07, 0, 0, 0),
    (0xccab9307, 32, 0x52, 0, 3, 0),
];
const TCM: SInfo = SInfo { name: 0x301d99a8, key: 0x32003632, unk8: 0x400, size: 48, entries: TCM_E };
const TCM_E: &[(u32, i32, u8, u8, i16, u32)] = &[
    (0xe6e17f14, 8, 0x4a, 0, 0, 0),
    (0x38da06a2, 16, 0x34, 0, 0, 0),
    (0xafb02915, 32, 0x21, 0, 0, 0),
    (0x13d14fa4, 36, 0x21, 0, 0, 0),
    (0x2543dc07, 40, 0x15, 0, 0, 0),
    (0x16af8fb2, 44, 0x15, 0, 0, 0),
];
/// CExtensionDefLightEffect / CLightAttrDef (CodeWalker MetaTypes.cs).
const LIGHTEXT: SInfo = SInfo { name: 0x27922c43, key: 0x913575d9, unk8: 0x400, size: 48, entries: LIGHTEXT_E };
const LIGHTEXT_E: &[(u32, i32, u8, u8, i16, u32)] = &[
    (0xe6e17f14, 8, 0x4a, 0, 0, 0),
    (0xd8950703, 16, 0x33, 0, 0, 0),
    (0x00000100, 0, 0x05, 0, 0, 0xf54b227b),
    (0x10579df2, 32, 0x52, 0, 2, 0),
];
const LIGHTATTR: SInfo = SInfo { name: 0xf54b227b, key: 0x8cdc7d6c, unk8: 0x300, size: 160, entries: LIGHTATTR_E };
const LIGHTATTR_E: &[(u32, i32, u8, u8, i16, u32)] = &[
    (0x00000100, 0, 0x21, 0, 0, 0),
    (0x39a625e4, 8, 0x50, 0, 0, 3),
    (0x00000100, 0, 0x11, 0, 0, 0),
    (0x5bdc13e3, 20, 0x50, 0, 2, 3),
    (0xe4447b12, 23, 0x11, 0, 0, 0),
    (0xefcd993d, 24, 0x21, 0, 0, 0),
    (0x67d26872, 28, 0x15, 0, 0, 0),
    (0x29d7f7b0, 32, 0x12, 0, 0, 0),
    (0x1cbbbe30, 34, 0x11, 0, 0, 0),
    (0x951bdd14, 35, 0x11, 0, 0, 0),
    (0x8609d52c, 36, 0x15, 0, 0, 0),
    (0xcc6806e8, 40, 0x21, 0, 0, 0),
    (0x2bb342ba, 44, 0x21, 0, 0, 0),
    (0x00000100, 0, 0x21, 0, 0, 0),
    (0x64b51e10, 48, 0x50, 0, 13, 4),
    (0xdae93379, 64, 0x11, 0, 0, 0),
    (0xbd94be3a, 65, 0x11, 0, 0, 0),
    (0x8bd6dabf, 66, 0x12, 0, 0, 0),
    (0xd1e766df, 68, 0x15, 0, 0, 0),
    (0x291d3720, 72, 0x21, 0, 0, 0),
    (0x78f8388f, 76, 0x21, 0, 0, 0),
    (0x00000100, 0, 0x11, 0, 0, 0),
    (0x8822fbce, 80, 0x50, 0, 21, 3),
    (0x26542706, 83, 0x11, 0, 0, 0),
    (0xb34d77f7, 84, 0x21, 0, 0, 0),
    (0x65a03c8b, 88, 0x21, 0, 0, 0),
    (0xa470b2e2, 92, 0x21, 0, 0, 0),
    (0x4df55f03, 96, 0x11, 0, 0, 0),
    (0x73e32c64, 97, 0x11, 0, 0, 0),
    (0xf7698288, 98, 0x11, 0, 0, 0),
    (0x7b33e620, 99, 0x11, 0, 0, 0),
    (0x38e6c28a, 100, 0x21, 0, 0, 0),
    (0x88a2b1fb, 104, 0x21, 0, 0, 0),
    (0x9639a173, 108, 0x21, 0, 0, 0),
    (0x00000100, 0, 0x21, 0, 0, 0),
    (0x34df9828, 112, 0x50, 0, 34, 3),
    (0x00000100, 0, 0x21, 0, 0, 0),
    (0x8e6f0ba9, 124, 0x50, 0, 36, 3),
    (0x455c3938, 136, 0x21, 0, 0, 0),
    (0xf8d9e344, 140, 0x21, 0, 0, 0),
    (0x00000100, 0, 0x21, 0, 0, 0),
    (0x2d3f79c0, 144, 0x50, 0, 40, 3),
    (0x402d6d92, 156, 0x15, 0, 0, 0),
];
const T_UINT: u32 = 0x15;
/// MetaTypeName.VECTOR4 (padded Vector3 arrays)
const T_VEC4: u32 = 0x33;

fn infos(mb: &mut MB, s: &[&'static SInfo], e: &[&'static EInfo]) {
    for x in e {
        mb.einfo(x);
    }
    for x in s {
        mb.sinfo(x);
    }
}

/// CEntityDef / CMloInstanceDef head (128 bytes).
fn entity(w: &mut W, arch: u32, guid: u32, pos: V3, rot: Q, sxy: f32, sz: f32, lod: f32) {
    entity_ext(w, arch, guid, pos, rot, sxy, sz, lod, (0, 0));
}

#[allow(clippy::too_many_arguments)]
fn entity_ext(w: &mut W, arch: u32, guid: u32, pos: V3, rot: Q, sxy: f32, sz: f32, lod: f32, ext: (u64, u16)) {
    w.u32(0).u32(0).u32(arch).u32(32).u32(guid).zeros(12);
    w.v3(pos).f32(0.0).v4(rot);
    w.f32(sxy).f32(sz).i32(-1).f32(lod).f32(0.0);
    w.i32(5).u32(0).i32(0); // ORPHANHD, numChildren, PRI_REQUIRED
    arr(w, ext.0, ext.1);
    w.i32(255).i32(255).u32(0).u32(0);
}

fn uints(mb: &mut MB, t: u32, v: &[u32]) -> (u64, u16) {
    if v.is_empty() {
        return (0, 0);
    }
    let b: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
    (mb.add_item(t, b), v.len() as u16)
}

fn grow(b: &mut [V3; 2], p: V3) {
    for k in 0..3 {
        b[0][k] = b[0][k].min(p[k]);
        b[1][k] = b[1][k].max(p[k]);
    }
}

/// Box of `a` (lo, hi) placed at pos with orientation `ori`.
fn placed(lo: V3, hi: V3, pos: V3, ori: Q, sc: V3, out: &mut [V3; 2]) {
    for j in 0..8 {
        let c = [if j & 4 != 0 { hi[0] } else { lo[0] } * sc[0], if j & 2 != 0 { hi[1] } else { lo[1] } * sc[1], if j & 1 != 0 { hi[2] } else { lo[2] } * sc[2]];
        let w = qrot(ori, c);
        grow(out, [w[0] + pos[0], w[1] + pos[1], w[2] + pos[2]]);
    }
}

const EMPTY: [V3; 2] = [[f32::MAX; 3], [f32::MIN; 3]];

/// Writes t6m_int.ytyp and the t6m_int_X_Y.ymap cells. `an` maps a 6m
/// archetype name to its converted name; `archetypes` are t6m_bld.ytyp's.
pub fn emit(ctx: &Ctx, ints: &[Interior], an: &HashMap<String, String>, archetypes: &[Archetype]) -> Result<(), String> {
    let stale = |prefix: &str| {
        if let Ok(rd) = fs::read_dir(&ctx.stream) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if n.starts_with(prefix) && n.ends_with(".ymap") {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
    };
    let by_an: HashMap<&str, &Archetype> = archetypes.iter().map(|a| (a.name.as_str(), a)).collect();
    let surf = surface_map(ctx);
    let mut cols = 0;
    stale_ybn(ctx);
    let name = |h: u32| format!("t6m_int_{h:08x}");
    let mut mb = MB::default();
    mb.ensure(meta::T_CMAPTYPES);
    let (mut ptrs, mut dropped, mut kept) = (Vec::new(), 0usize, Vec::new());
    let mut sets_report = Vec::new();
    let mut n_lights = 0usize;
    for it in ints {
        let mut main: Vec<Vec<u8>> = Vec::new();
        let mut sets: BTreeMap<u32, Vec<Vec<u8>>> = BTreeMap::new();
        let mut rbox: HashMap<u32, [V3; 2]> = HashMap::new();
        let mut guid = 1u32;
        let room_ix: HashMap<u32, u32> = it.rooms.iter().enumerate().map(|(i, r)| (r.0, i as u32)).collect();
        let roomed = it.portals.is_some();
        let mut att: Vec<Vec<u32>> = vec![Vec::new(); it.rooms.len()];
        let mut set_loc: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for c in &it.children {
            let Some(a) = an.get(&c.name).and_then(|n| by_an.get(n.as_str())) else {
                dropped += 1;
                continue;
            };
            let lod = if c.lod > 0.0 { c.lod } else { a.lod_dist };
            let (ext, nl) = light_ext(&mut mb, &c.lights);
            n_lights += nl;
            let mut w = W::new();
            entity_ext(&mut w, meta::hash(&a.name)?, guid, c.pos, c.rot, c.sxy, c.sz, lod, ext);
            guid += 1;
            let ori = [-c.rot[0], -c.rot[1], -c.rot[2], c.rot[3]];
            placed(a.bb_min, a.bb_max, c.pos, ori, [c.sxy, c.sxy, c.sz], rbox.entry(c.room).or_insert(EMPTY));
            let ri = if roomed { room_ix.get(&c.room).copied().unwrap_or(0) } else { 0 };
            match c.set {
                Some(s) => {
                    sets.entry(s).or_default().push(w.b);
                    set_loc.entry(s).or_default().push(ri);
                }
                None => {
                    att[ri as usize].push(main.len() as u32);
                    main.push(w.b);
                }
            }
        }
        if main.is_empty() && sets.is_empty() {
            continue;
        }
        kept.push(it);
        if it.portals.is_some() {
            if let Some(e) = &it.col {
                let order: Vec<u32> = it.rooms.iter().map(|r| r.0).collect();
                cols += collision(ctx, &ctx.entry_path(e), e, it, &order, &rbox, &surf)?;
            }
        }
        let (ep, en) = mb.ptr_array(meta::CENTITYDEF.name, main);
        let ports = it.portals.clone().unwrap_or_default();
        let mut all = EMPTY;
        for b in rbox.values() {
            grow(&mut all, b[0]);
            grow(&mut all, b[1]);
        }
        let mut rooms = Vec::new();
        for (i, (h, nm)) in it.rooms.iter().enumerate() {
            let label = nm.clone().unwrap_or_else(|| if i == 0 { "limbo".into() } else { format!("room_{h:08x}") });
            let bb = if i == 0 && !roomed { all } else { rbox.get(h).copied().filter(|b| b[0][0] <= b[1][0]).unwrap_or(it.bbox) };
            let (sp, sn) = mb.add_string(&label);
            let (ap, an_) = uints(&mut mb, T_UINT, &att[i]);
            let pc = ports.iter().filter(|p| p.from as usize == i || p.to as usize == i).count() as u32;
            let mut w = W::new();
            w.u32(0).u32(0);
            arr(&mut w, sp, sn);
            w.zeros(8).v3(bb[0]).f32(0.0).v3(bb[1]).f32(0.0);
            w.f32(1.0).u32(0).u32(0).u32(if i == 0 { 0 } else { 96 }).u32(pc).i32(0).i32(-1).u32(0);
            arr(&mut w, ap, an_);
            rooms.push(w.b);
        }
        let nrooms = rooms.len() as u16;
        let rp = mb.add_item(ROOM.name, rooms.concat());
        let mut pitems = Vec::new();
        for p in &ports {
            let cb: Vec<u8> = p.corners.iter().flat_map(|c| [c[0], c[1], c[2], 0.0]).flat_map(f32::to_le_bytes).collect();
            let cp = mb.add_item(T_VEC4, cb);
            let mut w = W::new();
            w.u32(0).u32(0).u32(p.from).u32(p.to).u32(0).u32(0).u32(0).u32(0);
            arr(&mut w, cp, 4);
            arr(&mut w, 0, 0);
            pitems.push(w.b);
        }
        let npt = pitems.len() as u16;
        let pp_ = if pitems.is_empty() { 0 } else { mb.add_item(PORTAL.name, pitems.concat()) };
        let mut set_items = Vec::new();
        for (h, ents) in sets {
            sets_report.push(json!({"interior": name(it.hash), "set": format!("{h:08x}"), "entities": ents.len()}));
            let (lp, ln) = uints(&mut mb, T_UINT, &set_loc[&h]);
            let (pp, pn) = mb.ptr_array(meta::CENTITYDEF.name, ents);
            let mut w = W::new();
            w.u32(0).u32(0).u32(h).u32(0);
            arr(&mut w, lp, ln);
            arr(&mut w, pp, pn);
            set_items.push(w.b);
        }
        let nsets = set_items.len() as u16;
        let sp = if set_items.is_empty() { 0 } else { mb.add_item(ESET.name, set_items.concat()) };
        let [lo, hi] = it.bbox;
        let c: V3 = std::array::from_fn(|k| (lo[k] + hi[k]) * 0.5);
        let r = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f32>().sqrt() * 0.5;
        let mut w = W::new();
        w.u32(0).u32(0).f32(lod_of(r)).u32(0).u32(0).zeros(12);
        w.v3(lo).f32(0.0).v3(hi).f32(0.0).v3(c).f32(0.0);
        w.f32(r.max(1.0)).f32(100.0).u32(meta::hash(&name(it.hash))?).u32(0).u32(0).u32(0).u32(0);
        w.i32(4).u32(0).u32(0); // ASSET_TYPE_ASSETLESS, assetName
        arr(&mut w, 0, 0); // extensions
        w.zeros(8).u32(0).u32(0); // mloFlags
        arr(&mut w, ep, en);
        arr(&mut w, rp, nrooms);
        arr(&mut w, pp_, npt); // portals
        arr(&mut w, sp, nsets);
        arr(&mut w, 0, 0); // timeCycleModifiers
        w.zeros(8);
        debug_assert_eq!(w.len(), 240);
        ptrs.push(mb.add_item(MLOARCH.name, w.b));
    }
    if cols > 0 {
        eprintln!("[t6m:interiors] {cols} collision ybns with room ids");
    }
    if n_lights > 0 {
        eprintln!("[t6m:interiors] {n_lights} MLO lights");
    }
    if dropped > 0 {
        ctx.skip(&format!("({dropped} interior entities)"), "interior entity archetype not converted", json!({"count": dropped}));
    }
    if !sets_report.is_empty() {
        ctx.skip(&format!("({} entity sets)", sets_report.len()), "interior entity sets written but off by default (ActivateInteriorEntitySet)", json!({"sets": sets_report}));
    }
    if kept.is_empty() {
        let _ = fs::remove_file(ctx.stream.join("t6m_int.ytyp"));
        stale("t6m_int_");
        return Ok(());
    }
    let n = ptrs.len() as u16;
    let pb: Vec<u8> = ptrs.iter().flat_map(|p| p.to_le_bytes()).collect();
    let ap = mb.add_item(meta::T_POINTER, pb);
    infos(&mut mb, &[&meta::CENTITYDEF, &ROOM, &PORTAL, &ESET, &TCM, &MLOARCH, &LIGHTEXT, &LIGHTATTR, &meta::CMAPTYPES], &[&meta::E_LODTYPE, &meta::E_PRIORITY, &meta::E_ASSETTYPE]);
    let mut w = W::new();
    w.u32(0).u32(0);
    arr(&mut w, 0, 0);
    arr(&mut w, ap, n);
    w.u32(meta::hash("t6m_int")?).u32(0);
    arr(&mut w, 0, 0);
    arr(&mut w, 0, 0);
    mb.blocks[0].1.extend_from_slice(&w.b);
    emit_meta(ctx, "t6m_int.ytyp", mb.build()?)?;

    // instances in 512 m cells
    let mut cells: BTreeMap<(i64, i64), Vec<(&Interior, V3, Q)>> = BTreeMap::new();
    for it in &kept {
        for &(p, q) in &it.instances {
            cells.entry(((p[0] / 512.0).floor() as i64, (p[1] / 512.0).floor() as i64)).or_default().push((it, p, q));
        }
    }
    stale("t6m_int_");
    for ((x, y), list) in cells {
        let file = format!("t6m_int_{x}_{y}");
        let mut mb = MB::default();
        mb.ensure(meta::T_CMAPDATA);
        let (mut ee, mut se) = (EMPTY, EMPTY);
        let mut ps = Vec::new();
        for (k, (it, p, q)) in list.iter().enumerate() {
            let [lo, hi] = it.bbox;
            let r = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f32>().sqrt() * 0.5;
            let lod = lod_of(r);
            let mut w = W::new();
            entity(&mut w, meta::hash(&name(it.hash))?, k as u32 + 1, *p, *q, 1.0, 1.0, lod);
            w.u32(0).u32(0); // groupId, floorId
            arr(&mut w, 0, 0); // defaultEntitySets
            w.u32(0).u32(0); // numExitPortals, MLOInstflags
            ps.push(mb.add_item(MLOINST.name, w.b));
            placed(lo, hi, *p, *q, [1.0; 3], &mut ee);
            placed(lo.map(|v| v - lod), hi.map(|v| v + lod), *p, *q, [1.0; 3], &mut se);
        }
        let n = ps.len() as u16;
        let ep = mb.add_item(meta::T_POINTER, ps.iter().flat_map(|p| p.to_le_bytes()).collect());
        let (np, nn) = mb.add_string(&file);
        let (xp, xn) = mb.add_string("6ixpack fivem");
        infos(&mut mb, &[&MLOINST, &meta::INSTANCED, &meta::LODLIGHT, &meta::DISTLODLIGHT, &meta::BLOCKDESC, &meta::CMAPDATA], &[&meta::E_LODTYPE, &meta::E_PRIORITY]);
        let mut w = W::new();
        w.u32(0).u32(0).u32(meta::hash(&file)?).u32(0).u32(0).u32(9).u32(0).u32(0);
        w.v3(se[0]).f32(0.0).v3(se[1]).f32(0.0).v3(ee[0]).f32(0.0).v3(ee[1]).f32(0.0);
        arr(&mut w, ep, n);
        for _ in 0..4 {
            arr(&mut w, 0, 0);
        }
        w.zeros(48);
        arr(&mut w, 0, 0);
        arr(&mut w, 0, 0);
        w.zeros(136);
        w.zeros(48);
        w.u32(0).u32(0);
        arr(&mut w, np, nn);
        arr(&mut w, xp, xn);
        arr(&mut w, 0, 0);
        arr(&mut w, 0, 0);
        debug_assert_eq!(w.len(), 512);
        mb.blocks[0].1.extend_from_slice(&w.b);
        emit_meta(ctx, &format!("{file}.ymap"), mb.build()?)?;
    }
    Ok(())
}

/// MLO lodDist from the interior's bounding radius (60..200 m): the
/// interior's contents only stream (and fill the building pool) when the
/// player is inside or near, as vanilla interiors do.
fn lod_of(r: f32) -> f32 {
    (r * 2.0 + 50.0).clamp(60.0, 200.0)
}

/// 6m surface id -> GTA V material (as collision.rs; DEFAULT when unknown).
fn surface_map(ctx: &Ctx) -> HashMap<u16, u8> {
    let p = ctx.o.work.join("collision").join("levels").join("surface_categories.json");
    let doc = fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    doc["surfaces"]
        .as_object()
        .map(|m| m.iter().filter_map(|(k, v)| Some((u16::from_str_radix(k, 16).ok()?, super::super::collision::gta_material(v["category"].as_str()?)))).collect())
        .unwrap_or_default()
}

fn stale_ybn(ctx: &Ctx) {
    if let Ok(rd) = fs::read_dir(&ctx.stream) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("t6m_intcol_") && n.ends_with(".ybn") {
                let _ = fs::remove_file(e.path());
            }
        }
    }
}

/// Writes one world-space ybn per instance of `it` from its interior bounds
/// `e`; polygons get the index (in `order`) of the smallest room whose box
/// (`rbox`, grown 0.5 m) holds their centroid, else 0 (limbo). Returns the
/// number of files written.
fn collision(ctx: &Ctx, levels: &Path, e: &Entry, it: &Interior, order: &[u32], rbox: &HashMap<u32, [V3; 2]>, surf: &HashMap<u16, u8>) -> Result<usize, String> {
    let Some(c) = sixng_core::bounds::load(levels, e).ok().and_then(|r| sixng_core::bounds::decode_movement(&r)) else {
        ctx.skip(&format!("t6m_int_{:08x}", it.hash), "interior collision does not decode", json!({"bounds": e.id}));
        return Ok(0);
    };
    let rooms: Vec<(u8, [V3; 2], f32)> = order
        .iter()
        .enumerate()
        .skip(1)
        .filter_map(|(i, h)| {
            let b = rbox.get(h)?;
            let b = [b[0].map(|v| v - 0.5), b[1].map(|v| v + 0.5)];
            let vol = (0..3).map(|k| (b[1][k] - b[0][k]).max(0.0)).product::<f32>();
            (i < 32 && vol > 0.0).then_some((i as u8, b, vol))
        })
        .collect();
    let (mut verts, mut tris, mut words) = (Vec::new(), Vec::new(), Vec::new());
    let mut inside = 0usize;
    for (id, (v, idx)) in &c.by_mat {
        let base = verts.len() as u32;
        verts.extend_from_slice(v);
        let mat = surf.get(id).copied().unwrap_or(0);
        for t in idx.chunks_exact(3) {
            let ctr: V3 = std::array::from_fn(|k| (v[t[0] as usize][k] + v[t[1] as usize][k] + v[t[2] as usize][k]) / 3.0);
            let room = rooms
                .iter()
                .filter(|(_, b, _)| (0..3).all(|k| ctr[k] >= b[0][k] && ctr[k] <= b[1][k]))
                .min_by(|a, b| a.2.total_cmp(&b.2))
                .map_or(0, |r| r.0);
            inside += (room != 0) as usize;
            tris.push([base + t[0], base + t[1], base + t[2]]);
            words.push(crate::gta5::ybn::material_word(mat, room));
        }
    }
    if tris.is_empty() {
        return Ok(0);
    }
    let mut n = 0;
    for (k, (p, q)) in it.instances.iter().enumerate() {
        let wv: Vec<V3> = verts.iter().map(|v| {
            let d = qrot(*q, *v);
            [p[0] + d[0], p[1] + d[1], p[2] + d[2]]
        }).collect();
        let file = format!("t6m_intcol_{:08x}_{k}.ybn", it.hash);
        let bytes = crate::gta5::ybn::build_ybn_words(&wv, &tris, &words).map_err(|e| format!("{file}: {e}"))?;
        let fp = format!("intcol|{}|{:?}|{:?}", e.id, p, q);
        let s = ctx.emit(&file, &fp, json!({"bounds": e.id, "triangles": tris.len(), "in_rooms": inside}), Some(bytes))?;
        ctx.report.lock().unwrap().converted.push(json!({"kind": "interior collision", "ybn": s, "triangles": tris.len(), "in_rooms": inside}));
        n += 1;
    }
    Ok(n)
}

type LightMaps = (HashMap<u32, Vec<Value>>, HashMap<(String, usize), Vec<Value>>);

/// The 6m light sets `ints`' children use, decoded straight from the
/// interiors' own meta files (they hold the entity lights and the light
/// extensions of the archetypes defined beside them; archetypes defined in
/// other files keep no lights): by archetype hash, and by (meta file
/// offset, entity index). Each light gets its set's offset added to its position.
fn light_sets(_ctx: &Ctx, index: &ArchiveIndex, ints: &[Interior], levels: &Path) -> LightMaps {
    let (mut by_arch, mut by_ent): LightMaps = Default::default();
    if ints.is_empty() || std::env::var_os("T6M_NO_LIGHTS").is_some() {
        return (by_arch, by_ent);
    }
    let kids = || ints.iter().flat_map(|i| i.children.iter());
    let want: std::collections::HashSet<u32> = kids().map(|c| crate::gta5::rsc::joaat(&c.name)).collect();
    let files: std::collections::BTreeSet<String> = kids().map(|c| c.src.0.clone()).collect();
    let by_off: HashMap<u64, &Entry> = index.entries.iter().map(|e| (e.off, e)).collect();
    for f in files {
        let Some(e) = u64::from_str_radix(&f, 16).ok().and_then(|o| by_off.get(&o)) else { continue };
        let Ok(r) = Resource::load_sys(levels, e) else { continue };
        for s in sixng_core::lights::light_sets(&r) {
            let off = s.ext_offset;
            let lights: Vec<Value> = s
                .lights
                .iter()
                .filter_map(|l| {
                    let mut v = serde_json::to_value(l).ok()?;
                    let p = l.position;
                    v["position"] = json!([p[0] + off[0], p[1] + off[1], p[2] + off[2]]);
                    Some(v)
                })
                .collect();
            match (s.owner, s.entity_index) {
                ("archetype", _) if want.contains(&s.archetype) => {
                    by_arch.entry(s.archetype).or_insert(lights);
                }
                ("entity", Some(i)) => by_ent.entry((f.clone(), i)).or_default().extend(lights),
                _ => {}
            }
        }
    }
    (by_arch, by_ent)
}

/// One CExtensionDefLightEffect holding `lights` as CLightAttrDefs (6m
/// light type 1/2/4 = GTA point/spot/capsule; others dropped). Returns
/// the entity's extensions array and the light count.
fn light_ext(mb: &mut MB, lights: &[Value]) -> ((u64, u16), usize) {
    let rgb = |v: &Value| {
        let x = v.as_str().and_then(|s| u32::from_str_radix(s, 16).ok()).unwrap_or(0xffffff);
        [(x >> 16) as u8, (x >> 8) as u8, x as u8]
    };
    // same CLightAttr field as GTA V, copied 1:1 (no known unit change);
    // T6M_LIGHT_SCALE tunes it
    let scale = std::env::var("T6M_LIGHT_SCALE").ok().and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0);
    let mut items = Vec::new();
    for l in lights {
        let ty = l["light_type"].as_u64().unwrap_or(0) as u8;
        if !matches!(ty, 1 | 2 | 4) {
            continue;
        }
        let fl = |k: &str, h: &str, d: f32| l[k].as_f64().or_else(|| l["fields"][h].as_f64()).map(|x| x as f32).unwrap_or(d);
        let byte = |k: &str| l[k].as_u64().unwrap_or(0) as u8;
        let (c, oc) = (rgb(&l["colour"]), rgb(&l["colour2"]));
        let time = l["time_flags"].as_str().and_then(|s| u32::from_str_radix(s, 16).ok()).unwrap_or(0xffffff);
        let mut w = W::new();
        w.zeros(8).v3(f3(&l["position"]));
        w.u8(c[0]).u8(c[1]).u8(c[2]).u8(byte("flashiness"));
        w.f32(fl("intensity", "efcd993d", 1.0) * scale).u32(0).u16(0).u8(ty).u8(0).u32(time);
        w.f32(fl("range", "cc6806e8", 5.0)).f32(fl("falloff_exponent", "2bb342ba", 8.0));
        w.zeros(16); // cullingPlane
        w.u8(0).u8(0).u16(0).u32(0); // shadowBlur, padding
        w.f32(fl("vol_intensity", "291d3720", 0.0)).f32(fl("", "78f8388f", 1.0));
        w.u8(oc[0]).u8(oc[1]).u8(oc[2]).u8(0);
        w.f32(fl("", "b34d77f7", 1.0)).f32(fl("corona_size", "65a03c8b", 0.0)).f32(fl("", "a470b2e2", 1.0));
        w.u8(byte("light_fade_distance")).u8(byte("shadow_fade_distance")).u8(0).u8(0);
        w.f32(fl("", "38e6c28a", 0.01)).f32(fl("corona_intensity", "88a2b1fb", 0.0)).f32(fl("", "9639a173", 0.1));
        w.v3(f3(&l["direction"])).v3(f3(&l["tangent"]));
        w.f32(fl("cone_inner", "455c3938", 5.0)).f32(fl("cone_outer", "f8d9e344", 60.0));
        w.v3(f3(&l["extents"])).u32(0); // extents (capsule length in x), projectedTextureKey
        debug_assert_eq!(w.len(), 160);
        items.push(w.b);
    }
    if items.is_empty() {
        return ((0, 0), 0);
    }
    let n = items.len();
    let lp = mb.add_item(LIGHTATTR.name, items.concat());
    let mut w = W::new();
    w.u32(0).u32(0).u32(0).u32(0).v3([0.0; 3]).f32(0.0);
    arr(&mut w, lp, n as u16);
    (mb.ptr_array(LIGHTEXT.name, vec![w.b]), n)
}

// ---------------------------------------------------------------------------
// One interior as plain data, outside `fivem build` (`redm interior`). Reads
// the same sixng outputs as `load` (<work>/placements/levels/interiors.jsonl
// and the interior file's placement JSON) through the same helpers, plus the
// meta object (portals, room bounds) and the linked bounds (collision).

/// One 6m interior archetype: everything a converter needs, interior space.
#[derive(serde::Serialize)]
pub struct InteriorSource {
    /// joaat of the 6m interior archetype name (the name itself is not recovered)
    #[serde(serialize_with = "hex8")]
    pub hash: u32,
    /// `int_<hash>`: the name used for outputs
    pub name: String,
    /// sixng id of the interior archetype file (holds the interior's entities)
    pub file: String,
    /// archetype bbox, interior space
    pub bbox: [V3; 2],
    /// world placements (CMloInstanceDef)
    pub placements: Vec<IntPlacement>,
    /// rooms in file order (index 0 is limbo in the 6m files seen)
    pub rooms: Vec<IntRoom>,
    /// decoded portals; `portals_complete` false when some did not decode
    /// (then `fivem build` keeps every entity in limbo)
    pub portals: Vec<Portal>,
    pub portals_complete: bool,
    /// the interior's entities, then its entity-set entities (instance order)
    pub entities: Vec<IntEntity>,
    pub entity_sets: Vec<IntSet>,
    /// linked collision in interior space; None when no bounds are linked
    /// (or the linked bounds do not decode: see `collision_note`)
    #[serde(skip)]
    pub collision: Option<IntCollision>,
    pub collision_note: Option<String>,
}

fn hex8<S: serde::Serializer>(h: &u32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("{h:08x}"))
}

#[derive(serde::Serialize, Clone)]
pub struct IntPlacement {
    /// sixng id of the map file holding the CMloInstanceDef
    pub map: String,
    /// instance index in that file
    pub instance: usize,
    pub position: V3,
    /// x, y, z, w as stored in CMloInstanceDef: the interior's world
    /// orientation itself (not inverted, unlike entities; sixng meta7a,
    /// checked there by fitting interior collision into its shell)
    pub rotation: Q,
}

#[derive(serde::Serialize, Clone)]
pub struct IntRoom {
    #[serde(serialize_with = "hex8")]
    pub hash: u32,
    pub name: Option<String>,
    /// room bbMin / bbMax from the 6m meta object (interior space); None
    /// when the room record has none
    pub bounds: Option<[V3; 2]>,
    pub flags: Option<u32>,
    /// indices into `entities`
    pub entities: Vec<usize>,
}

#[derive(serde::Serialize, Clone)]
pub struct IntEntity {
    /// index in the interior file's entity list
    pub file_index: usize,
    #[serde(serialize_with = "hex8")]
    pub archetype: u32,
    /// model name, when sixng resolved the archetype hash
    pub name: Option<String>,
    /// sixng model id ("<archive>@<offset>[#member]")
    pub model_id: Option<String>,
    /// interior space
    pub position: V3,
    /// x, y, z, w as stored in CEntityDef: the INVERSE of the entity's
    /// interior-space orientation (GTA V convention; `fivem build` writes
    /// it into MLO CEntityDefs as is and uses the conjugate for bounds)
    pub rotation: Q,
    pub scale_xy: f32,
    pub scale_z: f32,
    /// -1 = the archetype's own lodDist
    pub lod_dist: f32,
    pub flags: u32,
    /// tint palette row (`3c852527`, 0 = default)
    pub tint_row: u32,
    /// index into `rooms`; None = not in a room of this interior
    pub room: Option<usize>,
    /// index into `entity_sets`
    pub entity_set: Option<usize>,
}

#[derive(serde::Serialize, Clone)]
pub struct IntSet {
    #[serde(serialize_with = "hex8")]
    pub hash: u32,
    pub name: Option<String>,
    /// indices into `entities`
    pub entities: Vec<usize>,
}

/// Linked interior bounds, interior space (sixng bounds decode_native,
/// weapon-only shapes skipped as `fivem build` does).
pub struct IntCollision {
    pub id: String,
    /// "exact" (bbox match) or sixng's link method; with its confidence
    pub method: String,
    pub confidence: String,
    pub vertices: Vec<V3>,
    pub triangles: Vec<[u32; 3]>,
    /// 6m surface id per triangle
    pub materials: Vec<u16>,
    /// boxes, spheres, capsules, cylinders as stored (each with its surface id)
    pub primitives: Vec<sixng_core::bounds::NativePrim>,
    /// every shape as triangles (primitives tessellated by sixng-core's
    /// bounds decoder): movement shapes (weapon-only children skipped, as
    /// `fivem build`), and all shapes including weapon-only ones
    pub mover: Tris,
    pub all: Tris,
    /// 8-byte 6m material record per surface id
    pub material_records: BTreeMap<u16, [u8; 8]>,
    pub bbox: [V3; 2],
}

/// Interior archetype hash from `int_<hash>`, a bare 8-digit hex hash, or
/// a name (its joaat).
pub fn interior_hash(s: &str) -> Option<u32> {
    let t = s.trim().to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    let h = t.strip_prefix("int_").unwrap_or(&t);
    if h.len() == 8 && h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return u32::from_str_radix(h, 16).ok();
    }
    Some(crate::gta5::rsc::joaat(&t))
}

/// Output name of an interior archetype.
pub fn interior_name(h: u32) -> String {
    format!("int_{h:08x}")
}

fn placement_dir(work: &Path) -> std::path::PathBuf {
    work.join("placements").join("levels")
}

/// The sixng interior instances (<work>/placements/levels/interiors.jsonl),
/// or an error naming the passes that make it.
fn instance_lines(work: &Path) -> Result<Vec<Value>, String> {
    let w = work.display();
    let p = placement_dir(work).join("interiors.jsonl");
    let col = work.join("collision").join("levels").join("index.jsonl");
    let text = fs::read_to_string(&p).map_err(|e| {
        format!("{}: {e}\n  missing pass `sixng interiors`: run `6ixpack sixng --game <dir> --work {w} collision levels --out {w}` (bounds index for the collision link), then `6ixpack sixng --game <dir> --work {w} interiors levels --out {w}`", p.display())
    })?;
    if !col.is_file() {
        return Err(format!("{}: missing pass `sixng collision`: interiors.jsonl was made without it, so no interior has linked collision. Run `6ixpack sixng --game <dir> --work {w} collision levels --out {w}`, then `sixng interiors` again", col.display()));
    }
    Ok(text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
}

/// Placement JSON of an interior file (`levels.rpf@<off>`), or an error
/// naming the `sixng placements` run that writes it.
fn interior_file(work: &Path, file: &str) -> Result<Value, String> {
    let off = file.strip_prefix("levels.rpf@").ok_or_else(|| format!("{file}: interior file outside levels.rpf (only levels.rpf is read)"))?;
    let p = placement_dir(work).join(format!("{off}.json"));
    let b = fs::read(&p).map_err(|e| {
        let w = work.display();
        format!("{}: {e}\n  missing pass `sixng placements` for this file: run `6ixpack sixng --game <dir> --work {w} placements levels --id {file} --out {w}` (or --id-file with every interior_file of interiors.jsonl)", p.display())
    })?;
    serde_json::from_slice(&b).map_err(|e| format!("{}: {e}", p.display()))
}

/// One line of `redm interior --list`.
pub struct IntSummary {
    pub hash: u32,
    pub placements: Vec<V3>,
    pub rooms: usize,
    /// None when not decoded (no game dir given, or the meta did not parse)
    pub portals: Option<(usize, bool)>,
    pub entities: usize,
    pub set_entities: usize,
    /// entities (incl. entity-set entities) whose model can be loaded
    pub entities_resolvable: usize,
    /// distinct entity archetypes, and how many of them have a model id in
    /// an indexed archive (`resolvable`)
    pub archetypes: usize,
    pub resolvable: usize,
    /// collision bounds id and how it was linked ("exact", or sixng's method
    /// and confidence; "... (not used)" for a low-confidence link); None =
    /// no link at all
    pub collision: Option<(String, String)>,
    /// why the interior's entity data is missing (empty interior file, ...)
    pub problem: Option<String>,
}

fn link_of(inst: &Value) -> Option<(String, String)> {
    let id = collision_id(inst)?;
    let exact = inst["collision"].as_array().and_then(|a| a.first()).and_then(Value::as_str) == Some(id);
    let l = &inst["collision_linked"];
    Some((id.to_string(), if exact { "exact".into() } else { format!("{} {}", l["method"].as_str().unwrap_or("?"), l["confidence"].as_str().unwrap_or("?")) }))
}

/// levels.rpf and the pack archives (PACK_ARCHIVES) indexed in `work`.
pub fn indexed_archives(work: &Path) -> Vec<String> {
    let ix = work.join("index");
    std::iter::once("levels.rpf".to_string()).chain(super::super::PACK_ARCHIVES.iter().filter(|id| sixng_core::scan::index_path(&ix, id).is_file()).map(|id| id.to_string())).collect()
}

/// The sixng model id of a placement entity when its model is in one of
/// `archives` (Err: why not). This is `fivem build`'s route: sixng puts a
/// model id on every placement whose archetype hash is the joaat of an
/// indexed drawable / fragment / drawable-dictionary member name, or that
/// `sixng link-archetypes` linked; the build loads the model by that id.
pub fn resolvable(model_id: Option<&str>, archives: &[String]) -> Result<String, String> {
    let Some(m) = model_id else { return Err("no model id: the archetype hash names no drawable of an indexed archive and has no archetype link".into()) };
    let a = m.rsplit_once('@').map_or("", |x| x.0);
    if !archives.iter().any(|x| x == a) {
        return Err(format!("model in {a}, which is not indexed"));
    }
    Ok(m.to_string())
}

/// Every interior archetype of interiors.jsonl (`pattern`: substring of
/// its `int_<hash>` name), one summary each. `game`: also decode portals.
pub fn interior_list(work: &Path, game: Option<&Path>, pattern: &str) -> Result<Vec<IntSummary>, String> {
    let lines = instance_lines(work)?;
    let ip = work.join("index").join("levels.json");
    let index = sixng_core::scan::load_index(&ip).map_err(|e| format!("{}: {e:#}", ip.display()))?;
    let archives = indexed_archives(work);
    let mut by_hash: BTreeMap<u32, IntSummary> = BTreeMap::new();
    let mut files: HashMap<String, Result<Value, String>> = HashMap::new();
    let mut metas: HashMap<String, HashMap<u32, MloMeta>> = HashMap::new();
    for inst in &lines {
        let Some(h) = hx(&inst["archetype"]) else { continue };
        if !interior_name(h).contains(pattern) {
            continue;
        }
        let pos = f3(&inst["position"]);
        if let Some(s) = by_hash.get_mut(&h) {
            s.placements.push(pos);
            continue;
        }
        let l = &inst["collision_linked"];
        // a low-confidence link is shown but, as in `fivem build`, not used
        let low = l["id"].as_str().map(|id| (id.to_string(), format!("{} {} (not used)", l["method"].as_str().unwrap_or("?"), l["confidence"].as_str().unwrap_or("?"))));
        let mut s = IntSummary { hash: h, placements: vec![pos], rooms: 0, portals: None, entities: 0, set_entities: 0, entities_resolvable: 0, archetypes: 0, resolvable: 0, collision: link_of(inst).or(low), problem: None };
        match inst["interior_file"].as_str() {
            None => s.problem = Some("interior archetype file not found by `sixng interiors`".into()),
            // `sixng placements` writes no JSON for a file without entities
            Some(_) if inst_entities(inst).next().is_none() => s.problem = Some("no entities".into()),
            Some(file) => match files.entry(file.to_string()).or_insert_with(|| interior_file(work, file)) {
                Err(e) => s.problem = Some(e.lines().next().unwrap_or("").to_string()),
                Ok(src) => {
                    let ents = src["entities"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                    s.rooms = rooms_of(src, h).len();
                    s.entities = inst["entities"].as_array().map_or(0, Vec::len);
                    s.set_entities = inst["entity_set_entities"].as_array().map_or(0, Vec::len);
                    let mut arch: BTreeMap<u64, bool> = BTreeMap::new();
                    for e in inst_entities(inst).filter_map(|i| ents.get(i)) {
                        let ok = resolvable(raw_ent(e).model_id, &archives).is_ok();
                        s.entities_resolvable += ok as usize;
                        arch.entry(e["archetype"].as_u64().unwrap_or(0)).or_insert(ok);
                    }
                    s.archetypes = arch.len();
                    s.resolvable = arch.values().filter(|x| **x).count();
                    if let (Some(g), Some(off)) = (game, file.strip_prefix("levels.rpf@").and_then(|o| u64::from_str_radix(o, 16).ok())) {
                        let m = metas.entry(file.to_string()).or_insert_with(|| file_mlos(&g.join("levels.rpf"), &index, off));
                        s.portals = m.get(&h).map(|m| m.portals.as_ref().map_or((0, false), |p| (p.len(), true)));
                    }
                }
            },
        }
        by_hash.insert(h, s);
    }
    Ok(by_hash.into_values().collect())
}

/// Interior `want` (`int_<hash>`, hash or name) as plain data. Needs
/// `sixng collision`, `sixng interiors` and `sixng placements` (for the
/// interior's file) in `work`; errors name the missing pass.
pub fn source(game: &Path, work: &Path, want: &str) -> Result<InteriorSource, String> {
    let h = interior_hash(want).ok_or_else(|| format!("{want}: not an interior name"))?;
    let name = interior_name(h);
    let lines = instance_lines(work)?;
    let insts: Vec<&Value> = lines.iter().filter(|v| hx(&v["archetype"]) == Some(h)).collect();
    if insts.is_empty() {
        return Err(format!("{want}: no interior instance with archetype {h:08x} in interiors.jsonl (`redm interior --list` shows them)"));
    }
    let inst = insts.iter().copied().find(|v| v["interior_file"].is_string()).ok_or_else(|| format!("{name}: interior archetype file not found by `sixng interiors` (archetype in another archive?)"))?;
    let file = inst["interior_file"].as_str().unwrap_or_default().to_string();
    let src = interior_file(work, &file)?;
    let ip = work.join("index").join("levels.json");
    let index = sixng_core::scan::load_index(&ip).map_err(|e| format!("{}: {e:#} (run `6ixpack sixng index levels --game .. --work ..` first)", ip.display()))?;
    let levels = game.join("levels.rpf");
    let off = file.strip_prefix("levels.rpf@").and_then(|o| u64::from_str_radix(o, 16).ok()).unwrap_or(0);
    let meta = file_mlos(&levels, &index, off).remove(&h);
    if meta.is_none() {
        return Err(format!("{name}: interior archetype not found in the meta of {file} ({})", levels.display()));
    }
    let meta = meta.unwrap();
    let room_list = rooms_of(&src, h);
    let mut rooms: Vec<IntRoom> = room_list
        .iter()
        .enumerate()
        .map(|(i, (rh, rn))| {
            let m = meta.rooms.get(i).copied().unwrap_or_default();
            IntRoom { hash: *rh, name: rn.clone(), bounds: m.bounds, flags: m.flags, entities: Vec::new() }
        })
        .collect();
    let info = src["interiors"].as_array().into_iter().flatten().find(|v| hx(&v["interior"]) == Some(h));
    let mut sets: Vec<IntSet> =
        info.and_then(|v| v["entity_sets"].as_array()).into_iter().flatten().filter_map(|s| Some(IntSet { hash: hx(&s["entity_set"])?, name: s["name"].as_str().map(str::to_string), entities: Vec::new() })).collect();
    let ents = src["entities"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let mut entities = Vec::new();
    for fi in inst_entities(inst) {
        let Some(e) = ents.get(fi) else { continue };
        let r = raw_ent(e);
        let k = entities.len();
        let room = r.room.and_then(|rh| rooms.iter().position(|x| x.hash == rh));
        if let Some(i) = room {
            rooms[i].entities.push(k);
        }
        let entity_set = r.set.map(|sh| match sets.iter().position(|x| x.hash == sh) {
            Some(i) => i,
            None => {
                sets.push(IntSet { hash: sh, name: e["interior"]["entity_set_name"].as_str().map(str::to_string), entities: Vec::new() });
                sets.len() - 1
            }
        });
        if let Some(i) = entity_set {
            sets[i].entities.push(k);
        }
        entities.push(IntEntity {
            file_index: fi,
            archetype: e["archetype"].as_u64().unwrap_or(0) as u32,
            name: r.name.map(str::to_string),
            model_id: r.model_id.map(str::to_string),
            position: r.pos,
            rotation: r.rot,
            scale_xy: r.sxy,
            scale_z: r.sz,
            lod_dist: r.lod,
            flags: e["flags"].as_u64().unwrap_or(0) as u32,
            tint_row: e["tint_row"].as_u64().unwrap_or(0) as u32,
            room,
            entity_set,
        });
    }
    let placements = insts
        .iter()
        .map(|v| IntPlacement {
            map: v["map"].as_str().unwrap_or_default().to_string(),
            instance: v["instance"].as_u64().unwrap_or(0) as usize,
            position: f3(&v["position"]),
            rotation: q4(&v["rotation_world"]).unwrap_or([0.0, 0.0, 0.0, 1.0]),
        })
        .collect();
    let (collision, collision_note) = match link_of(inst) {
        None => (None, Some(format!("no linked collision (sixng link: {})", inst["collision_linked"]["confidence"].as_str().unwrap_or("none")))),
        Some((id, how)) => match int_collision(&levels, &index, &id) {
            Ok(mut c) => {
                let mut w = how.split(' ');
                c.method = w.next().unwrap_or("").to_string();
                c.confidence = w.next().unwrap_or("high").to_string();
                (Some(c), None)
            }
            Err(e) => (None, Some(format!("{id}: {e}"))),
        },
    };
    Ok(InteriorSource {
        hash: h,
        name,
        file,
        bbox: serde_json::from_value(inst["bbox"].clone()).unwrap_or([[-1.0; 3], [1.0; 3]]),
        placements,
        rooms,
        portals_complete: meta.portals.is_some(),
        portals: meta.portals.unwrap_or_default(),
        entities,
        entity_sets: sets,
        collision,
        collision_note,
    })
}

/// Triangles of a decoded bounds: vertices, triangles, 6m surface id per triangle.
#[derive(Default, Clone)]
pub struct Tris {
    pub vertices: Vec<V3>,
    pub triangles: Vec<[u32; 3]>,
    pub materials: Vec<u16>,
}

fn tris_of(c: &sixng_core::bounds::Collision) -> Tris {
    let mut t = Tris::default();
    for (id, (v, idx)) in &c.by_mat {
        let base = t.vertices.len() as u32;
        t.vertices.extend_from_slice(v);
        for x in idx.chunks_exact(3) {
            t.triangles.push([base + x[0], base + x[1], base + x[2]]);
            t.materials.push(*id);
        }
    }
    t
}

/// Decode bounds `id` of the levels index (interior space as stored).
fn int_collision(levels: &Path, index: &ArchiveIndex, id: &str) -> Result<IntCollision, String> {
    let e = index.entries.iter().find(|e| e.id == id).ok_or("bounds not in the levels index")?;
    let r = sixng_core::bounds::load(levels, e).map_err(|e| e.to_string())?;
    let c = sixng_core::bounds::decode_native(&r, true).ok_or("bounds do not decode")?;
    let mover = sixng_core::bounds::decode_movement(&r).map(|c| tris_of(&c)).unwrap_or_default();
    let all = sixng_core::bounds::decode(&r).map(|c| tris_of(&c)).unwrap_or_default();
    let Tris { vertices, triangles, materials } = tris_of(&c);
    if triangles.is_empty() && c.native.is_empty() {
        return Err("bounds hold no triangles or primitives".into());
    }
    Ok(IntCollision { id: id.to_string(), method: String::new(), confidence: String::new(), vertices, triangles, materials, primitives: c.native, mover, all, material_records: c.mats, bbox: c.bbox })
}
