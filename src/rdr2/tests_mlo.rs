//! MLO / ymap / ybn writer tests without game data. The files equivalent to
//! the MLO spec's goldens (hand-written XML built by CodeX) are rebuilt
//! here from the same values and checked structurally. With
//! `R6M_MLO_SPEC=<spec dir>` set (the folder holding `golden\` and
//! `ybn\golden\`), they are also compared with the golden binaries: data
//! blocks of the meta files, and vertices / polygons / BVH nodes / BVH
//! header / quantum / centre of the ybn geometry.

use super::{meta, meta_defs as d, read, rsc8::Rsc8, ybn, ymap, ytyp};
use crate::gta5::rsc::joaat;

fn ent(arch: &str, id: u64, pos: [f32; 3], rot: [f32; 4], lod: f32, flags: u32) -> ytyp::MloEntity {
    ytyp::MloEntity { archetype: arch.into(), position: pos, rotation: rot, scale_xy: 1.0, scale_z: 1.0, lod_dist: lod, flags, id }
}

const S2: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// The golden `r6m_mlo` interior: limbo + 2 rooms, exit portal 1->0, inner
/// portal 1->2, 3 entities, 1 entity set.
fn golden_mlo() -> ytyp::Mlo {
    let f = 1_572_864;
    ytyp::Mlo {
        name: "r6m_mlo".into(),
        lod_dist: 25.0,
        mlo_flags: 0,
        entities: vec![
            ent("r6m_mlo_prop", 0x4000_0000_0000_0011, [0.0; 3], [0.0, 0.0, 0.0, 1.0], 25.0, f),
            ent("r6m_mlo_prop", 0x4000_0000_0000_0012, [4.0, 0.0, 0.0], [0.0, 0.0, -S2, S2], 25.0, f),
            ent("r6m_mlo_shell", 0x4000_0000_0000_0013, [0.0; 3], [0.0, 0.0, 0.0, 1.0], 25.0, f),
        ],
        rooms: vec![
            ytyp::MloRoom { name: "limbo".into(), bb_min: [-2.5, -2.5, -0.25], bb_max: [6.5, 2.5, 3.25], timecycle: 0, flags: 32, attached: vec![2] },
            ytyp::MloRoom { name: "room_r6m_a".into(), bb_min: [-2.0, -2.0, 0.0], bb_max: [2.0, 2.0, 3.0], timecycle: joaat("r6m_mlo"), flags: 32, attached: vec![0] },
            ytyp::MloRoom { name: "room_r6m_b".into(), bb_min: [2.0, -2.0, 0.0], bb_max: [6.0, 2.0, 3.0], timecycle: 0, flags: 32, attached: vec![1] },
        ],
        portals: vec![
            ytyp::MloPortal { from: 1, to: 0, flags: 0, corners: [[-2.0, 0.5, 2.1], [-2.0, 0.5, 0.0], [-2.0, -0.5, 0.0], [-2.0, -0.5, 2.1]], attached: vec![] },
            ytyp::MloPortal { from: 1, to: 2, flags: 64, corners: [[2.0, -0.5, 2.1], [2.0, -0.5, 0.0], [2.0, 0.5, 0.0], [2.0, 0.5, 2.1]], attached: vec![] },
        ],
        sets: vec![ytyp::MloSet { name: "r6m_mlo_set_a".into(), entities: vec![ent("r6m_mlo_prop", 0x4000_0000_0000_0014, [-1.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0], 25.0, f)], locations: vec![1] }],
    }
}

fn golden_archetypes() -> Vec<ytyp::Archetype> {
    let mut a = ytyp::Archetype::from_box("r6m_mlo_shell", 100.0, [-2.5, -2.5, -0.25], [6.5, 2.5, 3.25]);
    a.texture_dictionary = "r6m_mlo".into();
    a.hd_texture_dist = 30.0;
    a.bs_radius = 5.1;
    let mut b = ytyp::Archetype::from_box("r6m_mlo_prop", 100.0, [-0.5, -0.5, 0.0], [0.5, 0.5, 1.0]);
    b.texture_dictionary = "r6m_mlo".into();
    vec![a, b]
}

/// (type, bytes) of every data block of a meta file.
fn data_blocks(bytes: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let m = read::meta_file(bytes).unwrap();
    m.blocks.iter().map(|&(t, l, o)| (t, m.r.virt[o..o + l as usize].to_vec())).collect()
}

