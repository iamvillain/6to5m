//! GTA V (legacy PC) .ydr drawable writer, ported from CodeWalker
//! (Drawable.cs: Drawable/DrawableBase, ShaderGroup, ShaderFX,
//! ShaderParametersBlock, DrawableModelsBlock, DrawableModel,
//! DrawableGeometry, VertexBuffer, IndexBuffer, VertexDeclaration).
//! Output matches obj2ydrxml.py run through CodeWalker's XML importer:
//! shader default.sps, layout Position/Normal/Colour0/TexCoord0 (GTAV1),
//! one geometry per material, one high-LOD model.

use super::rsc::{joaat, BlockId, Builder, Seg, W};
use std::collections::HashMap;

/// One geometry (one shader). `colours` are RGBA with R in the low byte
/// (written to the vertex as bytes R,G,B,A). `uvs` are final (already
/// DirectX top-left, not flipped). `indices` are triangles; at most 65535 vertices.
#[derive(Clone, Default, Debug)]
pub struct Geom {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colours: Vec<u32>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u16>,
    pub texture: String,
    /// GTA V render bucket: 0 opaque (default.sps), 1 alpha (alpha.sps),
    /// 2 decal (decal.sps), 3 cutout (cutout.sps).
    pub kind: u8,
    /// Tangent-space normal map (DirectX green); non-empty switches the
    /// geometry to normal_spec.sps with a flat spec map and adds tangents.
    pub normal: String,
    /// RDR2 only (`rdr2::ydr`): packed material map (`_ma`) of a `standard`
    /// geometry; empty = the vanilla blank. GTA V writers ignore it.
    pub spec: String,
    /// Four-layer ground: layer diffuse names (terrain_cb_w_4lyr.sps,
    /// layout PNCCTX); `colours1` holds the blend weights (G picks layers
    /// 2/3 over 0/1, B picks 1/3 over 0/2).
    pub layers: Option<[String; 4]>,
    /// Layer normal maps (BumpSampler_layerN), same order as `layers`.
    pub layer_bumps: [String; 4],
    pub colours1: Vec<u32>,
}

/// Flat specular map shared by every normal_spec.sps geometry.
pub const FLAT_SPEC: &str = "r6m_flatspec";
/// Flat tangent-space normal map (glass_env without a real bump map).
pub const FLAT_BUMP: &str = "r6m_flatbump";

#[derive(Clone, Default, Debug)]
pub struct Mesh {
    pub name: String,
    pub geoms: Vec<Geom>,
}

const VFT_FILE: u32 = 1079456120;
const VFT_SHADERGROUP: u32 = 1080113136;
const VFT_MODEL: u32 = 1080101528;
const VFT_GEOM: u32 = 1080133528;
const VFT_VB: u32 = 1080153080;
const VFT_IB: u32 = 1080152408;
const GTAV1: u64 = 0x7755555555996996;
const LAYOUT_FLAGS: u32 = 1 | (1 << 3) | (1 << 4) | (1 << 6); // Position Normal Colour0 TexCoord0
const STRIDE: usize = 36;
// Position Normal Colour0 Colour1 TexCoord0 Tangent (float4)
const LAYOUT_PNCCTX: u32 = 1 | (1 << 3) | (1 << 4) | (1 << 5) | (1 << 6) | (1 << 14);
const STRIDE_PNCCTX: usize = 56;

/// + Tangent (Float4): the normal-mapped layout.
const LAYOUT_FLAGS_T: u32 = LAYOUT_FLAGS | (1 << 14);
const STRIDE_T: usize = 52;

/// Shader parameters written for every geometry (obj2ydrxml.py order).
const VECTORS: [(&str, [f32; 4]); 6] = [
    ("matMaterialColorScale", [1.0, 0.0, 0.0, 1.0]),
    ("HardAlphaBlend", [1.0, 0.0, 0.0, 0.0]),
    ("useTessellation", [0.0, 0.0, 0.0, 0.0]),
    ("wetnessMultiplier", [1.0, 0.0, 0.0, 0.0]),
    ("globalAnimUV1", [0.0, 1.0, 0.0, 0.0]),
    ("globalAnimUV0", [1.0, 0.0, 0.0, 0.0]),
];

