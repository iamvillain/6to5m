//! RDR2 `.ymap` (CMapData, RSC8 version 3 meta) writer: plain props
//! (`CEntityDef`) or one interior placement (`CMloInstanceDef`), through
//! the generic meta writer. Values follow the MLO format spec (vanilla milo
//! ymaps and the user's working RedM interiors): parent 0, flags 0,
//! contentFlags 73 for a milo ymap (HD entities + interior instance +
//! physicsDictionaries), 1 for plain props; entities orphan HD.

use super::meta::{self, Meta, Obj};
use super::meta_defs as d;
use super::ytyp::{entity_obj, MloEntity};

const NAME: u32 = 0xE6E1_7F14;
const FLAGS: u32 = 0x67D2_6872;
const CONTENT_FLAGS: u32 = 0x6A67_5035;
const STREAMING_MIN: u32 = 0xDD22_821F;
const STREAMING_MAX: u32 = 0xA22E_A335;
const ENTITIES_MIN: u32 = 0x1C75_BCF1;
const ENTITIES_MAX: u32 = 0x6D07_4437;
const ENTITIES: u32 = 0xCCAB_9307;
const PHYSICS_DICTIONARIES: u32 = 0x3899_9564;
const NUM_EXIT_PORTALS: u32 = 0x1F83_7FB7;
const MLO_INST_FLAGS: u32 = 0xE03B_0CAA;
/// contentFlags: 1 HD entities, 8 interior instance, 64 physicsDictionaries.
pub const CONTENT_PROPS: u32 = 1;
pub const CONTENT_MILO: u32 = 1 | 8 | 64;
/// CEntityDef flags of a CMloInstanceDef (vanilla 10/10, user's interiors).
pub const INSTANCE_FLAGS: u32 = 0x18_0008;

/// One interior placement.
#[derive(Debug, Clone, PartialEq)]
pub struct MloInstance {
    /// the CMloArchetypeDef name
    pub archetype: String,
    pub position: [f32; 3],
    /// x, y, z, w: the interior's REAL world orientation (not conjugated)
    pub rotation: [f32; 4],
    pub lod_dist: f32,
    pub num_exit_portals: u32,
    pub id: u64,
}

/// (min, max) extents of a ymap.
pub type Extents = ([f32; 3], [f32; 3]);

fn root(name: &str, content: u32, streaming: Extents, entities: Extents) -> Obj {
    let mut o = Obj::new(&d::CMAPDATA);
    o.hash(NAME, &name.to_ascii_lowercase()).u32(FLAGS, 0).u32(CONTENT_FLAGS, content);
    o.f3(STREAMING_MIN, streaming.0, 0.0).f3(STREAMING_MAX, streaming.1, 0.0).f3(ENTITIES_MIN, entities.0, 0.0).f3(ENTITIES_MAX, entities.1, 0.0);
    o
}

/// `<name>.ymap` holding plain prop entities (world space; `rotation` =
/// conjugate of the real orientation, as in a ytyp's interior entities).
pub fn build_props(name: &str, ents: &[MloEntity], streaming: Extents, entities: Extents) -> Result<Vec<u8>, String> {
    if ents.is_empty() {
        return Err("ymap: no entities".into());
    }
    let mut m = Meta::default();
    let at = m.reserve(&d::CMAPDATA, 1);
    let ptrs: Vec<u64> = ents.iter().map(|e| m.object(&entity_obj(&d::CENTITYDEF, e))).collect();
    let list = m.ptrs(&ptrs);
    let mut r = root(name, CONTENT_PROPS, streaming, entities);
    r.arr(ENTITIES, list);
    m.set(at, &r.b);
    m.build(1)
}

/// `<name>_milo_.ymap`: one CMloInstanceDef (orphan HD, parentIndex -1,
/// groupId none, MLOInstflags 0) and physicsDictionaries = [interior].
pub fn build_milo(name: &str, inst: &MloInstance, streaming: Extents, entities: Extents) -> Result<Vec<u8>, String> {
    let mut m = Meta::default();
    let at = m.reserve(&d::CMAPDATA, 1);
    let e = MloEntity { archetype: inst.archetype.clone(), position: inst.position, rotation: inst.rotation, scale_xy: 1.0, scale_z: 1.0, lod_dist: inst.lod_dist, flags: INSTANCE_FLAGS, id: inst.id };
    let mut o = entity_obj(&d::CMLOINSTANCEDEF, &e);
    o.u32(NUM_EXIT_PORTALS, inst.num_exit_portals).u32(MLO_INST_FLAGS, 0);
    let p = m.object(&o);
    let list = m.ptrs(&[p]);
    let phys = m.hashes(&[meta::name_hash(&inst.archetype)]);
    let mut r = root(name, CONTENT_MILO, streaming, entities);
    r.arr(ENTITIES, list).arr(PHYSICS_DICTIONARIES, phys);
    m.set(at, &r.b);
    m.build(1)
}