fn spec_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("R6M_MLO_SPEC").map(Into::into)
}

/// Golden binary's data blocks, compared block by block with `mine`
/// except the block types in `skip`; returns the differing block types.
fn compare_golden(mine: &[u8], golden: &str, skip: &[u32]) -> Option<Vec<(u32, usize)>> {
    let p = spec_dir()?.join("golden").join(golden);
    let g = data_blocks(&std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())));
    let m = data_blocks(mine);
    assert_eq!(g.iter().map(|b| (b.0, b.1.len())).collect::<Vec<_>>(), m.iter().map(|b| (b.0, b.1.len())).collect::<Vec<_>>(), "{golden}: block types / sizes");
    // (block type, first differing byte)
    Some(g.iter().zip(&m).filter(|(a, b)| !skip.contains(&a.0) && a.1 != b.1).map(|(a, b)| (a.0, a.1.iter().zip(&b.1).position(|(x, y)| x != y).unwrap_or(0))).collect())
}

#[test]
fn meta_pointer_maths() {
    assert_eq!(meta::meta_ptr(5, 56), 0x0380_0005);
    assert_eq!(meta::meta_ptr(10, 64), 0x0400_000A);
    let mut m = meta::Meta::default();
    // packed below 0x20 (u32 arrays), 8-aligned from 0x20 (hash arrays)
    assert_eq!(m.u32s(&[1]).0, meta::meta_ptr(1, 0));
    assert_eq!(m.u32s(&[2, 3]).0, meta::meta_ptr(1, 4));
    assert_eq!(m.hashes(&[1]).0, meta::meta_ptr(2, 0));
    assert_eq!(m.hashes(&[2]).0, meta::meta_ptr(2, 8));
    assert_eq!(m.u32s(&[]), (0, 0));
    // a block is never filled past 0xFFFFF bytes: the next one starts
    let big = vec![0u8; 0xF_FFF0];
    m.array(0x21, &big, 1);
    let p = m.array(0x21, &[0; 32], 8).0;
    assert_eq!(p & 0xFFFF, 4);
    // the class table follows first use, with inline struct classes
    m.use_class(&d::CMLOARCHETYPEDEF);
    m.use_class(&d::CENTITYDEF);
    let b = {
        let mut x = meta::Meta::default();
        x.use_class(&d::CMAPTYPES);
        x.use_class(&d::CMLOARCHETYPEDEF);
        let at = x.reserve(&d::CMAPTYPES, 1);
        x.set(at, &[0; 8]);
        x.build(1).unwrap()
    };
    let i = read::meta_file(&b).unwrap();
    let names: Vec<u32> = i.classes.iter().map(|c| c.name).collect();
    assert_eq!(names, vec![d::CMAPTYPES.name, d::CMLOARCHETYPEDEF.name, d::C_FB36133A.name, d::RAGE_SPDSPHERE.name]);
}