/// Per-vertex tangents from UV derivatives (w = bitangent sign).
pub fn uv_tangents(p: &[[f32; 3]], nrm: &[[f32; 3]], uv: &[[f32; 2]], idx: &[u32]) -> Vec<[f32; 4]> {
    let n = p.len();
    let (mut t, mut bt) = (vec![[0f32; 3]; n], vec![[0f32; 3]; n]);
    for tri in idx.chunks_exact(3) {
        let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        if a >= n || b >= n || c >= n {
            continue;
        }
        let e1 = [p[b][0] - p[a][0], p[b][1] - p[a][1], p[b][2] - p[a][2]];
        let e2 = [p[c][0] - p[a][0], p[c][1] - p[a][1], p[c][2] - p[a][2]];
        let (du1, dv1, du2, dv2) = (uv[b][0] - uv[a][0], uv[b][1] - uv[a][1], uv[c][0] - uv[a][0], uv[c][1] - uv[a][1]);
        let d = du1 * dv2 - du2 * dv1;
        if d.abs() < 1e-12 {
            continue;
        }
        let r = 1.0 / d;
        for k in 0..3 {
            let sk = (e1[k] * dv2 - e2[k] * dv1) * r;
            let tk = (e2[k] * du1 - e1[k] * du2) * r;
            for i in [a, b, c] {
                t[i][k] += sk;
                bt[i][k] += tk;
            }
        }
    }
    (0..n)
        .map(|i| {
            let nn = nrm[i];
            let d = t[i][0] * nn[0] + t[i][1] * nn[1] + t[i][2] * nn[2];
            let tt = unit3([t[i][0] - nn[0] * d, t[i][1] - nn[1] * d, t[i][2] - nn[2] * d]);
            let c = [nn[1] * tt[2] - nn[2] * tt[1], nn[2] * tt[0] - nn[0] * tt[2], nn[0] * tt[1] - nn[1] * tt[0]];
            let s = if c[0] * bt[i][0] + c[1] * bt[i][1] + c[2] * bt[i][2] < 0.0 { -1.0 } else { 1.0 };
            [tt[0], tt[1], tt[2], s]
        })
        .collect()
}

fn unit3(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-12 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 0.0, 1.0] }
}

fn pad16(o: usize) -> usize {
    (16 - o % 16) % 16
}

