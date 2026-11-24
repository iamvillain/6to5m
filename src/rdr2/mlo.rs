//! 6m interior data -> RDR2 MLO pieces: the CMloArchetypeDef (entities of
//! converted archetypes only, room / portal / entity-set indices remapped),
//! the placement, the milo ymap extents and the collision children.

use super::{ybn, ytyp};

/// One 6m interior entity, as `fivem::InteriorSource` holds it.
#[derive(Debug, Clone)]
pub struct InEntity {
    pub archetype: u32,
    pub position: [f32; 3],
    /// as stored in the 6m CEntityDef: the inverse of the local orientation
    pub rotation: [f32; 4],
    pub scale_xy: f32,
    pub scale_z: f32,
    pub lod_dist: f32,
    pub room: Option<usize>,
    pub set: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct InRoom {
    pub hash: u32,
    pub name: Option<String>,
    pub bounds: Option<[[f32; 3]; 2]>,
}

#[derive(Debug, Clone)]
pub struct InPortal {
    pub from: u32,
    pub to: u32,
    pub corners: [[f32; 3]; 4],
}

/// The 6m interior, reduced to what the MLO needs.
#[derive(Debug, Clone)]
pub struct Input {
    pub name: String,
    pub hash: u32,
    pub bbox: [[f32; 3]; 2],
    pub entities: Vec<InEntity>,
    pub rooms: Vec<InRoom>,
    pub portals: Vec<InPortal>,
    pub portals_complete: bool,
    /// (name hash, name)
    pub sets: Vec<(u32, Option<String>)>,
}

/// What `assemble` changed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Stats {
    /// entities whose archetype was not converted (left out)
    pub dropped: usize,
    /// entities without a room of this interior, put in limbo
    pub to_limbo: usize,
    /// portals whose corner order was reversed so the normal points away from roomFrom
    pub flipped: usize,
    /// portals did not decode: everything in limbo, no portals
    pub all_limbo: bool,
    /// entity sets left empty (not written)
    pub empty_sets: usize,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| a[k] - b[k])
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Quaternion product (x, y, z, w).
pub fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

pub fn conj(q: [f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}

/// Rotate `v` by `q`.
pub fn qrot(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let r = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), conj(q));
    [r[0], r[1], r[2]]
}

/// Quaternion of a yaw (degrees, counter-clockwise seen from +Z).
pub fn yaw(deg: f32) -> [f32; 4] {
    let h = deg.to_radians() * 0.5;
    [0.0, 0.0, h.sin(), h.cos()]
}

