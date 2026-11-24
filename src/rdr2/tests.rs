//! Self-contained RSC8 tests: no game data, no stored binaries. Files are
//! built in memory and read back with `read` (and `rsc8::Rsc8`).

use super::{read, rsc8::Rsc8, ydr, ytd, ytyp};
use crate::gta5::{
    rsc::joaat,
    ydr::{Geom, Mesh},
    ytd::{Tex, TexFormat},
};

/// Unit cube, 24 vertices (4 per face, face normals), 36 indices.
fn cube(texture: &str, normal: &str) -> Mesh {
    let mut g = Geom { texture: texture.into(), normal: normal.into(), ..Default::default() };
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1., 0., 0.], [0., 1., 0.], [0., 0., 1.]),
        ([-1., 0., 0.], [0., 0., 1.], [0., 1., 0.]),
        ([0., 1., 0.], [0., 0., 1.], [1., 0., 0.]),
        ([0., -1., 0.], [1., 0., 0.], [0., 0., 1.]),
        ([0., 0., 1.], [1., 0., 0.], [0., 1., 0.]),
        ([0., 0., -1.], [0., 1., 0.], [1., 0., 0.]),
    ];
    for (n, u, v) in faces {
        let base = g.positions.len() as u16;
        for (a, b) in [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)] {
            g.positions.push(std::array::from_fn(|k| 0.5 * n[k] + 0.5 * a * u[k] + 0.5 * b * v[k]));
            g.normals.push(n);
            g.colours.push(0xFFFF_FFFF);
            g.uvs.push([(a + 1.) / 2., (1. - b) / 2.]);
        }
        g.indices.extend([0, 1, 2, 0, 2, 3].map(|i| base + i));
    }
    Mesh { name: "mini_cube".into(), geoms: vec![g] }
}

fn tex(name: &str, w: u16, h: u16, levels: u8, format: TexFormat) -> Tex {
    let mut t = Tex { name: name.into(), width: w, height: h, levels, format, data: Vec::new(), unk32: 0, usage_data: 0, extra_flags: 0 };
    let len = t.mip_chain_len();
    t.data = (0..len).map(|i| (i * 7 + 1) as u8).collect();
    t
}

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

#[test]
fn dec3n_encoding() {
    // golden normals: (1,0,0), (-1,0,0), (0,0,1)
    assert_eq!(ydr::dec3n([1., 0., 0.], 0), 0x2008_03FF);
    assert_eq!(ydr::dec3n([-1., 0., 0.], 0), 0x2008_0000);
    assert_eq!(ydr::dec3n([0., 0., 1.], 0), 0x3FF8_0200);
    assert_eq!(ydr::dec3n([0., 0., 0.], -1) >> 30, 3);
    for v in [[0.3f32, -0.7, 0.648], [-0.01, 0.0, 1.0], [0.577, 0.577, -0.577]] {
        for w in [-1i8, 0, 1] {
            let (d, dw) = ydr::dec3n_decode(ydr::dec3n(v, w));
            assert_eq!(dw, w);
            for k in 0..3 {
                assert!((d[k] - v[k]).abs() <= 1.0 / 511.0 + 1e-6, "{v:?} -> {d:?}");
            }
        }
    }
}

#[test]
fn shader_param_hashes() {
    for (n, h) in [("default", 0xE4DF_46D5u32), ("standard", 0xBCF0_3FC1), ("diffusetex", 0xC8E8_C282), ("bumptex", 0x1E77_BF4C), ("speculartex", 0xC6F4_6E79), ("tintoverride", 0xF086_53A5), ("diffusesampler", 0xF1FE_2B71), ("bumpiness", 0xF671_2B81), ("mini_checker", 0x39FA_7EA6)] {
        assert_eq!(joaat(n), h, "{n}");
    }
}

#[test]
fn block_counts() {
    // vanilla RDR2 values
    assert_eq!(ytd::block_count(128, 128, 6, 8, 4), 0x600);
    assert_eq!(ytd::block_count(256, 256, 7, 8, 4), 0x1600);
    assert_eq!(ytd::block_count(4, 4, 1, 8, 4), 0x40);
    assert_eq!(ytd::block_count(64, 64, 5, 8, 4), 0x200);
    assert_eq!(ytd::block_count(16, 16, 5, 4, 1), 512);
}

#[test]
fn ytd_multi_format() {
    let texs = vec![
        ytd::Texture { tex: tex("mm_ab", 64, 64, 5, TexFormat::DXT1), srgb: true },
        ytd::Texture { tex: tex("mm_nm", 64, 64, 5, TexFormat::ATI2), srgb: false },
        ytd::Texture { tex: tex("mm_ma", 64, 64, 7, TexFormat::DXT1), srgb: false },
        ytd::Texture { tex: tex("mm_rgba", 16, 16, 5, TexFormat::A8B8G8R8), srgb: false },
    ];
    let b = ytd::build_ytd(&texs).unwrap();
    // header: RSC8, version 2, deflate (byte 0), no encryption, flag nibbles 0 / 2
    assert_eq!(&b[..4], b"RSC8");
    assert_eq!(&b[4..8], &[2, 0, 0, 0]);
    let (vf, pf) = (le32(&b, 8), le32(&b, 12));
    assert_eq!((vf & 0xF, pf & 0xF), (0, 2));
    let r = Rsc8::parse(&b).unwrap();
    assert_eq!(r.virt.len() as u32, vf & !0xF);
    assert_eq!(r.phys.len() as u32, pf & !0xF);
    // dictionary root at 0; first texture (largest block, 176 B) right after it
    assert_eq!(r.u64(0x18), 1);
    let d = read::ytd(&b).unwrap();
    assert!(d.block_map);
    assert_eq!(d.textures.len(), 4);
    let t0 = r.sys(r.u64(r.sys(r.u64(0x30), 8).unwrap()), 0xB0).unwrap();
    assert_eq!(t0, 0x40);
    let by = |n: &str| d.textures.iter().find(|t| t.name == n).unwrap().clone();
    let (ab, nm, ma, rgba) = (by("mm_ab"), by("mm_nm"), by("mm_ma"), by("mm_rgba"));
    assert_eq!((ab.format, ab.block_stride, ab.block_count, ab.mips), (0x48, 8, 0x200, 5));
    assert_eq!((nm.format, nm.block_stride, nm.block_count), (0x53, 16, 0x200));
    // BC chains end at 4x4: 64 -> 4 is 5 levels
    assert_eq!((ma.format, ma.mips), (0x47, 5));
    assert_eq!((rgba.format, rgba.block_stride, rgba.block_count, rgba.mips), (0x1C, 4, 512, 5));
    for t in &d.textures {
        assert_eq!(t.flags, 0x1800_8002);
        assert_eq!(t.data_off % 16, 0);
    }
    // pixel data = the mip chain, then zeros up to BlockCount * BlockStride
    let src = &texs[0].tex.data;
    assert_eq!(&r.phys[ab.data_off..ab.data_off + src.len()], &src[..]);
    assert!(r.phys[ab.data_off + src.len()..ab.data_off + 0x200 * 8].iter().all(|&x| x == 0));
    // physical blocks by size, descending: the BC5 one (8 KB) first
    assert_eq!(nm.data_off, 0);
}