fn aabb(pts: impl Iterator<Item = [f32; 3]>) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in pts {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

/// TextureBase reference (legacy, 80 bytes) as CodeWalker builds it from a
/// shader parameter <Name>: VFT 0, Unknown_32h = 2.
fn add_texture_ref(b: &mut Builder, name: &str) -> BlockId {
    let nm = b.string(name);
    let mut w = W::new();
    w.u32(0).u32(1).zeros(32); // VFT, 1, 8h..24h
    w.ptr(Some(nm));
    w.u16(1).u16(2).u32(0).u32(0).u32(0); // 30h, 32h, 34h, 38h, 3Ch
    w.u32(0).u32(0).u32(0).u32(0); // UsageData, 44h, ExtraFlags, 4Ch
    b.add(Seg::Sys, w)
}

/// terrain_cb_w_4lyr.sps with the four layer diffuses and normal maps.
fn add_terrain_shader(b: &mut Builder, layers: &[String; 4], bumps: &[String; 4]) -> BlockId {
    // Parameter order and values as in GTA V's own terrain_cb_w_4lyr
    // drawables (dt1_16_ground.ydr): diffuse/bump pairs per layer, then
    // the vectors in reverse.
    const NAMES: [&str; 8] = [
        "TextureSampler_layer0", "BumpSampler_layer0", "TextureSampler_layer1", "BumpSampler_layer1",
        "TextureSampler_layer2", "BumpSampler_layer2", "TextureSampler_layer3", "BumpSampler_layer3",
    ];
    let vectors = [
        ("materialWetnessMultiplier", [0.5, 0.7, 0.8, 0.6]),
        ("bumpSelfShadowAmount", [0.3, 0.0, 0.0, 0.0]),
        ("bumpiness", [1.0, 0.0, 0.0, 0.0]),
        ("specularIntensityMult", [0.007, 0.0, 0.0, 0.0]),
        ("specularFalloffMult", [5.0, 0.0, 0.0, 0.0]),
    ];
    let names: Vec<&String> = (0..4).flat_map(|i| [&layers[i], &bumps[i]]).collect();
    let texs: Vec<BlockId> = names.iter().map(|t| add_texture_ref(b, t)).collect();
    let nt = texs.len();
    let n = nt + vectors.len();
    let base_size = 32 + 16 * n + 16 * vectors.len() + 4 * n;
    let data_size = base_size + pad16(base_size);
    let pb = b.reserve(Seg::Sys);
    let mut w = W::new();
    for (i, t) in texs.iter().enumerate() {
        w.u8(0).u8((i + 2) as u8).u16(0).u32(0).ptr(Some(*t));
    }
    let mut voff = 16 * n;
    for (i, _) in vectors.iter().enumerate() {
        w.u8(1).u8((160 + (vectors.len() - 1 - i)) as u8).u16(0).u32(0).ptr_at(Some(pb), voff as u64);
        voff += 16;
    }
    for (_, v) in &vectors {
        w.v4(*v);
    }
    for name in NAMES {
        w.u32(joaat(&name.to_ascii_lowercase()));
    }
    for (name, _) in &vectors {
        w.u32(joaat(&name.to_ascii_lowercase()));
    }
    w.zeros(32 + data_size * 4);
    b.set(pb, w);
    let sname = "terrain_cb_w_4lyr";
    let mut w = W::new();
    w.ptr(Some(pb));
    w.u32(joaat(sname)).u32(0);
    w.u8(n as u8).u8(0).u16(32768);
    w.u16((16 * n + 16 * vectors.len()) as u16).u16(data_size as u16);
    w.u32(joaat(&format!("{sname}.sps"))).u32(0);
    w.u32(1 | 0xFF00).u16(0).u8(0).u8(nt as u8).u64(0);
    b.add(Seg::Sys, w)
}

fn add_shader(b: &mut Builder, texture: &str, kind: u8, normal: &str) -> BlockId {
    let (sname, bucket) = match kind {
        0 if !normal.is_empty() => ("normal_spec", 0u8),
        1 => ("alpha", 1u8),
        2 => ("decal", 2),
        3 => ("cutout", 3),
        // self-lit (lamps, signs, screens): emissive.sps, opaque bucket
        4 => ("emissive", 0),
        // 6m glass families: glass_env.sps, alpha bucket
        5 => ("glass_env", 1),
        _ => ("default", 0),
    };
    let mut vectors: Vec<(&str, [f32; 4])> = VECTORS.to_vec();
    if kind == 4 {
        vectors.insert(0, ("emissiveMultiplier", [1.0, 0.0, 0.0, 0.0]));
    }
    let mut samplers = vec![("diffusesampler", texture)];
    if sname == "glass_env" {
        // exact legacy parameter set (CodeWalker ShadersGen9Conversion.xml glass_env "old" names)
        samplers.push(("bumpsampler", if normal.is_empty() { FLAT_BUMP } else { normal }));
        samplers.push(("specsampler", FLAT_SPEC));
        vectors = vec![
            ("globalAnimUV0", [1.0, 0.0, 0.0, 0.0]),
            ("globalAnimUV1", [0.0, 1.0, 0.0, 0.0]),
            ("specularFresnel", [0.97, 0.0, 0.0, 0.0]),
            ("specularFalloffMult", [100.0, 0.0, 0.0, 0.0]),
            ("specularIntensityMult", [1.0, 0.0, 0.0, 0.0]),
            ("specMapIntMask", [1.0, 0.0, 0.0, 0.0]),
            ("bumpiness", [1.0, 0.0, 0.0, 0.0]),
            ("useTessellation", [0.0, 0.0, 0.0, 0.0]),
        ];
    }
    if sname == "normal_spec" {
        samplers.push(("bumpsampler", normal));
        samplers.push(("specsampler", FLAT_SPEC));
        vectors.extend([
            ("specularIntensityMult", [0.25, 0.0, 0.0, 0.0]),
            ("specularFalloffMult", [40.0, 0.0, 0.0, 0.0]),
            ("specularFresnel", [0.97, 0.0, 0.0, 0.0]),
            ("bumpiness", [1.0, 0.0, 0.0, 0.0]),
        ]);
    }
    let texs: Vec<BlockId> = samplers.iter().map(|(_, t)| add_texture_ref(b, t)).collect();
    let nt = texs.len();
    let n = nt + vectors.len();
    // ShaderParametersBlock: params (16 each), vector data, hashes, padding.
    let base_size = 32 + 16 * n + 16 * vectors.len() + 4 * n;
    let data_size = base_size + pad16(base_size);
    let pb = b.reserve(Seg::Sys);
    let mut w = W::new();
    // texture params: DataType 0, Unknown_1h = i + 2
    for (i, t) in texs.iter().enumerate() {
        w.u8(0).u8((i + 2) as u8).u16(0).u32(0).ptr(Some(*t));
    }
    let mut voff = 16 * n;
    for (i, _) in vectors.iter().enumerate() {
        // vector params get 160.. counted from the last one
        let unk = 160 + (vectors.len() - 1 - i);
        w.u8(1).u8(unk as u8).u16(0).u32(0).ptr_at(Some(pb), voff as u64);
        voff += 16;
    }
    for (_, v) in &vectors {
        w.v4(*v);
    }
    for (name, _) in &samplers {
        w.u32(joaat(name));
    }
    for (name, _) in &vectors {
        w.u32(joaat(&name.to_ascii_lowercase()));
    }
    w.zeros(32 + data_size * 4);
    debug_assert_eq!(w.len(), base_size + data_size * 4);
    b.set(pb, w);

    let mut w = W::new();
    w.ptr(Some(pb));
    w.u32(joaat(sname)).u32(0);
    w.u8(n as u8).u8(bucket).u16(32768);
    w.u16((16 * n + 16 * vectors.len()) as u16).u16(data_size as u16);
    w.u32(joaat(&format!("{sname}.sps"))).u32(0);
    w.u32((1u32 << bucket) | 0xFF00).u16(0).u8(0).u8(nt as u8).u64(0);
    b.add(Seg::Sys, w)
}

/// Writes one DrawableGeometry (152 bytes) into `w` and its buffers as blocks.
fn write_geom(b: &mut Builder, w: &mut W, g: &Geom) -> Result<(), String> {
    let nv = g.positions.len();
    if nv > 65535 {
        return Err(format!("geometry has {nv} vertices (max 65535)"));
    }
    if g.normals.len() != nv || g.colours.len() != nv || g.uvs.len() != nv {
        return Err("geometry attribute arrays differ in length".into());
    }
    let terrain = g.layers.is_some();
    if terrain && g.colours1.len() != nv {
        return Err("terrain geometry needs colours1 per vertex".into());
    }
    let bump = terrain || ((!g.normal.is_empty() && g.kind == 0 || g.kind == 5));
    let (flags, stride) = if terrain {
        (LAYOUT_PNCCTX, STRIDE_PNCCTX)
    } else if bump {
        (LAYOUT_FLAGS_T, STRIDE_T)
    } else {
        (LAYOUT_FLAGS, STRIDE)
    };
    let tangents = if bump {
        let idx: Vec<u32> = g.indices.iter().map(|&i| i as u32).collect();
        uv_tangents(&g.positions, &g.normals, &g.uvs, &idx)
    } else {
        Vec::new()
    };
    let mut vb = Vec::with_capacity(nv * stride);
    for i in 0..nv {
        for f in g.positions[i].iter().chain(&g.normals[i]) {
            vb.extend_from_slice(&f.to_le_bytes());
        }
        vb.extend_from_slice(&g.colours[i].to_le_bytes());
        if terrain {
            vb.extend_from_slice(&g.colours1[i].to_le_bytes());
        }
        for f in &g.uvs[i] {
            vb.extend_from_slice(&f.to_le_bytes());
        }
        if let Some(t) = tangents.get(i) {
            for f in t {
                vb.extend_from_slice(&f.to_le_bytes());
            }
        }
    }
    let vdata = b.sys_bytes(vb);
    let mut d = W::new();
    d.u32(flags).u16(stride as u16).u8(0).u8(if terrain { 6 } else if bump { 5 } else { 4 }).u64(GTAV1);
    let decl = b.add(Seg::Sys, d);
    let mut v = W::new();
    v.u32(VFT_VB).u32(1).u16(stride as u16).u16(0).u32(0);
    v.ptr(Some(vdata)).u32(nv as u32).u32(0).ptr(Some(vdata)).u64(0).ptr(Some(decl));
    v.zeros(128 - v.len());
    let vbuf = b.add(Seg::Sys, v);

    let mut ib = Vec::with_capacity(g.indices.len() * 2);
    for i in &g.indices {
        ib.extend_from_slice(&i.to_le_bytes());
    }
    let idata = b.sys_bytes(ib);
    let mut x = W::new();
    x.u32(VFT_IB).u32(1).u32(g.indices.len() as u32).u32(0).ptr(Some(idata));
    x.zeros(96 - x.len());
    let ibuf = b.add(Seg::Sys, x);

    let ni = g.indices.len() as u32;
    let start = w.len();
    w.u32(VFT_GEOM).u32(1).u64(0).u64(0).ptr(Some(vbuf)).u64(0).u64(0).u64(0);
    w.ptr(Some(ibuf)).u64(0).u64(0).u64(0);
    w.u32(ni).u32(ni / 3).u16(nv as u16).u16(3).u32(0);
    w.u64(0).u16(stride as u16).u16(0).u32(0);
    w.ptr(Some(vdata)).u64(0).u64(0).u64(0);
    debug_assert_eq!(w.len() - start, 152);
    Ok(())
}

/// DrawableModelsBlock with a single high-LOD model holding every geometry.
fn add_models(b: &mut Builder, mesh: &Mesh, aabbs: &[([f32; 3], [f32; 3])], sh_base: usize) -> Result<(BlockId, usize), String> {
    let id = b.reserve(Seg::Sys);
    let n = mesh.geoms.len();
    let mut w = W::new();
    // ResourcePointerListHeader + one model pointer
    w.ptr_at(Some(id), 16).u16(1).u16(1).u32(0);
    let model_off = 16 + 8 + pad16(16 + 8);
    w.ptr_at(Some(id), model_off as u64);
    w.align(16);
    // DrawableModel
    let mut off = model_off + 48;
    let shmap = off;
    off += n * 2;
    off += if n == 1 { 6 } else { pad16(off) };
    let geoptrs = off;
    off += n * 8;
    off += pad16(off);
    let bounds = off;
    let nb = n + usize::from(n > 1);
    off += nb * 32;
    let mut gofs = Vec::with_capacity(n);
    for _ in 0..n {
        off += pad16(off);
        gofs.push(off);
        off += 152;
    }
    w.u32(VFT_MODEL).u32(1).ptr_at(Some(id), geoptrs as u64);
    w.u16(n as u16).u16(n as u16).u32(0);
    w.ptr_at(Some(id), bounds as u64).ptr_at(Some(id), shmap as u64);
    w.u32(0).u16(0x00FF).u16(n as u16); // SkeletonBinding, RenderMask 255 / Flags 0
    for i in 0..n {
        w.u16((sh_base + i) as u16);
    }
    if n == 1 {
        w.zeros(6);
    } else {
        w.align(16);
    }
    for &g in &gofs {
        w.ptr_at(Some(id), g as u64);
    }
    w.align(16);
    let mut bl = Vec::new();
    if n > 1 {
        let (lo, hi) = aabb(aabbs.iter().flat_map(|a| [a.0, a.1]));
        bl.push((lo, hi));
    }
    bl.extend_from_slice(aabbs);
    for (lo, hi) in bl {
        w.v3(lo).f32(0.0).v3(hi).f32(0.0);
    }
    for g in &mesh.geoms {
        w.align(16);
        write_geom(b, &mut w, g)?;
    }
    let len = w.len();
    b.set(id, w);
    Ok((id, len))
}

/// Add a Drawable (208 bytes) and everything it references. The drawable
/// block is reserved first, so on an empty builder it becomes the root.
/// `file_root` adds the ResourcePagesInfo block a standalone .ydr needs.
/// `tex_dict` is an embedded texture dictionary block (ytd::add_dict).
pub fn add_drawable(b: &mut Builder, mesh: &Mesh, lod_dist: f32, tex_dict: Option<BlockId>, file_root: bool) -> Result<BlockId, String> {
    let id = b.reserve(Seg::Sys);
    fill_drawable(b, id, mesh, lod_dist, tex_dict, file_root)?;
    Ok(id)
}

/// Like `add_drawable`, into a block already reserved with `reserve(Seg::Sys)`.
pub fn fill_drawable(b: &mut Builder, id: BlockId, mesh: &Mesh, lod_dist: f32, tex_dict: Option<BlockId>, file_root: bool) -> Result<(), String> {
    fill_drawable_lods(b, id, std::slice::from_ref(mesh), [lod_dist; 4], tex_dict, file_root)
}

/// Drawable with up to four LOD levels (High, Med, Low, VLow: LodGroup
/// model lists +0x50..+0x68, distances +0x70..+0x7C, render masks +0x80),
/// e.g. the source model's own levels and distances. `levels[0]` is High
/// and sets name and bounds; empty levels after it are skipped.
pub fn fill_drawable_lods(b: &mut Builder, id: BlockId, levels: &[Mesh], dists: [f32; 4], tex_dict: Option<BlockId>, file_root: bool) -> Result<(), String> {
    let mesh = levels.first().ok_or("no LOD levels")?;
    if mesh.geoms.is_empty() {
        return Err("mesh has no geometries".into());
    }
    let pages = if file_root {
        let mut p = W::new();
        p.u32(0).u32(0).u8(128).u8(0).u16(0).u32(0).zeros(8 * 128);
        let p = b.add(Seg::Sys, p);
        b.pages_info = Some(p);
        Some(p)
    } else {
        None
    };
    let name = b.string(&mesh.name);

    // shader group
    let shaders: Vec<BlockId> = levels.iter().take(4).flat_map(|l| l.geoms.iter()).map(|g| match &g.layers { Some(l) => add_terrain_shader(b, l, &g.layer_bumps), None => add_shader(b, &g.texture, g.kind, &g.normal) }).collect();
    let mut pa = W::new();
    for &s in &shaders {
        pa.ptr(Some(s));
    }
    let parr = b.add(Seg::Sys, pa);
    let mut sg = W::new();
    sg.u32(VFT_SHADERGROUP).u32(1).ptr(tex_dict).ptr(Some(parr));
    sg.u16(shaders.len() as u16).u16(shaders.len() as u16).u32(0).u64(0).u64(0);
    sg.u32(64 / 16).u32(0).u64(0);
    let sgid = b.add(Seg::Sys, sg);

    // bounds
    let aabbs: Vec<_> = mesh.geoms.iter().map(|g| aabb(g.positions.iter().copied())).collect();
    let (lo, hi) = aabb(aabbs.iter().flat_map(|a| [a.0, a.1]));
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
    let mut r2 = 0f64;
    for g in &mesh.geoms {
        for p in &g.positions {
            let d: f64 = (0..3).map(|k| ((p[k] - c[k]) as f64).powi(2)).sum();
            r2 = r2.max(d);
        }
    }
    let (models, mlen) = add_models(b, mesh, &aabbs, 0)?;
    let mut lod_models = [Some(models), None, None, None];
    let mut base = mesh.geoms.len();
    for (k, l) in levels.iter().enumerate().take(4).skip(1) {
        if l.geoms.is_empty() {
            continue;
        }
        let ab: Vec<_> = l.geoms.iter().map(|g| aabb(g.positions.iter().copied())).collect();
        lod_models[k] = Some(add_models(b, l, &ab, base)?.0);
        base += l.geoms.len();
    }

    let mut w = W::new();
    w.u32(VFT_FILE).u32(1).ptr(pages);
    w.ptr(Some(sgid)).u64(0);
    w.v3(c).f32(r2.sqrt() as f32);
    w.v3(lo).u32(0x7f800001).v3(hi).u32(0x7f800001);
    for m in lod_models {
        w.ptr(m);
    }
    for d in dists {
        w.f32(d);
    }
    for m in lod_models {
        w.u32(if m.is_some() { 0xFF01 } else { 0 }); // Flags 1, RenderMask 255
    }
    w.u64(0).u16(0).u16(mlen.div_ceil(16) as u16).u32(0);
    w.ptr(Some(models));
    w.ptr(Some(name)).u64(0).u16(0).u16(0).u32(0).u64(0).u64(0);
    debug_assert_eq!(w.len(), 208);
    b.set(id, w);
    Ok(())
}

/// Build a standalone .ydr (resource version 165). `embed` is a builder
/// hook that adds an embedded texture dictionary and returns its block.
pub fn build_ydr_lods(levels: &[Mesh], dists: [f32; 4], embed: Option<&dyn Fn(&mut Builder) -> Result<BlockId, String>>) -> Result<Vec<u8>, String> {
    let mut b = Builder::new();
    let root = b.reserve(Seg::Sys);
    let dict = embed.map(|e| e(&mut b)).transpose()?;
    fill_drawable_lods(&mut b, root, levels, dists, dict, true)?;
    b.build(165)
}

pub fn build_ydr(mesh: &Mesh, lod_dist: f32, embed: Option<&dyn Fn(&mut Builder) -> Result<BlockId, String>>) -> Result<Vec<u8>, String> {
    let mut b = Builder::new();
    let root = b.reserve(Seg::Sys); // the drawable must be the root block
    let dict = embed.map(|e| e(&mut b)).transpose()?;
    fill_drawable(&mut b, root, mesh, lod_dist, dict, true)?;
    b.build(165)
}

/// Parse a .mtl: material -> map_Kd file stem.
pub fn parse_mtl(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        let q: Vec<&str> = line.split_whitespace().collect();
        match q.first() {
            Some(&"newmtl") if q.len() > 1 => cur = Some(q[1].to_string()),
            Some(&"map_Kd") if q.len() > 1 => {
                if let Some(m) = &cur {
                    let f = q[1];
                    let stem = std::path::Path::new(f).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    out.insert(m.clone(), stem);
                }
            }
            _ => {}
        }
    }
    out
}