#[test]
fn mlo_ytyp_golden_equivalent() {
    let x = golden_mlo();
    assert_eq!(x.portal_counts(), vec![1, 2, 1]);
    assert_eq!(x.exit_portals(), 1);
    let b = ytyp::build_ytyp_mlo("r6m_mlo", &golden_archetypes(), Some(&x)).unwrap();
    let i = read::ytyp(&b).unwrap();
    // block graph as the golden (CodeX's depth-first rule)
    let types: Vec<u32> = i.blocks.iter().map(|b| b.0).collect();
    assert_eq!(types, vec![d::CMAPTYPES.name, d::CBASEARCHETYPEDEF.name, d::CMLOARCHETYPEDEF.name, d::CENTITYDEF.name, 7, d::CMLOROOMDEF.name, 0x10, 0x15, d::CMLOPORTALDEF.name, 0x33, d::CMLOENTITYSET.name]);
    let lens: Vec<u32> = i.blocks.iter().map(|b| b.1).collect();
    assert_eq!(lens, vec![96, 320, 352, 576, 56, 384, 25, 16, 160, 128, 48]);
    let classes: Vec<u32> = i.classes.iter().map(|c| c.name).collect();
    let want = [&d::CMAPTYPES, &d::CBASEARCHETYPEDEF, &d::CMLOARCHETYPEDEF, &d::C_FB36133A, &d::RAGE_SPDSPHERE, &d::CENTITYDEF, &d::CMLOROOMDEF, &d::CMLOPORTALDEF, &d::CMLOENTITYSET];
    assert_eq!(classes, want.iter().map(|c| c.name).collect::<Vec<_>>());
    let enums: Vec<u32> = i.enums.iter().map(|e| e.0).collect();
    assert_eq!(enums, vec![d::E_RAGE_FWARCHETYPEDEF_EASSETTYPE.name, d::E_MAPENTITYTYPE.name, d::E_RAGE_ELODTYPE.name, d::E_RAGE_EPRIORITYLEVEL.name]);
    assert_eq!(i.archetypes.len(), 3);
    let m = &i.mlos[0];
    assert_eq!((m.name, m.physics_dictionary, m.entities), (joaat("r6m_mlo"), joaat("r6m_mlo"), 3));
    assert_eq!(m.rooms.iter().map(|r| (r.0.as_str(), r.4, r.5.clone())).collect::<Vec<_>>(), vec![("limbo", 1, vec![2]), ("room_r6m_a", 2, vec![0]), ("room_r6m_b", 1, vec![1])]);
    assert_eq!(m.portals.iter().map(|p| (p.0, p.1, p.2)).collect::<Vec<_>>(), vec![(1, 0, 0), (1, 2, 64)]);
    assert_eq!(m.sets, vec![(joaat("r6m_mlo_set_a"), 1, vec![1])]);
    // golden: every data block identical except the prop archetypes, whose
    // values are this crate's static-prop ones (flags, bbMin.w = x)
    if let Some(diff) = compare_golden(&b, "r6m_mlo.ytyp", &[d::CBASEARCHETYPEDEF.name]) {
        assert!(diff.is_empty(), "data blocks differ from the golden: {diff:08X?}");
    }
}

#[test]
fn mlo_index_rules() {
    let mut x = golden_mlo();
    x.check().unwrap();
    x.rooms[1].attached.push(2);
    assert!(x.check().unwrap_err().contains("entity 2"));
    let mut x = golden_mlo();
    x.rooms[0].name = "outside".into();
    assert!(x.check().is_err());
    let mut x = golden_mlo();
    x.sets[0].locations.clear();
    assert!(x.check().is_err());
    let mut x = golden_mlo();
    x.portals[0].to = 3;
    assert!(x.check().is_err());
}

#[test]
fn props_ymap_golden_equivalent() {
    let es = vec![
        ent("r6m_prop_a", 0x4000_0000_0000_0001, [0.0; 3], [0.0, 0.0, 0.0, 1.0], 100.0, 1_572_864),
        ent("r6m_prop_b", 0x4000_0000_0000_0002, [5.0, 3.0, 1.0], [0.0, 0.0, -S2, S2], 60.0, 1_572_866),
    ];
    let b = ymap::build_props("r6m_props", &es, ([-100.0; 3], [100.0; 3]), ([-1.0, -1.0, 0.0], [6.0, 4.0, 2.0])).unwrap();
    let i = read::ymap(&b).unwrap();
    assert_eq!((i.name, i.flags, i.content_flags, i.entities.len(), i.blocks), (joaat("r6m_props"), 0, 1, 2, 3));
    assert_eq!(i.entities[1].position, [5.0, 3.0, 1.0]);
    assert_eq!(i.classes, vec![d::CMAPDATA.name, d::RAGE_FWINSTANCEDMAPDATA.name, d::CLODLIGHT.name, d::CDISTANTLODLIGHT.name, d::CBLOCKDESC.name, d::CENTITYDEF.name]);
    if let Some(diff) = compare_golden(&b, "r6m_props.ymap", &[]) {
        assert!(diff.is_empty(), "data blocks differ from the golden: {diff:08X?}");
    }
}