#[test]
fn ydr_default_cube() {
    let b = ydr::build_ydr(&[cube("mini_checker", "")], [9998.0; 4], None, &ydr::Opts::default()).unwrap();
    assert_eq!(&b[..8], &[b'R', b'S', b'C', b'8', 162, 0, 0, 0]);
    assert_eq!((le32(&b, 8) & 0xF, le32(&b, 12) & 0xF), (0xA, 2));
    let r = Rsc8::parse(&b).unwrap();
    assert!(r.phys.is_empty());
    let i = read::ydr(&b).unwrap();
    assert_eq!(i.name, "mini_cube.#dr");
    assert_eq!(i.mask_flags, [0xFF01, 0, 0, 0]);
    assert!((i.radius - 0.866_025).abs() < 1e-4);
    assert_eq!(i.bbox, ([-0.5; 3], [0.5; 3]));
    assert_eq!(i.shaders.len(), 1);
    let s = &i.shaders[0];
    assert_eq!((s.hash, s.bucket, s.data_size, s.buffers), (0xE4DF_46D5, 0, 424, 2));
    assert_eq!(s.textures, vec![None, Some("mini_checker".to_string()), None]);
    assert_eq!(s.samplers, vec![1, 0, 7, 13, 8, 10, 11, 9]);
    assert_eq!(i.geoms.len(), 1);
    let g = &i.geoms[0];
    assert_eq!((g.vertices, g.indices, g.stride, g.format_data), (24, 36, 28, 0x6071));
    assert_eq!(g.elements, vec![(0, 0, 12, 6), (4, 288, 4, 24), (24, 384, 4, 28), (28, 480, 8, 16)]);
    // blocks: root 0xD0 bytes, then the largest (vertex data 672 B) and the
    // parameter block (424 B), as in the CodeX golden file
    assert_eq!(g.vdata, 0xD0);
    let sh = r.sys(r.u64(r.sys(r.u64(r.sys(r.u64(0x10), 64).unwrap() + 0x10), 8).unwrap()), 64).unwrap();
    assert_eq!(r.sys(r.u64(sh + 8), 424).unwrap(), 0x370);
    assert_eq!(r.u32(sh + 4), 0x6D65_7461);
    assert_eq!(r.u64(sh + 0x18), 0, "Unknowns pointer stays null (vanilla)");
    // texture reference stub: vanilla VFT, flags, tile mode, MipLevels 1
    let tp = r.sys(r.u64(sh + 0x10), 24).unwrap();
    let stub = r.sys(r.u64(tp + 8), 80).unwrap();
    assert_eq!((r.u64(stub), r.u32(stub + 0x10), r.u8(stub + 0x20), r.u8(stub + 0x22)), (0x1_4091_0008, 0x1000_0000, 0xFF, 1));
    // vertex data: normal (1,0,0) of the first face in Dec3N, colour white
    assert_eq!(r.u32(g.vdata + 288), 0x2008_03FF);
    assert_eq!(r.u32(g.vdata + 384), 0xFFFF_FFFF);
    // block map pages 1 / 0
    let bm = r.sys(r.u64(8), 48).unwrap();
    assert_eq!((r.u8(bm + 8), r.u8(bm + 9)), (1, 0));
}