/// OBJ -> Mesh with obj2ydrxml.py semantics: per-usemtl geometry (split at
/// 65535 vertices), averaged face normals over source vertices, vertex
/// colours from `v x y z r g b`, V flipped, texture `r6m_<map_Kd stem
/// lowercase, 0x removed>` or `r6m_missing`. `mtl` maps material -> stem.
pub fn obj_to_mesh(obj_text: &str, mtl: &HashMap<String, String>) -> Mesh {
    let mut v: Vec<[f32; 3]> = Vec::new();
    let mut vc: Vec<[f32; 3]> = Vec::new();
    let mut vt: Vec<[f32; 2]> = Vec::new();
    let mut order: Vec<Option<String>> = Vec::new();
    let mut groups: HashMap<Option<String>, Vec<[(i64, i64); 3]>> = HashMap::new();
    let mut cur: Option<String> = None;
    let fl = |s: &str| s.parse::<f32>().unwrap_or(0.0);
    for line in obj_text.lines() {
        let p: Vec<&str> = line.split_whitespace().collect();
        match p.first().copied() {
            Some("v") if p.len() >= 4 => {
                v.push([fl(p[1]), fl(p[2]), fl(p[3])]);
                vc.push(if p.len() >= 7 { [fl(p[4]), fl(p[5]), fl(p[6])] } else { [1.0; 3] });
            }
            Some("vt") if p.len() >= 3 => vt.push([fl(p[1]), fl(p[2])]),
            Some("usemtl") if p.len() >= 2 => {
                cur = Some(p[1].to_string());
                if !groups.contains_key(&cur) {
                    order.push(cur.clone());
                    groups.insert(cur.clone(), Vec::new());
                }
            }
            Some("f") if p.len() >= 4 => {
                let corner = |c: &str| {
                    let mut it = c.split('/');
                    let a = it.next().and_then(|x| x.parse::<i64>().ok()).map(|x| x - 1).unwrap_or(-1);
                    let t = it.next().and_then(|x| x.parse::<i64>().ok()).map(|x| x - 1).unwrap_or(-1);
                    (a, t)
                };
                if !groups.contains_key(&cur) {
                    order.push(cur.clone());
                    groups.insert(cur.clone(), Vec::new());
                }
                groups.get_mut(&cur).unwrap().push([corner(p[1]), corner(p[2]), corner(p[3])]);
            }
            _ => {}
        }
    }
    // averaged (area-weighted) normals per source vertex
    let mut nrm = vec![[0f32; 3]; v.len()];
    for faces in groups.values() {
        for f in faces {
            let (a, b, c) = (f[0].0 as usize, f[1].0 as usize, f[2].0 as usize);
            if a >= v.len() || b >= v.len() || c >= v.len() {
                continue;
            }
            let e1 = [v[b][0] - v[a][0], v[b][1] - v[a][1], v[b][2] - v[a][2]];
            let e2 = [v[c][0] - v[a][0], v[c][1] - v[a][1], v[c][2] - v[a][2]];
            let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            for &i in &[a, b, c] {
                for k in 0..3 {
                    nrm[i][k] += n[k];
                }
            }
        }
    }
    let norm = |n: [f32; 3]| {
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        let l = if l == 0.0 { 1.0 } else { l };
        [n[0] / l, n[1] / l, n[2] / l]
    };
    let texname = |m: &Option<String>| match m.as_ref().and_then(|m| mtl.get(m)) {
        Some(st) => format!("r6m_{}", st.to_lowercase().replace("0x", "")),
        None => "r6m_missing".to_string(),
    };
    let mut geoms = Vec::new();
    for m in &order {
        let tex = texname(m);
        let mut g = Geom { texture: tex.clone(), ..Default::default() };
        let mut remap: HashMap<(i64, i64), u16> = HashMap::new();
        for f in &groups[m] {
            if f.iter().any(|c| c.0 < 0 || c.0 as usize >= v.len()) {
                continue;
            }
            let new = f.iter().filter(|c| !remap.contains_key(c)).count();
            if g.positions.len() + new > 65535 {
                geoms.push(std::mem::replace(&mut g, Geom { texture: tex.clone(), ..Default::default() }));
                remap.clear();
            }
            for c in f {
                let idx = *remap.entry(*c).or_insert_with(|| {
                    let vi = c.0 as usize;
                    g.positions.push(v[vi]);
                    g.normals.push(norm(nrm[vi]));
                    let col = vc[vi].map(|x| (x * 255.0).round().clamp(0.0, 255.0) as u32);
                    g.colours.push(col[0] | col[1] << 8 | col[2] << 16 | 0xFF00_0000);
                    let (u, w) = if c.1 >= 0 && (c.1 as usize) < vt.len() { (vt[c.1 as usize][0], vt[c.1 as usize][1]) } else { (0.0, 0.0) };
                    g.uvs.push([u, 1.0 - w]);
                    (g.positions.len() - 1) as u16
                });
                g.indices.push(idx);
            }
        }
        if !g.indices.is_empty() {
            geoms.push(g);
        }
    }
    Mesh { name: String::new(), geoms }
}