#[test]
fn milo_ymap_golden_equivalent() {
    let inst = ymap::MloInstance { archetype: "r6m_mlo".into(), position: [100.0, 200.0, 50.0], rotation: [0.0, 0.0, 0.382_683_43, 0.923_879_5], lod_dist: 150.0, num_exit_portals: 1, id: 0x4000_0000_0000_0021 };
    let b = ymap::build_milo("r6m_mlo_milo_", &inst, ([-150.0, -50.0, -200.0], [350.0, 450.0, 300.0]), ([96.0, 196.0, 49.75], [104.0, 204.0, 53.25])).unwrap();
    let i = read::ymap(&b).unwrap();
    assert_eq!((i.content_flags, i.physics_dictionaries.clone()), (73, vec![joaat("r6m_mlo")]));
    let e = &i.entities[0];
    assert_eq!((e.class, e.archetype, e.position, e.num_exit_portals, e.flags), (d::CMLOINSTANCEDEF.name, joaat("r6m_mlo"), [100.0, 200.0, 50.0], Some(1), 1_572_872));
    let enums: Vec<u32> = read::meta_file(&b).unwrap().enums.iter().map(|e| e.0).collect();
    assert_eq!(enums, vec![d::E_RAGE_ELODTYPE.name, d::E_RAGE_EPRIORITYLEVEL.name, d::E_EINTERIORGROUPID.name]);
    if let Some(diff) = compare_golden(&b, "r6m_mlo_milo_.ymap", &[]) {
        assert!(diff.is_empty(), "data blocks differ from the golden: {diff:08X?}");
    }
}

#[test]
fn material_ids() {
    for (n, id) in [("", 12667), ("DEFAULT", 12667), ("CONCRETE", 3117), ("ROCK", 29771), ("GRAVEL_LARGE", 5937), ("WOOD_SOLID_MEDIUM", 32350), ("MARBLE", 10369), ("CARPET_SOLID", 28734), ("TARMAC", 17263)] {
        assert_eq!(ybn::material_id(n), id, "{n}");
    }
    assert_eq!(ybn::material_word(3117, 1), 0x0100_0C2D);
}

/// The floor golden's geometry child: 4 x 4 x 3 m room, 10 triangles.
fn floor() -> ybn::Geometry {
    let v = [[-2.0, -2.0, -1.5], [2.0, -2.0, -1.5], [2.0, 2.0, -1.5], [-2.0, 2.0, -1.5], [-2.0, -2.0, 1.5], [2.0, -2.0, 1.5], [2.0, 2.0, 1.5], [-2.0, 2.0, 1.5]];
    let t = [[0, 1, 2], [0, 2, 3], [0, 5, 1], [0, 4, 5], [1, 6, 2], [1, 5, 6], [2, 7, 3], [2, 6, 7], [3, 4, 0], [3, 7, 4]];
    ybn::Geometry {
        box_min: [-2.005, -2.005, -0.005],
        box_max: [2.005, 2.005, 3.005],
        margin: 0.005,
        verts: v.to_vec(),
        tris: t.to_vec(),
        tri_mat: vec![0; 10],
        materials: vec![(ybn::material_word(ybn::material_id("CONCRETE"), 1), 0)],
        type_flags: ybn::TYPE_MOVER_WEAPON,
        include_flags: ybn::INCLUDE_MOVER_WEAPON,
    }
}

#[test]
fn floor_ybn_and_bvh() {
    let g = floor();
    let d1 = ybn::derive(&g).unwrap();
    // golden: polygon order 0 1 2 3 | 2 7 3, 2 6 7 | ..., 7 nodes, root inner
    assert_eq!(d1.nodes.len(), 7);
    assert_eq!(&d1.polys[4].0, &[2, 7, 3]);
    assert_eq!(d1.nodes[0][6..], [7, 0]);
    assert_eq!(d1.verts[0], [-32685, -32685, -32658]);
    assert_eq!(d1.polys[0].1, [2, 6, 1]);
    // deterministic
    let d2 = ybn::derive(&g).unwrap();
    assert_eq!((d1.nodes.clone(), d1.polys.clone(), d1.verts.clone()), (d2.nodes, d2.polys, d2.verts));
    let b = ybn::build_ybn(&[g]).unwrap();
    let i = ybn::read(&b).unwrap();
    assert_eq!(i.children.len(), 1);
    assert_eq!((i.children[0].3, i.children[0].4, i.children[0].6), (8, 10, 7));
    assert_eq!(i.children[0].5, vec![(0x0100_0C2D, 0)]);
    if let Some(dir) = spec_dir() {
        for name in ["r6m_floor", "r6m_grid"] {
            let xml = std::fs::read_to_string(dir.join("ybn/golden/inputs").join(format!("{name}.ybn.xml"))).unwrap();
            let raw = std::fs::read(dir.join("ybn/golden").join(format!("{name}.ybn"))).unwrap();
            compare_geometry(name, &geometry_from_xml(&xml), &raw);
        }
    }
}