#[test]
fn ydr_standard_cube() {
    let b = ydr::build_ydr(&[cube("mini_ab", "mini_n")], [9998.0; 4], None, &ydr::Opts::default()).unwrap();
    let i = read::ydr(&b).unwrap();
    let s = &i.shaders[0];
    assert_eq!((s.hash, s.data_size, s.buffers, s.samplers.len()), (0xBCF0_3FC1, 1038, 1, 14));
    assert_eq!(s.textures.iter().flatten().cloned().collect::<Vec<_>>(), vec!["mini_ab", "mini_n", ydr::BLANK_MA, ydr::BLANK_MB]);
    let g = &i.geoms[0];
    assert_eq!((g.stride, g.format_data), (40, 0x60A1));
    let slots: Vec<usize> = g.elements.iter().map(|e| e.0).collect();
    assert_eq!(slots, vec![0, 4, 8, 24, 28, 29]);
    // tangents are unit Dec3N with w = +-1 (stored 1 / 3); TEXCOORD1 = (1, 0)
    let r = Rsc8::parse(&b).unwrap();
    for v in 0..24 {
        let (t, w) = ydr::dec3n_decode(r.u32(g.vdata + 384 + 4 * v));
        assert!(w == 1 || w == -1);
        assert!(((t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt() - 1.0).abs() < 0.01);
    }
    assert_eq!((r.f32(g.vdata + 768), r.f32(g.vdata + 772)), (1.0, 0.0));
    assert_eq!(ydr::texture_names(&[cube("mini_ab", "mini_n")], &ydr::Opts::default()), vec!["mini_ab", "mini_n", ydr::BLANK_MA, ydr::BLANK_MB]);
    // --default-only keeps the diffuse-only shader
    let b = ydr::build_ydr(&[cube("mini_ab", "mini_n")], [9998.0; 4], None, &ydr::Opts { standard: false, flat_normal: false, kinds: false }).unwrap();
    assert_eq!(read::ydr(&b).unwrap().shaders[0].hash, 0xE4DF_46D5);
}

#[test]
fn ydr_embedded_dictionary() {
    let t = vec![ytd::Texture { tex: tex("mini_checker", 64, 64, 5, TexFormat::DXT1), srgb: true }];
    let b = ydr::build_ydr(&[cube("mini_checker", "")], [9998.0; 4], Some(&t), &ydr::Opts::default()).unwrap();
    let r = Rsc8::parse(&b).unwrap();
    assert_eq!(r.phys.len(), 0x1000);
    assert_eq!(le32(&b, 12), 0x1002);
    let i = read::ydr(&b).unwrap();
    let d = i.dict.as_ref().unwrap();
    assert!(!d.block_map, "embedded dictionary has no block map");
    assert_eq!(d.textures[0].name, "mini_checker");
    // the shader's texture parameter points at the dictionary's texture itself
    let sg = r.sys(r.u64(0x10), 64).unwrap();
    let dict_tex = r.u64(r.sys(r.u64(r.sys(r.u64(sg + 8), 64).unwrap() + 0x30), 8).unwrap());
    let sh = r.sys(r.u64(r.sys(r.u64(sg + 0x10), 8).unwrap()), 64).unwrap();
    assert_eq!(r.u64(r.sys(r.u64(sh + 0x10), 24).unwrap() + 8), dict_tex);
    let bm = r.sys(r.u64(8), 48).unwrap();
    assert_eq!((r.u8(bm + 8), r.u8(bm + 9)), (1, 1));
}

#[test]
fn ydr_lods_and_buckets() {
    let mut hi = cube("a", "");
    let mut extra = cube("b", "").geoms.remove(0);
    extra.kind = 3; // cutout
    hi.geoms.push(extra);
    let med = cube("a", "");
    let b = ydr::build_ydr(&[hi, med, Mesh::default(), Mesh::default()], [10.0, 50.0, 100.0, 200.0], None, &ydr::Opts::default()).unwrap();
    let i = read::ydr(&b).unwrap();
    // shaders shared across levels: "a" opaque and "b" cutout
    assert_eq!(i.shaders.len(), 2);
    assert_eq!(i.shaders[1].bucket, 3);
    assert_eq!(i.mask_flags, [0xFF09, 0xFF01, 0, 0]);
    assert_eq!(i.geoms.iter().map(|g| (g.lod, g.shader)).collect::<Vec<_>>(), vec![(0, 0), (0, 1), (1, 0)]);
    let r = Rsc8::parse(&b).unwrap();
    assert_eq!([r.f32(0x70), r.f32(0x74)], [10.0, 50.0]);
}

/// The two archetypes of the CodeX reference files (r6m_test, r6m_test2).
fn r6m_archetypes() -> Vec<ytyp::Archetype> {
    let mut a = ytyp::Archetype::from_box("r6m_test", 100.0, [-1.0; 3], [1.0; 3]);
    let mut b = ytyp::Archetype::from_box("r6m_test2", 50.0, [-0.5, -0.25, 0.0], [0.5, 0.25, 2.0]);
    a.bs_radius = 1.732_050_8;
    (b.bs_centre, b.bs_radius) = ([0.0; 3], 1.732_050_8);
    vec![a, b]
}

#[test]
fn ytyp_case_sensitive_hashes() {
    // class / enum names hash without lower-casing; fields with joaat()
    let cs = |s: &str| {
        let mut h = 0u32;
        for c in s.bytes() {
            h = h.wrapping_add(c as u32);
            h = h.wrapping_add(h << 10);
            h ^= h >> 6;
        }
        h = h.wrapping_add(h << 3);
        h ^= h >> 11;
        h.wrapping_add(h << 15)
    };
    assert_eq!(cs("CMapTypes"), ytyp::CMAPTYPES);
    assert_eq!(cs("CBaseArchetypeDef"), ytyp::CBASEARCHETYPEDEF);
    assert_eq!(cs("MAP_ENTITY_TYPE_DUMMY_OBJECT"), 0x7CD1_64E4);
    assert_eq!(cs("ASSET_TYPE_DRAWABLE"), 0x07C0_CB71);
    assert_ne!(joaat("CMapTypes"), ytyp::CMAPTYPES);
    assert_eq!(joaat("r6m_test"), 0x839D_095D);
    assert_eq!(joaat("R6M_Test"), 0x839D_095D);
    assert_eq!(joaat("r6m_test2"), 0x07D0_74C0);
    assert_eq!(ytyp::meta_ptr(2, 160), 0x0A00_0002);
}

fn check_ytyp(n: usize) {
    let archs = &r6m_archetypes()[..n];
    let b = ytyp::build_ytyp("r6m_test", archs).unwrap();
    // header: RSC8, version 3, deflate, virtual only (pflags = version nibble 3)
    assert_eq!(&b[..8], &[b'R', b'S', b'C', b'8', 3, 0, 0, 0]);
    assert_eq!(le32(&b, 12), 3);
    let r = Rsc8::parse(&b).unwrap();
    assert!(r.phys.is_empty());
    assert_eq!(r.virt.len(), if n == 1 { 0x520 } else { 0x5C0 });
    // root
    assert_eq!((r.u64(0), r.u32(0x10), r.u16(0x14), r.u32(0x20)), (0, 0x5052_4430, 0x79, 1));
    assert_eq!((r.u16(0x48), r.u16(0x4A), r.u16(0x4C), r.u64(0x50)), (2, 2, 3, 0));
    let bm = r.sys(r.u64(8), 48).unwrap();
    assert_eq!((r.u8(bm + 8), r.u8(bm + 9)), (1, 0));
    let unk = r.sys(r.u64(0x40), 12).unwrap();
    assert!(r.virt[unk..unk + 12].iter().all(|&x| x == 0));
    let i = read::ytyp(&b).unwrap();
    // definition tables
    let cl: Vec<_> = i.classes.iter().map(|c| (c.name, c.key, c.size, c.members.len())).collect();
    assert_eq!(cl, vec![(ytyp::CMAPTYPES, 249_887_037, 96, 11), (ytyp::CBASEARCHETYPEDEF, 1_841_055_807, 160, 21)]);
    assert_eq!(i.classes[1].members[20], (0x07D1_64A8, 144, 0x62, 0, 0x0795_25DA));
    let en: Vec<_> = i.enums.iter().map(|e| (e.0, e.1, e.2.len())).collect();
    assert_eq!(en, vec![(0x76BA_F7C7, 1_561_968_235, 5), (0x0795_25DA, 3_087_975_696, 8)]);
    assert_eq!(i.enums[0].2[2], (2, 0x07C0_CB71));
    assert_eq!(i.enums[1].2[3], (3, 0x7CD1_64E4));
    // data blocks, every meta pointer resolved (archetypes array + n elements)
    let bl: Vec<_> = i.blocks.iter().map(|b| (b.0, b.1)).collect();
    assert_eq!(bl, vec![(ytyp::CMAPTYPES, 96), (ytyp::CBASEARCHETYPEDEF, 160 * n as u32), (7, 8 * n as u32)]);
    assert_eq!(i.meta_ptrs, 1 + n);
    let (m, p) = (i.blocks[0].2, i.blocks[2].2);
    assert_eq!((r.u64(m + 24), r.u16(m + 32), r.u16(m + 34)), (3, n as u16, n as u16));
    for k in [8, 40, 64, 80] {
        assert!(r.virt[m + k..m + k + 16].iter().all(|&x| x == 0), "CMapTypes +{k} not an empty array");
    }
    for k in 0..n {
        assert_eq!(r.u64(p + 8 * k), ytyp::meta_ptr(2, 160 * k));
    }
    assert_eq!(i.name, 0x839D_095D);
    // archetype fields
    assert_eq!(i.archetypes.len(), n);
    for (a, x) in i.archetypes.iter().zip(archs) {
        let h = joaat(&x.name);
        assert_eq!((a.name, a.texture_dictionary, a.asset_name), (h, h, h));
        assert_eq!((a.clip_dictionary, a.drawable_dictionary, a.physics_dictionary), (0, 0, 0));
        assert_eq!((a.load_flags, a.flags, a.avoidance), (0x30C2_0000, 0x30C2_0000, 8));
        assert_eq!((a.asset_type, a.entity_type, a.extensions), (2, 3, 0));
        assert_eq!((a.lod_dist, a.hd_texture_dist, a.bs_radius, a.bs_centre), (x.lod_dist, 5.0, x.bs_radius, x.bs_centre));
        // w lanes repeat x, as in vanilla files
        assert_eq!(a.bb_min, [x.bb_min[0], x.bb_min[1], x.bb_min[2], x.bb_min[0]]);
        assert_eq!(a.bb_max, [x.bb_max[0], x.bb_max[1], x.bb_max[2], x.bb_max[0]]);
    }
}

#[test]
fn ytyp_one_archetype() {
    check_ytyp(1);
}

#[test]
fn ytyp_two_archetypes() {
    check_ytyp(2);
}

#[test]
fn ytyp_defaults_and_errors() {
    let a = ytyp::Archetype::from_box("p", 40.0, [-0.5, -0.25, 0.0], [0.5, 0.25, 2.0]);
    assert_eq!(a.bs_centre, [0.0, 0.0, 1.0]);
    assert!((a.bs_radius - (1.0f32 + 0.25 + 4.0).sqrt() / 2.0).abs() < 1e-6);
    let mut c = a.clone();
    c.physics_dictionary = Some("p".into());
    let i = read::ytyp(&ytyp::build_ytyp("p", &[c]).unwrap()).unwrap();
    assert_eq!(i.archetypes[0].physics_dictionary, joaat("p"));
    assert!(ytyp::build_ytyp("p", &[]).is_err());
    let mut bad = a.clone();
    bad.bb_max[0] = f32::NAN;
    assert!(ytyp::build_ytyp("p", &[bad]).is_err());
    // a meta pointer past its data block is caught by the reader
    let b = ytyp::build_ytyp("p", &[a]).unwrap();
    let mut r = Rsc8::parse(&b).unwrap();
    let p = r.u64(0x38) as usize & 0x0FFF_FFFF;
    let ptrs = r.u64(p + 32 + 8) as usize & 0x0FFF_FFFF;
    r.virt[ptrs..ptrs + 8].copy_from_slice(&ytyp::meta_ptr(2, 160).to_le_bytes());
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut enc, &r.virt).unwrap();
    let mut f = b[..16].to_vec();
    f.extend(enc.finish().unwrap());
    assert!(read::ytyp(&f).unwrap_err().contains("past data block"));
}