/// Test path: rebuild a Mesh from a CodeWalker YDR XML dump (GTAV1
/// Position/Normal/Colour0/TexCoord0 layout only).
fn cwxml_to_mesh(xml: &str) -> Result<Mesh, String> {
    let mut texes = Vec::new();
    let mut in_shader_tex = false;
    for line in xml.lines() {
        let t = line.trim();
        if t.starts_with("<Item name=\"DiffuseSampler\"") {
            in_shader_tex = true;
        } else if in_shader_tex && t.starts_with("<Name>") {
            texes.push(t.trim_start_matches("<Name>").trim_end_matches("</Name>").to_string());
            in_shader_tex = false;
        }
    }
    let mut geoms = Vec::new();
    let mut g = Geom::default();
    let mut mode = 0; // 1 vertex data, 2 index data
    let mut in_ib = false;
    for line in xml.lines() {
        let t = line.trim();
        if let Some(r) = t.strip_prefix("<ShaderIndex value=\"") {
            let si: usize = r.split('"').next().unwrap_or("0").parse().unwrap_or(0);
            g = Geom { texture: texes.get(si).cloned().unwrap_or_default(), ..Default::default() };
        } else if t == "<IndexBuffer>" {
            in_ib = true;
        } else if t == "<Data>" {
            mode = if in_ib { 2 } else { 1 };
        } else if t == "</Data>" {
            mode = 0;
        } else if t == "</IndexBuffer>" {
            in_ib = false;
            geoms.push(std::mem::take(&mut g));
        } else if mode == 1 {
            let f: Vec<f32> = t.split_whitespace().map(|x| x.parse().unwrap_or(0.0)).collect();
            if f.len() != 12 {
                return Err(format!("unexpected vertex line: {t}"));
            }
            g.positions.push([f[0], f[1], f[2]]);
            g.normals.push([f[3], f[4], f[5]]);
            g.colours.push(f[6] as u32 | (f[7] as u32) << 8 | (f[8] as u32) << 16 | (f[9] as u32) << 24);
            g.uvs.push([f[10], f[11]]);
        } else if mode == 2 {
            g.indices.extend(t.split_whitespace().map(|x| x.parse::<u16>().unwrap_or(0)));
        }
    }
    Ok(Mesh { name: String::new(), geoms })
}