/// The MLO archetype. `out_name(archetype hash)` is the converted model
/// name, None when that archetype was not converted (its entities are left
/// out and every index list is remapped). Entity rotations are copied as
/// stored: 6m CEntityDefs store the inverse of the orientation, the same
/// convention RDR2 MLO entities use. Portal corners are reordered when
/// their normal points into roomFrom (judged by roomFrom's entity centroid,
/// else its box centre). Undecoded portals: every entity in limbo and no
/// portals, as `fivem build` does.
pub fn assemble(x: &Input, out_name: &dyn Fn(u32) -> Option<String>, id_base: u64) -> (ytyp::Mlo, Stats) {
    let mut st = Stats { all_limbo: !x.portals_complete, ..Default::default() };
    let roomed = x.portals_complete && !x.rooms.is_empty();
    let mut rooms: Vec<ytyp::MloRoom> = if x.rooms.is_empty() {
        vec![ytyp::MloRoom { name: "limbo".into(), bb_min: x.bbox[0], bb_max: x.bbox[1], timecycle: 0, flags: 32, attached: Vec::new() }]
    } else {
        x.rooms
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let name = if i == 0 { "limbo".to_string() } else { r.name.clone().unwrap_or_else(|| format!("room_{:08x}", r.hash)) };
                let [lo, hi] = r.bounds.unwrap_or(x.bbox);
                ytyp::MloRoom { name, bb_min: lo, bb_max: hi, timecycle: 0, flags: 32, attached: Vec::new() }
            })
            .collect()
    };
    let mut sets: Vec<ytyp::MloSet> = x.sets.iter().map(|(h, n)| ytyp::MloSet { name: n.clone().unwrap_or_else(|| format!("set_{h:08x}")), entities: Vec::new(), locations: Vec::new() }).collect();
    let mut entities = Vec::new();
    let mut id = id_base;
    for e in &x.entities {
        let Some(name) = out_name(e.archetype) else {
            st.dropped += 1;
            continue;
        };
        id += 1;
        let me = ytyp::MloEntity { archetype: name, position: e.position, rotation: e.rotation, scale_xy: e.scale_xy, scale_z: e.scale_z, lod_dist: e.lod_dist, flags: ytyp::INTERIOR_ENTITY_FLAGS, id };
        let room = if roomed { e.room.filter(|&r| r < rooms.len()) } else { None };
        if room.is_none() && roomed {
            st.to_limbo += 1;
        }
        let ri = room.unwrap_or(0) as u32;
        match e.set.filter(|&s| s < sets.len()) {
            Some(s) => {
                sets[s].entities.push(me);
                sets[s].locations.push(ri);
            }
            None => {
                rooms[ri as usize].attached.push(entities.len() as u32);
                entities.push(me);
            }
        }
    }
    let portals = if !roomed {
        Vec::new()
    } else {
        x.portals
            .iter()
            .map(|p| {
                let c = p.corners;
                let n = cross(sub(c[1], c[0]), sub(c[2], c[1]));
                let inside: Vec<[f32; 3]> = x.entities.iter().filter(|e| e.room == Some(p.from as usize) && e.set.is_none()).map(|e| e.position).collect();
                let probe = if inside.is_empty() {
                    let [lo, hi] = x.rooms.get(p.from as usize).and_then(|r| r.bounds).unwrap_or(x.bbox);
                    std::array::from_fn(|k| (lo[k] + hi[k]) * 0.5)
                } else {
                    std::array::from_fn(|k| inside.iter().map(|v| v[k]).sum::<f32>() / inside.len() as f32)
                };
                // roomFrom's side must be -n
                let corners = if dot(n, sub(probe, c[0])) > 0.0 {
                    st.flipped += 1;
                    [c[3], c[2], c[1], c[0]]
                } else {
                    c
                };
                ytyp::MloPortal { from: p.from, to: p.to, flags: if p.to == 0 { 0 } else { 64 }, corners, attached: Vec::new() }
            })
            .collect()
    };
    st.empty_sets = sets.iter().filter(|s| s.entities.is_empty()).count();
    sets.retain(|s| !s.entities.is_empty());
    (ytyp::Mlo { name: x.name.clone(), lod_dist: 150.0, mlo_flags: 0, entities, rooms, portals, sets }, st)
}

/// World extents of the MLO's room boxes placed at `pos` / `rot` (real
/// orientation); streaming extents = those +- max(lod, 250) m.
pub fn extents(m: &ytyp::Mlo, pos: [f32; 3], rot: [f32; 4], lod: f32) -> (([f32; 3], [f32; 3]), ([f32; 3], [f32; 3])) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for r in &m.rooms {
        for j in 0..8 {
            let c = [if j & 4 != 0 { r.bb_max[0] } else { r.bb_min[0] }, if j & 2 != 0 { r.bb_max[1] } else { r.bb_min[1] }, if j & 1 != 0 { r.bb_max[2] } else { r.bb_min[2] }];
            let w = qrot(rot, c);
            for k in 0..3 {
                lo[k] = lo[k].min(w[k] + pos[k]);
                hi[k] = hi[k].max(w[k] + pos[k]);
            }
        }
    }
    let s = lod.max(250.0);
    ((lo.map(|v| v - s), hi.map(|v| v + s)), (lo, hi))
}

/// RDR2 materials.meta name for an inferred 6m surface category (`sixng
/// surfaces`), DEFAULT when unknown.
pub fn material_name(category: &str) -> &'static str {
    let c = category.to_ascii_lowercase();
    let has = |k: &str| c.contains(k);
    if has("generic") {
        "DEFAULT"
    } else if has("ballast") || has("gravel") {
        "GRAVEL_SMALL"
    } else if has("dirt track") {
        "DIRT_TRACK"
    } else if has("road") {
        "TARMAC"
    } else if has("seabed") || has("underwater") {
        "MUD_UNDERWATER"
    } else if has("shore") {
        "SAND_WET"
    } else if has("sand") {
        "SAND_COMPACT"
    } else if has("wetland") || has("swamp") {
        "MARSH"
    } else if has("forest") {
        "SOIL"
    } else if has("grass") || has("vegetated") {
        "GRASS"
    } else if has("rock") {
        "ROCK"
    } else if has("paved") || has("hard") || has("wall") || has("structure") {
        "CONCRETE"
    } else {
        "DEFAULT"
    }
}