/// The first GeometryBVH of a golden input XML (box, margin, relative
/// vertices, `Tri <mat> a b c` polygons).
fn geometry_from_xml(xml: &str) -> ybn::Geometry {
    let g = &xml[xml.find("type=\"GeometryBVH\"").unwrap()..];
    let attr = |tag: &str| -> [f32; 3] {
        let s = &g[g.find(&format!("<{tag} ")).unwrap()..];
        std::array::from_fn(|k| {
            let a = ["x=\"", "y=\"", "z=\""][k];
            let v = &s[s.find(a).unwrap() + 3..];
            v[..v.find('"').unwrap()].parse().unwrap()
        })
    };
    let sect = |tag: &str| &g[g.find(&format!("<{tag}>")).unwrap() + tag.len() + 2..g.find(&format!("</{tag}>")).unwrap()];
    let verts = sect("Vertices").lines().map(str::trim).filter(|l| !l.is_empty()).map(|l| {
        let v: Vec<f32> = l.split(',').map(|x| x.trim().parse().unwrap()).collect();
        [v[0], v[1], v[2]]
    }).collect();
    let (mut tris, mut mats) = (Vec::new(), Vec::new());
    for l in sect("Polygons").lines().map(str::trim).filter(|l| !l.is_empty()) {
        let w: Vec<u32> = l.split_whitespace().skip(1).map(|x| x.parse().unwrap()).collect();
        mats.push(w[0] as u8);
        tris.push([w[1], w[2], w[3]]);
    }
    let m = &g[g.find("<Margin value=\"").unwrap() + 15..];
    let nmat = sect("Materials").matches("<Item>").count();
    ybn::Geometry { box_min: attr("BoxMin"), box_max: attr("BoxMax"), margin: m[..m.find('"').unwrap()].parse().unwrap(), verts, tris, tri_mat: mats, materials: vec![(0, 0); nmat], type_flags: 0x3E, include_flags: 0x7F3_BEC0 }
}

/// Byte comparison with a golden: vertices, polygons (incl. area word and
/// edges), BVH nodes and header, quantum and centre.
fn compare_geometry(name: &str, g: &ybn::Geometry, golden: &[u8]) {
    let r = Rsc8::parse(golden).unwrap();
    let off = |p: u64| (p & 0x0FFF_FFFF) as usize;
    let n = r.u16(154) as usize;
    let co = (0..n).map(|k| off(r.u64(off(r.u64(112)) + 8 * k))).find(|&o| r.u8(o + 16) == 5).unwrap();
    let d = ybn::derive(g).unwrap();
    let f3 = |o: usize| [r.f32(o), r.f32(o + 4), r.f32(o + 8)];
    assert_eq!(f3(co + 144), d.quantum, "{name}: quantum");
    assert_eq!(f3(co + 160), d.center, "{name}: centre");
    let (nv, np) = (r.u32(co + 208) as usize, r.u32(co + 212) as usize);
    let vo = off(r.u64(co + 176));
    let mine_v: Vec<u8> = d.verts.iter().flat_map(|v| v.iter().flat_map(|x| x.to_le_bytes())).collect();
    assert_eq!(&r.virt[vo..vo + 6 * nv], &mine_v[..], "{name}: vertices");
    let po = off(r.u64(co + 136));
    let mine_p: Vec<u8> = d.polys.iter().flat_map(|(t, e, _)| {
        let mut b = 1u32.to_le_bytes().to_vec();
        t.iter().for_each(|x| b.extend((*x as u16).to_le_bytes()));
        e.iter().for_each(|x| b.extend(x.to_le_bytes()));
        b
    }).collect();
    assert_eq!(&r.virt[po..po + 16 * np], &mine_p[..], "{name}: polygons");
    let bo = off(r.u64(co + 304));
    let nn = r.u32(bo + 88) as usize;
    let no = off(r.u64(bo + 80));
    let mine_n: Vec<u8> = d.nodes.iter().flat_map(|n| n.iter().flat_map(|x| x.to_le_bytes())).collect();
    assert_eq!(&r.virt[no..no + 16 * nn], &mine_n[..], "{name}: BVH nodes");
    for (k, v) in d.bvh.iter().enumerate() {
        assert_eq!(f3(bo + 16 * k), *v, "{name}: BVH header row {k}");
    }
    assert_eq!((r.u32(bo + 12), r.u32(bo + 60)), (1, 0xFFC0_0000), "{name}: BVH header w lanes");
}