pub fn cli(args: &[String]) -> Result<(), String> {
    let mut pos = Vec::new();
    let mut ytd = None;
    let mut cwxml = false;
    let mut lod = 9998.0f32;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--ytd" => {
                ytd = Some(args.get(i + 1).ok_or("--ytd needs a path")?.clone());
                i += 1;
            }
            "--lod" => {
                lod = args.get(i + 1).and_then(|s| s.parse().ok()).ok_or("--lod needs a number")?;
                i += 1;
            }
            "--from-cwxml" => cwxml = true,
            a => pos.push(a.to_string()),
        }
        i += 1;
    }
    if pos.len() != 2 {
        return Err("usage: ydr <in.obj> <out.ydr> [--ytd <embed.ytd>] [--lod <dist>]".into());
    }
    if ytd.is_some() {
        return Err("ydr: --ytd embedding not wired yet".into());
    }
    let (src, dst) = (std::path::Path::new(&pos[0]), std::path::Path::new(&pos[1]));
    let text = std::fs::read_to_string(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let mut mesh = if cwxml {
        cwxml_to_mesh(&text)?
    } else {
        let mut mtl = HashMap::new();
        if let Some(lib) = text.lines().find_map(|l| l.trim().strip_prefix("mtllib ")) {
            let p = src.parent().unwrap_or(std::path::Path::new(".")).join(lib.trim());
            if let Ok(t) = std::fs::read_to_string(&p) {
                mtl = parse_mtl(&t);
            }
        }
        obj_to_mesh(&text, &mtl)
    };
    mesh.name = dst.file_name().map(|s| s.to_string_lossy().split('.').next().unwrap_or("").to_string()).unwrap_or_default();
    let data = build_ydr(&mesh, lod, None)?;
    std::fs::write(dst, &data).map_err(|e| format!("{}: {e}", dst.display()))?;
    let nv: usize = mesh.geoms.iter().map(|g| g.positions.len()).sum();
    println!("{}: {} geometries, {} verts, {} bytes", dst.display(), mesh.geoms.len(), nv, data.len());
    Ok(())
}