/// Collision children from interior-space triangles: per triangle the
/// material of its 6m surface id (`surface(id)` -> materials.meta name) and
/// the room (index < 32) of the smallest room box (grown 0.5 m) holding
/// its centroid, else 0. Returns the children and the (material word,
/// triangles) census.
pub fn collision(verts: &[[f32; 3]], tris: &[[u32; 3]], surf: &[u16], rooms: &[ytyp::MloRoom], surface: &dyn Fn(u16) -> &'static str, type_flags: u64, include_flags: u64) -> Result<(Vec<ybn::Geometry>, Vec<(u32, usize)>), String> {
    let boxes: Vec<(u8, [[f32; 3]; 2], f32)> = rooms
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(i, _)| *i < 32)
        .map(|(i, r)| {
            let b = [r.bb_min.map(|v| v - 0.5), r.bb_max.map(|v| v + 0.5)];
            (i as u8, b, (0..3).map(|k| (b[1][k] - b[0][k]).max(0.0)).product::<f32>())
        })
        .collect();
    let mut mats: Vec<(u32, u32)> = Vec::new();
    let mut census: Vec<(u32, usize)> = Vec::new();
    let mut tri_mat = Vec::with_capacity(tris.len());
    for (t, &sid) in tris.iter().zip(surf) {
        let ctr: [f32; 3] = std::array::from_fn(|k| (verts[t[0] as usize][k] + verts[t[1] as usize][k] + verts[t[2] as usize][k]) / 3.0);
        let room = boxes.iter().filter(|(_, b, _)| (0..3).all(|k| ctr[k] >= b[0][k] && ctr[k] <= b[1][k])).min_by(|a, b| a.2.total_cmp(&b.2)).map_or(0, |r| r.0);
        let w = ybn::material_word(ybn::material_id(surface(sid)), room);
        let i = match mats.iter().position(|m| m.0 == w) {
            Some(i) => i,
            None => {
                if mats.len() == 255 {
                    return Err("more than 255 collision materials".into());
                }
                mats.push((w, 0));
                census.push((w, 0));
                mats.len() - 1
            }
        };
        census[i].1 += 1;
        tri_mat.push(i as u8);
    }
    Ok((ybn::Geometry::from_triangles(verts, tris, &tri_mat, &mats, type_flags, include_flags), census))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(arch: u32, x: f32, room: Option<usize>, set: Option<usize>) -> InEntity {
        InEntity { archetype: arch, position: [x, 0.0, 0.0], rotation: [0.0, 0.0, 0.0, 1.0], scale_xy: 1.0, scale_z: 1.0, lod_dist: -1.0, room, set }
    }

    fn input() -> Input {
        Input {
            name: "int_t".into(),
            hash: 1,
            bbox: [[-5.0; 3], [5.0; 3]],
            // 1 and 3 are not converted; 4 has no room; 5 is in a set
            entities: vec![e(10, 0.0, Some(1), None), e(11, 1.0, Some(1), None), e(10, 2.0, Some(2), None), e(11, 3.0, Some(2), None), e(12, 4.0, None, None), e(10, -1.0, Some(1), Some(0))],
            rooms: vec![
                InRoom { hash: 0, name: Some("limbo".into()), bounds: Some([[-5.0; 3], [5.0; 3]]) },
                InRoom { hash: 2, name: None, bounds: Some([[-2.0, -2.0, -2.0], [1.5, 2.0, 2.0]]) },
                InRoom { hash: 3, name: None, bounds: Some([[1.5, -2.0, -2.0], [5.0, 2.0, 2.0]]) },
            ],
            // exit from room 1 at x = -2 with its normal pointing +x (into room 1): flipped
            portals: vec![InPortal { from: 1, to: 0, corners: [[-2.0, -0.5, 2.0], [-2.0, -0.5, 0.0], [-2.0, 0.5, 0.0], [-2.0, 0.5, 2.0]] }, InPortal { from: 1, to: 2, corners: [[1.5, -0.5, 2.0], [1.5, -0.5, 0.0], [1.5, 0.5, 0.0], [1.5, 0.5, 2.0]] }],
            portals_complete: true,
            sets: vec![(7, Some("set_a".into())), (8, None)],
        }
    }

    fn names(h: u32) -> Option<String> {
        (h != 11).then(|| format!("m{h}"))
    }

    #[test]
    fn remaps_rooms_portals_sets() {
        let (m, st) = assemble(&input(), &names, 0x4000_0000_0000_0000);
        m.check().unwrap();
        assert_eq!((st.dropped, st.to_limbo, st.empty_sets, st.all_limbo), (2, 1, 1, false));
        assert_eq!(m.entities.iter().map(|e| e.position[0]).collect::<Vec<_>>(), vec![0.0, 2.0, 4.0]);
        assert_eq!(m.rooms.iter().map(|r| r.attached.clone()).collect::<Vec<_>>(), vec![vec![2], vec![0], vec![1]]);
        assert_eq!((m.rooms[0].name.as_str(), m.rooms[1].name.as_str()), ("limbo", "room_00000002"));
        assert_eq!(m.sets.len(), 1);
        assert_eq!((m.sets[0].name.as_str(), m.sets[0].locations.clone()), ("set_a", vec![1]));
        assert_eq!(m.portal_counts(), vec![1, 2, 1]);
        assert_eq!(m.exit_portals(), 1);
        // normals point away from roomFrom (room 1 entity centroid x 0.0 / -1)
        for p in &m.portals {
            let c = p.corners;
            let n = cross(sub(c[1], c[0]), sub(c[2], c[1]));
            assert!(dot(n, sub([0.0; 3], c[0])) < 0.0);
        }
        assert_eq!(st.flipped, 1);
        assert_eq!(m.entities.iter().map(|e| e.id).collect::<Vec<_>>(), vec![0x4000_0000_0000_0001, 0x4000_0000_0000_0002, 0x4000_0000_0000_0003]);
    }

    #[test]
    fn undecoded_portals_put_everything_in_limbo() {
        let mut x = input();
        x.portals_complete = false;
        let (m, st) = assemble(&x, &names, 0);
        m.check().unwrap();
        assert!(st.all_limbo && m.portals.is_empty());
        assert_eq!(m.rooms[0].attached, vec![0, 1, 2]);
        assert!(m.rooms[1..].iter().all(|r| r.attached.is_empty()));
    }

    #[test]
    fn quaternions_and_extents() {
        let q = yaw(90.0);
        let v = qrot(q, [1.0, 0.0, 0.0]);
        assert!((v[0]).abs() < 1e-6 && (v[1] - 1.0).abs() < 1e-6);
        let (m, _) = assemble(&input(), &names, 0);
        let (s, e) = extents(&m, [100.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], 150.0);
        assert_eq!((e.0, e.1), ([95.0, -5.0, -5.0], [105.0, 5.0, 5.0]));
        assert_eq!(s.0, [-155.0, -255.0, -255.0]);
        assert_eq!(material_name("walls / vertical building surface"), "CONCRETE");
        assert_eq!(material_name("generic / default surface"), "DEFAULT");
    }

    #[test]
    fn collision_rooms_and_materials() {
        let (m, _) = assemble(&input(), &names, 0);
        let v = [[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0], [3.0, -1.0, 0.0], [4.0, -1.0, 0.0], [3.5, 1.0, 0.0], [9.0, 9.0, 9.0], [9.5, 9.0, 9.0], [9.0, 9.5, 9.0]];
        let t = [[0, 1, 2], [3, 4, 5], [6, 7, 8]];
        let (g, census) = collision(&v, &t, &[1, 1, 2], &m.rooms, &|id| if id == 1 { "CONCRETE" } else { "" }, ybn::TYPE_MOVER_WEAPON, ybn::INCLUDE_MOVER_WEAPON).unwrap();
        assert_eq!(g.len(), 1);
        let c = ybn::material_id("CONCRETE") as u32;
        assert_eq!(census, vec![(c | 1 << 24, 1), (c | 2 << 24, 1), (12667, 1)]);
        assert_eq!(g[0].tri_mat, vec![0, 1, 2]);
    }
}