#[test]
fn rejects_bad_input() {
    assert!(ytd::build_ytd(&[]).is_err());
    let mut m = cube("a", "");
    m.geoms[0].indices.push(99);
    m.geoms[0].indices.extend([0, 1]);
    assert!(ydr::build_ydr(&[m], [9998.0; 4], None, &ydr::Opts::default()).is_err());
    let mut b = ydr::build_ydr(&[cube("a", "")], [9998.0; 4], None, &ydr::Opts::default()).unwrap();
    b[8] ^= 0x10; // virtual size no longer matches the payload
    assert!(Rsc8::parse(&b).is_err());
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn prop_args_tint_row_and_as() {
    let p = super::parse_prop_args(&args(&["Prop_A", "--game", "g", "--out", "o", "--tint-row", "5", "--as", "Prop_A_Blue"])).unwrap();
    assert_eq!((p.name.as_deref(), p.tint_row, p.as_name.as_deref(), p.tints), (Some("prop_a"), 5, Some("prop_a_blue"), false));
    let p = super::parse_prop_args(&args(&["prop_a", "--game", "g"])).unwrap();
    assert_eq!((p.tint_row, p.as_name, p.tints), (0, None, false));
    assert!(super::parse_prop_args(&args(&["prop_a", "--tints"])).unwrap().tints);
}

#[test]
fn prop_args_errors() {
    for bad in [
        &["p", "--tint-row"][..],
        &["p", "--tint-row", "-1"],
        &["p", "--tint-row", "x"],
        &["p", "--as"],
        &["p", "--as", "has space"],
        &["p", "--as", "dash-name"],
        &["p", "--as", ""],
        &["p", "--tints", "--tint-row", "2"],
        &["p", "--tints", "--as", "q"],
    ] {
        assert!(super::parse_prop_args(&args(bad)).is_err(), "{bad:?}");
    }
    assert!(super::model_name(&"a".repeat(63)).is_ok());
    assert!(super::model_name(&"a".repeat(64)).is_err());
}

#[test]
fn tint_palette_rows() {
    use crate::gta5::fivem::{check_tint_row, palette_index, palette_rows, PaletteUse};
    assert_eq!([palette_rows(0), palette_rows(37), palette_rows(256), palette_rows(2051)], [0, 1, 1, 8]);
    assert_eq!([palette_index(0, 1), palette_index(5, 1), palette_index(7, 255)], [1, 1281, 2047]);
    // 8 rows x 256 + 3: column 0 white, column 1 tinted in rows 1.., column 10 filler
    let mut palette = vec![[255, 187, 255, 255]; 2051];
    for r in 0..8 {
        palette[r * 256] = [255; 4];
        palette[r * 256 + 1] = if r == 0 { [255; 4] } else { [0x70, 0xad, 0xde, 255] };
    }
    let u = PaletteUse { palette, columns: vec![0, 1] };
    assert_eq!((u.colour(0, 1), u.colour(5, 1), u.colour(5, 0), u.colour(5, 10)), (None, Some([0x70, 0xad, 0xde]), None, None));
    let uses = [u];
    assert!(check_tint_row("m", &uses, 0).is_ok());
    assert!(check_tint_row("m", &uses, 7).is_ok());
    let e = check_tint_row("m", &uses, 8).unwrap_err();
    assert!(e.contains("0..=7"), "{e}");
    // short single row: only row 0
    let short = [PaletteUse { palette: vec![[0xc0, 0x8f, 0x4f, 255]; 37], columns: vec![0] }];
    assert!(check_tint_row("m", &short, 0).is_ok() && check_tint_row("m", &short, 1).is_err());
    assert_eq!(short[0].colour(0, 0), Some([0xc0, 0x8f, 0x4f]));
    assert_eq!(short[0].colour(0, 40), None);
    assert!(check_tint_row("m", &[], 1).unwrap_err().contains("no tint palette"));
}

/// RGB of a BC1 block whose texels all use colour 0 (indices 0).
fn bc1_c0(block: &[u8]) -> [u8; 3] {
    let c = u16::from_le_bytes([block[0], block[1]]);
    let (r, g, b) = ((c >> 11) as u8, (c >> 5 & 63) as u8, (c & 31) as u8);
    [r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2]
}

#[test]
fn blank_material_maps() {
    let ma = ytd::blank_ma("x_ma");
    assert_eq!((ma.tex.width, ma.tex.height, ma.tex.format, ma.srgb), (4, 4, TexFormat::DXT1, false));
    assert_eq!(&ma.tex.data[4..], &[0; 4], "all texels colour 0");
    assert_eq!(bc1_c0(&ma.tex.data), [0, 243, 255]);
    // blank_mb: indices 2 = (2 c0 + c1) / 3 of 0x000F / 0x0010: (0, 0, ~127)
    let mb = ytd::blank_mb("x_mb");
    assert_eq!(&mb.tex.data, &[0x0F, 0x00, 0x10, 0x00, 0xAA, 0xAA, 0xAA, 0xAA]);
    let (c0, c1) = (bc1_c0(&mb.tex.data[0..2]), bc1_c0(&mb.tex.data[2..4]));
    let mix: Vec<u32> = (0..3).map(|k| (2 * c0[k] as u32 + c1[k] as u32) / 3).collect();
    assert_eq!(mix, vec![0, 0, 126]);
}

#[test]
fn ma_b_channel_rule() {
    let px = |b: &dyn Fn(usize) -> u8| -> Vec<u8> { (0..256).flat_map(|i| [if i % 7 == 0 { 255 } else { 0 }, (i % 200) as u8, b(i), 17]).collect() };
    // flat 128 (+-3 noise): no AO authored -> 255; R and G copied, A 255
    let src = px(&|i| 125 + (i % 7) as u8);
    let out = ytd::ma_from_spec(&src);
    for (s, o) in src.chunks_exact(4).zip(out.chunks_exact(4)) {
        assert_eq!(o, &[s[0], s[1], 255, 255]);
    }
    // structured AO (crevices) is copied
    let src = px(&|i| if i % 16 < 3 { 40 } else { 230 });
    assert!(ytd::ma_from_spec(&src).chunks_exact(4).zip(src.chunks_exact(4)).all(|(o, s)| o[2] == s[2] && o[3] == 255));
    // flat but not 128 (e.g. 200), and 128-centred but noisy: copied
    assert!(!ytd::flat_128([200u8; 64].into_iter()));
    assert!(!ytd::flat_128((0..64u8).map(|i| if i % 2 == 0 { 118 } else { 138 })));
    assert!(ytd::flat_128([128u8, 131, 125, 128].into_iter()));
    assert!(ytd::flat_128([135u8; 8].into_iter()) && !ytd::flat_128([136u8; 8].into_iter()));
}

#[test]
fn standard_binds_material_ma() {
    let mut a = cube("a_ab", "a_n");
    a.geoms[0].spec = "r6m_0000000a_ma".into();
    // a tinted copy of material a shares its _ma
    let mut at = a.geoms[0].clone();
    at.texture = "a_ab_t70adde".into();
    let mut b = cube("b_ab", "b_n").geoms.remove(0);
    b.spec = "r6m_0000000b_ma".into();
    let c = cube("c_ab", "c_n").geoms.remove(0); // no speculartex: blank
    a.geoms.extend([at, b, c]);
    let o = ydr::Opts::default();
    let bin = ydr::build_ydr(std::slice::from_ref(&a), [9998.0; 4], None, &o).unwrap();
    let i = read::ydr(&bin).unwrap();
    let spec_of = |diffuse: &str| i.shaders.iter().find(|s| s.textures.iter().flatten().any(|t| t == diffuse)).map(|s| s.textures.iter().flatten().nth(2).unwrap().clone()).unwrap();
    assert_eq!(spec_of("a_ab"), "r6m_0000000a_ma");
    assert_eq!(spec_of("a_ab_t70adde"), "r6m_0000000a_ma");
    assert_eq!(spec_of("b_ab"), "r6m_0000000b_ma");
    assert_eq!(spec_of("c_ab"), ydr::BLANK_MA);
    let names = ydr::texture_names(std::slice::from_ref(&a), &o);
    for n in ["r6m_0000000a_ma", "r6m_0000000b_ma", ydr::BLANK_MA, ydr::BLANK_MB] {
        assert_eq!(names.iter().filter(|x| x.as_str() == n).count(), 1, "{n}");
    }
    // --default-only binds no _ma at all
    assert!(!ydr::texture_names(&[a], &ydr::Opts { standard: false, flat_normal: false, kinds: false }).iter().any(|n| n.ends_with("_ma")));
}

/// `redm prop`: opaque geometry without a normal map is `standard` with the
/// flat normal map and the blank `_ma`; non-opaque geometry and
/// `--default-only` keep `default`; the OBJ path (flat_normal off) too.
#[test]
fn opaque_without_normal_map_is_standard() {
    let mut a = cube("plain_ab", "");
    let mut alpha = cube("glass_ab", "").geoms.remove(0);
    alpha.kind = 1;
    a.geoms.push(alpha);
    let o = ydr::Opts { standard: true, flat_normal: true, kinds: false };
    let names = ydr::texture_names(std::slice::from_ref(&a), &o);
    assert_eq!(names, vec!["plain_ab", ydr::FLAT_NORMAL, ydr::BLANK_MA, ydr::BLANK_MB, "glass_ab"]);
    let bin = ydr::build_ydr(std::slice::from_ref(&a), [9998.0; 4], None, &o).unwrap();
    let i = read::ydr(&bin).unwrap();
    let hashes: Vec<u32> = i.shaders.iter().map(|s| s.hash).collect();
    assert_eq!(hashes, vec![joaat("standard"), joaat("default")]);
    let bound: Vec<&str> = i.shaders[0].textures.iter().flatten().map(String::as_str).collect();
    assert_eq!(bound, vec!["plain_ab", ydr::FLAT_NORMAL, ydr::BLANK_MA, ydr::BLANK_MB]);
    for off in [ydr::Opts { standard: false, flat_normal: true, kinds: false }, ydr::Opts::default()] {
        assert!(!ydr::texture_names(std::slice::from_ref(&a), &off).iter().any(|n| n == ydr::FLAT_NORMAL));
    }
    // the stand-in: one linear BC1 block of (128, 128, 255) in 565
    let t = ytd::flat_normal(ydr::FLAT_NORMAL);
    assert_eq!((t.tex.width, t.tex.height, t.tex.levels, t.srgb, t.tex.format), (4, 4, 1, false, TexFormat::DXT1));
    let c = u16::from_le_bytes([t.tex.data[0], t.tex.data[1]]);
    assert_eq!((c >> 11, c >> 5 & 63, c & 31), (16, 32, 31));
}

#[test]
fn lod_switch_scaling() {
    // --lod-scale 1 --lod-floor 0: the 6m distances unchanged
    let box_6m = [3.651_17, 5.126_83, 7.213_725, 9998.0];
    assert_eq!(super::scale_lods(box_6m, 1.0, 0.0), box_6m);
    // defaults: x2, floors 20 / 28.3 / 40 m, never 9998 touched
    let d = super::scale_lods(box_6m, super::LOD_SCALE, super::LOD_FLOOR);
    assert_eq!(d[0], 20.0);
    assert!((d[1] - 28.284_27).abs() < 1e-3 && (d[2] - 40.0).abs() < 1e-3 && d[3] == 9998.0);
    let bench = super::scale_lods([10.893_481, 15.322_959, 9998.0, 9998.0], 2.0, 20.0);
    assert!((bench[0] - 21.786_962).abs() < 1e-4 && (bench[1] - 30.645_918).abs() < 1e-4 && bench[2] == 9998.0);
    // a 0 High switch gets the floor; switches never shrink
    assert_eq!(super::scale_lods([0.0, 8.0, 9998.0, 9998.0], 2.0, 20.0)[0], 20.0);
    let m = super::scale_lods([30.0, 31.0, 32.0, 9998.0], 2.0, 0.0);
    assert!(m[0] <= m[1] && m[1] <= m[2]);
    assert!(super::parse_prop_args(&["x".into(), "--lod-scale".into(), "0".into()]).is_err());
    let p = super::parse_prop_args(&["x".into(), "--lod-scale".into(), "1".into(), "--lod-floor".into(), "0".into()]).unwrap();
    assert_eq!((p.lod_scale, p.lod_floor), (Some(1.0), Some(0.0)));
}

/// Name hash of a vanilla parameter name as CodeX prints it (unnamed ones
/// as `xxx_0xHHHHHHHH`).
fn param_hash(n: &str) -> u32 {
    match n.rsplit_once("_0x") {
        Some((_, h)) => u32::from_str_radix(h, 16).unwrap(),
        None => joaat(n),
    }
}

type VParam = (&'static str, u16, &'static [f32]);

/// `standard`-family cbuffer parameters up to hardalphablend as CodeX
/// prints them for vanilla p_strmillwindow01ax, p_lamp01x and
/// p_helpwantedsign01x (name, offset, catalogue default).
const V_STD_HEAD: &[VParam] = &[
    ("tintpaletteselector", 0, &[0., 0.]),
    ("detailsettings", 16, &[0., 0., 24., 24.]),
    ("detailindexoverride", 32, &[-1.]),
    ("fresnel", 36, &[0.]),
    ("specmapintmask", 48, &[1., 0., 0.]),
    ("tintoverride", 64, &[1., 1., 1., 0.]),
    ("pbrglassiness", 80, &[0.]),
    ("pbrcavityrange", 84, &[0.]),
    ("pbrtweak_metallic", 88, &[0.]),
    ("pbrtweak_sheen", 92, &[0.]),
    ("pbrtweak_roughness", 96, &[0.]),
    ("pbrtweak_ao", 100, &[0.]),
    ("pbrtweak_translucency", 104, &[0.]),
    ("pbrtweak_fresnel", 108, &[0.]),
    ("pbrtweak_detail", 112, &[0.]),
    ("pbrtweak_detailindex_", 116, &[-1.]),
    ("pbrdetail_metallic", 120, &[0.]),
    ("pbrdetail_sheen", 124, &[0.]),
    ("pbrdetail_roughness", 128, &[0.]),
    ("pbrdetail_ao", 132, &[0.]),
    ("pbrdetail_translucency", 136, &[0.]),
    ("bumpiness", 140, &[1.]),
    ("wetnessmultiplier", 144, &[1.]),
    ("porosity", 148, &[0.5]),
    ("usetessellation", 152, &[0.]),
    ("hardalphablend", 156, &[0.]),
];
const V_TAIL_STD: &[VParam] = &[("marktaaghostingbit", 160, &[0.]), ("usepaintedwetness", 164, &[0.]), ("haspuddleswitch", 168, &[0.]), ("puddleheightbias", 172, &[0.]), ("puddlemaskscale", 176, &[2.]), ("translucencyastintmask", 180, &[0.])];
const V_TAIL_ALPHA: &[VParam] = &[("marktaaghostingbit", 160, &[0.]), ("translucencyastintmask", 164, &[0.])];
const V_TAIL_GLASS: &[VParam] = &[("bloommod", 160, &[0.2, 0.7]), ("marktaaghostingbit", 168, &[0.]), ("translucencyastintmask", 172, &[0.])];
const V_UNNAMED: &[VParam] = &[("PehecJA_0x9FBBA584", 0, &[0., 0.]), ("vuCYLFA_0x7CF9469A", 8, &[0., 0.]), ("TapXvDA_0xFEB9BA87", 16, &[0.])];
const V_DEFAULT_B0: &[VParam] = &[("tintoverride", 0, &[1., 1., 1., 0.]), ("wetnessmultiplier", 16, &[1.]), ("usetessellation", 20, &[0.]), ("marktaaghostingbit", 24, &[0.])];

/// Expected (hash, buffer, offset, values) list of a shader.
fn expect_cbuffers(runs: &[(u8, &[VParam])], set: &[(&str, &[f32])]) -> Vec<(u32, u8, u16, Vec<f32>)> {
    runs.iter()
        .flat_map(|&(b, r)| r.iter().map(move |&(n, off, v)| (param_hash(n), b, off, set.iter().find(|s| s.0 == n).map_or(v, |s| s.1).to_vec())))
        .collect()
}

/// `standard`-family texture slots as CodeX names them, vanilla order.
const V_STD_TEX: [&str; 7] = ["tintpalettetex", "oIZmQFA_0xE9F32960", "diffusetex", "detailtexarrayn", "bumptex", "speculartex", "speculartex2"];

fn hashes_of(n: &[&str]) -> Vec<u32> {
    n.iter().map(|n| param_hash(n)).collect()
}

struct Want {
    shader: ydr::Shader,
    name: &'static str,
    size: usize,
    buffers: Vec<u16>,
    textures: Vec<u32>,
    samplers: Vec<(u32, u8)>,
    cbuffers: Vec<(u32, u8, u16, Vec<f32>)>,
}

fn vanilla_defs() -> Vec<Want> {
    let smp = |s: &[(&str, u8)]| s.iter().map(|&(n, i)| (joaat(n), i)).collect::<Vec<_>>();
    let s_std = [("pointclamp", 0), ("trilinearclamp", 7), ("diffusesampler", 1), ("trilinearwrap", 13), ("miplinearclamp", 8), ("anisotropic16xwrap", 2), ("projdecaltexarraysampler", 12), ("miplinearwrap", 10), ("bumpsampler", 3), ("specsampler", 4), ("trilinearborderblack", 11), ("specsampler2", 5), ("raintexturesamplerglobal", 6), ("gcsmshadowtexturesamp", 9)];
    let s_decal = [("trilinearclamp", 7), ("diffusesampler", 1), ("pointclamp", 0), ("miplinearclamp", 8), ("anisotropic16xwrap", 2), ("projdecaltexarraysampler", 12), ("miplinearwrap", 10), ("bumpsampler", 3), ("specsampler", 4), ("trilinearborderblack", 11), ("specsampler2", 5), ("raintexturesamplerglobal", 6), ("gcsmshadowtexturesamp", 9)];
    let s_alpha = [("pointclamp", 0), ("diffusesampler", 1), ("trilinearclamp", 7), ("miplinearclamp", 8), ("anisotropic16xwrap", 2), ("miplinearwrap", 10), ("bumpsampler", 3), ("specsampler", 4), ("trilinearborderblack", 11), ("specsampler2", 5), ("raintexturesamplerglobal", 6), ("gcsmshadowtexturesamp", 9)];
    let s_default = [("diffusesampler", 1), ("pointclamp", 0), ("trilinearclamp", 7), ("trilinearwrap", 13), ("miplinearclamp", 8), ("miplinearwrap", 10), ("trilinearborderblack", 11), ("gcsmshadowtexturesamp", 9)];
    let mut decal_tex = V_STD_TEX.to_vec();
    decal_tex.push("fjsTvVA_0x56D855C5");
    use ydr::Shader::*;
    vec![
        Want { shader: Default, name: "default", size: 424, buffers: vec![32, 32], textures: hashes_of(&["oIZmQFA_0xE9F32960", "diffusetex", "fjsTvVA_0x56D855C5"]), samplers: smp(&s_default), cbuffers: expect_cbuffers(&[(0, V_DEFAULT_B0), (1, V_UNNAMED)], &[]) },
        Want { shader: Standard, name: "standard", size: 1038, buffers: vec![192], textures: hashes_of(&V_STD_TEX), samplers: smp(&s_std), cbuffers: expect_cbuffers(&[(0, V_STD_HEAD), (0, V_TAIL_STD)], &[]) },
        Want { shader: Decal, name: "standard_decal", size: 1229, buffers: vec![32, 192], textures: hashes_of(&decal_tex), samplers: smp(&s_decal), cbuffers: expect_cbuffers(&[(0, V_UNNAMED), (1, V_STD_HEAD), (1, V_TAIL_STD)], &[]) },
        Want { shader: Alpha, name: "standard_alpha", size: 972, buffers: vec![176], textures: hashes_of(&V_STD_TEX), samplers: smp(&s_alpha), cbuffers: expect_cbuffers(&[(0, V_STD_HEAD), (0, V_TAIL_ALPHA)], &[]) },
        Want { shader: Glass, name: "standard_glass", size: 972, buffers: vec![176], textures: hashes_of(&V_STD_TEX), samplers: smp(&s_alpha), cbuffers: expect_cbuffers(&[(0, V_STD_HEAD), (0, V_TAIL_GLASS)], &[("pbrglassiness", &[1.])]) },
    ]
}

/// Shader definitions against the vanilla parameter lists (CodeX XML of
/// the files named in ydr.rs).
#[test]
fn shader_definitions_match_vanilla() {
    for w in vanilla_defs() {
        let d = w.shader.def();
        assert_eq!((d.name, d.hash), (w.name, joaat(w.name)));
        assert_eq!(d.block_size(), w.size, "{}", w.name);
        assert_eq!(d.buffer_sizes(), w.buffers, "{}", w.name);
        assert_eq!(d.texture_hashes(), w.textures, "{}", w.name);
        assert_eq!(d.samplers().to_vec(), w.samplers, "{}", w.name);
        let got: Vec<(u32, u8, u16, Vec<f32>)> = d.cbuffers().map(|(h, b, o, v)| (h, b, o, v.to_vec())).collect();
        assert_eq!(got, w.cbuffers, "{}", w.name);
    }
}

/// One geometry per kind through `build_ydr` and back through the in-crate
/// reader: hash, bucket and bucket-mask bytes, block size, buffer sizes,
/// parameter table, every cbuffer value, bound textures, vertex layout and
/// the per-LOD bucket flags.
#[test]
fn shader_blocks_round_trip() {
    // opaque, cutout, decal, alpha, glass, emissive
    let mut m = cube("k0_ab", "k0_n");
    for (k, tex) in [(3u8, "k3_ab"), (2, "k2_ab"), (1, "k1_ab"), (5, "k5_ab"), (4, "k4_ab")] {
        let mut g = cube(tex, "").geoms.remove(0);
        g.kind = k;
        if k == 3 {
            g.spec = "k3_ma".into();
        }
        m.geoms.push(g);
    }
    let o = ydr::Opts { standard: true, flat_normal: true, kinds: true };
    let bin = ydr::build_ydr(std::slice::from_ref(&m), [9998.0; 4], None, &o).unwrap();
    let i = read::ydr(&bin).unwrap();
    use ydr::Shader::*;
    let order = [(Standard, 0u8), (Standard, 3), (Decal, 2), (Alpha, 1), (Glass, 1), (Default, 0)];
    let want = vanilla_defs();
    assert_eq!(i.shaders.len(), order.len());
    for (k, s) in i.shaders.iter().enumerate() {
        let (sh, bucket) = order[k];
        let w = want.iter().find(|w| w.shader == sh).unwrap();
        assert_eq!((s.hash, s.bucket, s.bucket_mask), (joaat(w.name), bucket, 1 << bucket), "shader {k}");
        assert_eq!(s.data_size as usize, w.size, "shader {k}");
        assert_eq!(s.buffer_sizes, w.buffers.iter().map(|&x| x as usize).collect::<Vec<_>>(), "shader {k}");
        let texs: Vec<u32> = s.params.iter().filter(|p| p.1 & 3 == 0).map(|p| p.0).collect();
        assert_eq!(texs, w.textures, "shader {k}");
        let smps: Vec<(u32, u8)> = s.params.iter().filter(|p| p.1 & 3 == 2).map(|p| p.0).zip(s.samplers.iter().copied()).collect();
        assert_eq!(smps, w.samplers, "shader {k}");
        assert_eq!(s.cbuffers, w.cbuffers, "shader {k}");
    }
    // the standard family binds a normal map (flat stand-in) and _ma (blank stand-in)
    let bound = |k: usize| i.shaders[k].textures.iter().flatten().cloned().collect::<Vec<_>>();
    assert_eq!(bound(0), vec!["k0_ab", "k0_n", ydr::BLANK_MA, ydr::BLANK_MB]);
    assert_eq!(bound(1), vec!["k3_ab", ydr::FLAT_NORMAL, "k3_ma", ydr::BLANK_MB]);
    for (k, d) in [(2, "k2_ab"), (3, "k1_ab"), (4, "k5_ab")] {
        assert_eq!(bound(k), vec![d, ydr::FLAT_NORMAL, ydr::BLANK_MA, ydr::BLANK_MB], "shader {k}");
    }
    assert_eq!(bound(5), vec!["k4_ab"]);
    // layout: PNXCTT (stride 40) for the standard family, PNCT (28) for default
    assert_eq!(i.geoms.iter().map(|g| g.stride).collect::<Vec<_>>(), vec![40, 40, 40, 40, 40, 28]);
    let slots = |g: usize| i.geoms[g].elements.iter().map(|e| e.0).collect::<Vec<_>>();
    assert_eq!(slots(2), vec![0, 4, 8, 24, 28, 29]);
    assert_eq!(slots(5), vec![0, 4, 24, 28]);
    // per-LOD bucket flags follow the buckets used (0, 1, 2, 3)
    assert_eq!(i.mask_flags[0], 0xFF0F);
    assert_eq!(ydr::diffuse_names(std::slice::from_ref(&m), &o), vec!["k0_ab", "k3_ab", "k2_ab", "k1_ab", "k5_ab", "k4_ab"]);
}

/// kind -> (shader, bucket): `redm prop` (kinds), `convert ydr`
/// (Opts::default) and `--default-only`.
#[test]
fn kind_to_shader_mapping() {
    use ydr::Shader::*;
    let g = |kind: u8, normal: &str| Geom { kind, normal: normal.into(), ..Geom::default() };
    let prop = ydr::Opts { standard: true, flat_normal: true, kinds: true };
    let got: Vec<_> = (0..6).map(|k| ydr::shader_for(&g(k, ""), &prop)).collect();
    assert_eq!(got, vec![(Standard, 0), (Alpha, 1), (Decal, 2), (Standard, 3), (Default, 0), (Glass, 1)]);
    let conv = ydr::Opts::default();
    let got: Vec<_> = (0..6).map(|k| ydr::shader_for(&g(k, "n"), &conv)).collect();
    assert_eq!(got, vec![(Standard, 0), (Default, 1), (Default, 1), (Default, 3), (Default, 0), (Default, 1)]);
    assert_eq!(ydr::shader_for(&g(0, ""), &conv), (Default, 0));
    let default_only = ydr::Opts { standard: false, ..prop };
    let got: Vec<_> = (0..6).map(|k| ydr::shader_for(&g(k, "n"), &default_only)).collect();
    assert_eq!(got, vec![(Default, 0), (Default, 1), (Default, 1), (Default, 3), (Default, 0), (Default, 1)]);
}
