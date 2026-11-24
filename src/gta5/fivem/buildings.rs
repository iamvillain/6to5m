//! Buildings/props (buildings_port.py): sixng world placements -> one ydr
//! per archetype (shared texture dictionaries), t6m_bld.ytyp (bounds from
//! the mesh, textureDictionary set) and 512 m t6m_bld_XX_YY.ymap cells.

use super::geom::{aabb, push_geoms, push_geoms_n, V3};
#[path = "interiors.rs"]
mod interiors;
pub(crate) use interiors::{interior_list, interior_name, resolvable as interior_resolvable, source as interior_source, InteriorSource};
#[cfg(test)]
pub(crate) use interiors::interior_hash;
#[path = "lodlights.rs"]
mod lodlights;
#[path = "texopt.rs"]
mod texopt;
use super::{safe_name, sha, Ctx, FRAG_V, TEX_V, YDR_V};
use crate::gta5::{
    meta::{self, Archetype, Entity, MapData, MapTypes},
    ydr::{self, Mesh},
    ytd::{self, Tex, TexFormat},
};
use memmap2::Mmap;
use rayon::prelude::*;
use serde_json::{json, Value};
use sixng_core::scan::{ArchiveIndex, Entry};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Texture dictionary cap: graphics bytes after RSC7 page packing
/// (rsc::gfx_pages_size), what FiveM checks against its 16 MB streaming
/// limit. Was compared with the plain sum of DDS sizes, which packed to
/// 18-20 MB (power-of-two pages): 646 ytds evicted, blurry buildings.
const TXD_CAP: u64 = 15 << 20;
/// System pages of a texture dictionary (pages info 1 KB + 144 B and a name
/// per texture): budgeted from the 128-page total.
const TXD_SYS_PAGES: u32 = 4;
/// A family dictionary below this many bytes is not closed at the end of
/// its family: following families are packed into it (fewer, larger files
/// to download and mount per region).
const TXD_PACK: u64 = 4 << 20;
const MISSING: &str = "r6m_missing";
/// Engine default texture in colour slots of every 6m shader family, held by
/// no store or resource of the install (SixNG export.rs ENGINE_DEFAULTS:
/// white). Terminal railings (ap_06_terminal_d shader 63, tinted) bind only
/// it, so they resolved to nothing and drew magenta.
const ENGINE_WHITE: u64 = 0x1e2c_9340_e14a_7d07;

/// 6m render bucket bits -> GTA V shader kind (ydr::Geom::kind).
/// DXT1 (opaque) or DXT5 (with alpha) DDS with a box-filtered mip chain,
/// compressed in pure Rust (texpresso).
/// A TTPF texture's own data as a DX10 DDS (native format, all mips,
/// slice 0), for formats the ytd writer takes. None for arrays, split
/// 2-byte layouts or other formats (callers fall back to RGBA8).
fn native_dds(t: &sixng_core::ttpf::TtpfTex, data: &[u8]) -> Option<Vec<u8>> {
    use sixng_core::texture::Fmt;
    if t.split_2b || t.slices() != 1 || !matches!(t.dxgi_format, 70..=78 | 79 | 80 | 82 | 83 | 97..=99 | 27..=29 | 87 | 90 | 91) {
        return None;
    }
    let fmt = Fmt::from_dxgi(t.dxgi_format);
    if fmt == Fmt::Unknown {
        return None;
    }
    let l = t.layout()?;
    let (bd, bpe) = (fmt.block_dim().max(1), fmt.bytes_per_elem() as usize);
    let bc = bd > 1;
    let mips = l.mips.len() as u32;
    let blocks = |m: u32| (((t.width >> m).max(1)).div_ceil(bd) as usize, ((t.height >> m).max(1)).div_ceil(bd) as usize);
    let (bx0, by0) = blocks(0);
    let mut out = Vec::new();
    let mut w = |v: u32| out.extend_from_slice(&v.to_le_bytes());
    w(u32::from_le_bytes(*b"DDS "));
    w(124);
    w(if bc { 0xA1007 } else { 0x2100F });
    w(t.height);
    w(t.width);
    w(if bc { (bx0 * by0 * bpe) as u32 } else { (bx0 * bpe) as u32 });
    w(0);
    w(mips);
    for _ in 0..11 {
        w(0);
    }
    w(32);
    w(4);
    w(u32::from_le_bytes(*b"DX10"));
    for _ in 0..5 {
        w(0);
    }
    w(if mips > 1 { 0x401008 } else { 0x1000 });
    for _ in 0..4 {
        w(0);
    }
    w(t.dxgi_format);
    w(3);
    w(0);
    w(1);
    w(0);
    for m in 0..mips {
        let (bx, by) = blocks(m);
        let p = &l.mips[m as usize];
        let lin = l.untile_slice(data, m as usize, 0);
        let (pw, ph) = (p.w as usize, p.h as usize);
        for y in 0..by {
            if y < ph {
                let n = bx.min(pw);
                out.extend_from_slice(&lin[y * pw * bpe..(y * pw + n) * bpe]);
                out.resize(out.len() + (bx - n) * bpe, 0);
            } else {
                out.resize(out.len() + bx * bpe, 0);
            }
        }
    }
    Some(out)
}

fn rgba_dds(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut levels = vec![(rgba.to_vec(), w, h)];
    while levels.len() < 12 {
        let (src, sw, sh) = levels.last().unwrap();
        let (sw, sh) = (*sw, *sh);
        if sw < 8 || sh < 8 {
            break;
        }
        let (nw, nh) = (sw / 2, sh / 2);
        let mut d = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let at = |xx: u32, yy: u32| src[((yy * sw + xx) * 4 + c) as usize] as u32;
                    d[((y * nw + x) * 4 + c) as usize] = ((at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1) + 2) / 4) as u8;
                }
            }
        }
        levels.push((d, nw, nh));
    }
    let opaque = rgba.chunks_exact(4).all(|p| p[3] == 255);
    let (fmt, cc) = if opaque { (texpresso::Format::Bc1, *b"DXT1") } else { (texpresso::Format::Bc3, *b"DXT5") };
    let u = |v: u32, b: &mut Vec<u8>| b.extend_from_slice(&v.to_le_bytes());
    let mut b = Vec::new();
    b.extend_from_slice(b"DDS ");
    u(124, &mut b);
    u(0x1 | 0x2 | 0x4 | 0x1000 | 0x20000 | 0x80000, &mut b);
    u(h, &mut b);
    u(w, &mut b);
    u(fmt.compressed_size(w as usize, h as usize) as u32, &mut b);
    u(0, &mut b);
    u(levels.len() as u32, &mut b);
    for _ in 0..11 {
        u(0, &mut b);
    }
    u(32, &mut b);
    u(0x4, &mut b);
    b.extend_from_slice(&cc);
    for _ in 0..5 {
        u(0, &mut b);
    }
    u(0x1000 | 0x400000 | 0x8, &mut b);
    for _ in 0..4 {
        u(0, &mut b);
    }
    for (px, lw, lh) in &levels {
        b.extend_from_slice(&compress_par(fmt, px, *lw as usize, *lh as usize));
    }
    b
}

/// Block compression in parallel: strips of 16 block rows (64 px) are
/// independent, so they compress on all cores and are concatenated.
fn compress_par(fmt: texpresso::Format, px: &[u8], w: usize, h: usize) -> Vec<u8> {
    const ROWS: usize = 64;
    if h <= ROWS || h % 4 != 0 {
        let mut out = vec![0u8; fmt.compressed_size(w, h)];
        fmt.compress(px, w, h, texpresso::Params::default(), &mut out);
        return out;
    }
    let strips: Vec<Vec<u8>> = (0..h.div_ceil(ROWS))
        .into_par_iter()
        .map(|i| {
            let (y0, y1) = (i * ROWS, ((i + 1) * ROWS).min(h));
            let mut out = vec![0u8; fmt.compressed_size(w, y1 - y0)];
            fmt.compress(&px[y0 * w * 4..y1 * w * 4], w, y1 - y0, texpresso::Params::default(), &mut out);
            out
        })
        .collect();
    strips.concat()
}

/// Rejects decodes that cannot be a colour map: flat images (BCPack data
/// sixng cannot decode yet comes out as one solid colour, seen in game as
/// solid blue/magenta props) and tangent-space normal maps.
fn bad_colour(rgba: &[u8]) -> Option<&'static str> {
    let n = (rgba.len() / 4).max(1) as f64;
    let (mut s, mut q) = ([0f64; 4], [0f64; 4]);
    for px in rgba.chunks_exact(4) {
        for k in 0..4 {
            let v = px[k] as f64;
            s[k] += v;
            q[k] += v * v;
        }
    }
    let m = s.map(|x| x / n);
    let var: f64 = (0..3).map(|k| q[k] / n - m[k] * m[k]).sum();
    // Undecoded data is one solid colour, alpha included. A near-white
    // texture whose shape is all in alpha is real (6m d960fe1b35c3c4c2, a
    // fluorescent diffuser: RGB variance 0.3, alpha sigma 120); it was
    // rejected as flat, 284 times in one build.
    let alpha_var = q[3] / n - m[3] * m[3];
    // Undecoded BCPack comes out as one solid colour (variance ~0). Real
    // near-uniform maps exist: 6m glass diffusetex has variance ~10.
    if var < 0.5 && alpha_var < 0.5 {
        return Some("flat decode (undecoded BCPack data)");
    }
    if m[2] > 200.0 && (100.0..160.0).contains(&m[0]) && (100.0..160.0).contains(&m[1]) {
        return Some("normal map, not a colour map");
    }
    // Packed masks (blend weights, noise) have independent channels; colour
    // maps are luminance-dominated, with R/G/B strongly correlated.
    let corr = |a: usize, b: usize| {
        let (mut sab, mut saa, mut sbb) = (0f64, 0f64, 0f64);
        for px in rgba.chunks_exact(4) {
            let (x, y) = (px[a] as f64 - m[a], px[b] as f64 - m[b]);
            sab += x * y;
            saa += x * x;
            sbb += y * y;
        }
        if saa < 1.0 || sbb < 1.0 { 1.0 } else { sab / (saa * sbb).sqrt() }
    };
    // (no channel-correlation test: slots are chosen by hash, and colourful
    // colour maps are decorrelated too)
    let _ = corr;
    None
}

/// 6m paints ground tiles as stacked decal layers blended by alpha masks.
/// GTA needs an opaque surface underneath: the decal texture covering the
/// most area becomes opaque (when it covers at least half the footprint and
/// the mesh has no opaque ground already); every other layer stays an
/// alpha-blended decal.
fn promote_base_layer(mesh: &mut Mesh) {
    let area = |g: &ydr::Geom| -> f64 {
        g.indices
            .chunks_exact(3)
            .map(|t| {
                let (a, b, c) = (g.positions[t[0] as usize], g.positions[t[1] as usize], g.positions[t[2] as usize]);
                let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - a[0], c[1] - a[1]]);
                ((u[0] * v[1] - u[1] * v[0]) as f64).abs() / 2.0
            })
            .sum()
    };
    let mut by_tex: HashMap<String, f64> = HashMap::new();
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for g in &mesh.geoms {
        for p in &g.positions {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        if g.kind == 2 && g.texture != MISSING {
            *by_tex.entry(g.texture.clone()).or_default() += area(g);
        }
    }
    let foot = ((hi[0] - lo[0]) * (hi[1] - lo[1])).max(0.0) as f64;
    let opaque: f64 = mesh.geoms.iter().filter(|g| g.kind == 0).map(area).sum();
    if let Some((t, a)) = by_tex.into_iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
        if a >= foot * 0.5 && opaque < foot * 0.5 {
            for g in mesh.geoms.iter_mut().filter(|g| g.kind == 2 && g.texture == t) {
                g.kind = 0;
            }
        }
    }
}

/// True when a texture is (near) grayscale: mean per-pixel channel spread
/// under 4 levels.
fn near_gray(pools: &Pools, p: &std::path::Path) -> bool {
    let Some((_, _, rgba)) = pools.load(p).ok().and_then(|b| crate::texture::dds_preview(&b).ok()) else { return false };
    let n = (rgba.len() / 4).max(1) as f64;
    let spread: f64 = rgba.chunks_exact(4).map(|px| (px[0].max(px[1]).max(px[2]) - px[0].min(px[1]).min(px[2])) as f64).sum();
    spread / n < 4.0
}

/// Rejects a DDS that is not a usable colour map.
fn check_dds(bytes: &[u8]) -> Result<(), String> {
    let (w, h, rgba) = crate::texture::dds_preview(bytes).map_err(|e| e.to_string())?;
    let why = if w <= 8 && h <= 8 { Some("placeholder (8 px or less)") } else { bad_colour(&rgba) };
    match why {
        None => Ok(()),
        Some(why) => Err(why.into()),
    }
}

/// Self-lit shader family (lamps, signs, screens; sixng-core material
/// families, from SixNG 4a1bd3d): written as GTA V emissive.sps.
fn is_glass(shader_hash: u32) -> bool {
    sixng_core::material::classify(&format!("{shader_hash:08x}"), &[])["class"] == "glass"
}

/// Shared flat normal map (glass_env when the 6m bump slot is a placeholder).
fn flat_bump(pools: &Pools) -> Result<String, String> {
    let p = pools.dir.join(format!("{}.dds", &ydr::FLAT_BUMP[4..]));
    if !pools.store.exists(&p) {
        pools.store.put(&p, rgba_dds(&[128u8, 128, 255, 255].repeat(16 * 16), 16, 16), super::texmem::Src::FlatBump);
    }
    Ok(ydr::FLAT_BUMP.into())
}

fn is_emissive(shader_hash: u32) -> bool {
    sixng_core::material::classify(&format!("{shader_hash:08x}"), &[])["class"] == "emissive"
}

/// Tangent-space normal map parameter hashes (sixng-core export NORMAL_PARAMS).
const NORMAL_SLOTS: [u32; 3] = [0x84c69350, 0x58b6e707, 0x1e77bf4c];

fn kind_of(bucket: u8) -> u8 {
    match bucket {
        x if x & 0x04 != 0 => 2,
        x if x & 0x42 != 0 => 1,
        x if x & 0x08 != 0 => 3,
        _ => 0,
    }
}

/// Diffuse/colour parameter hashes in priority order (model::COLOR_SLOTS).
const COLOR_SLOTS: [u32; 17] = [
    3370697346, 756347250, 0x346394EA, 0x7D4F12E2, 3464205966, 3632507550, 851042057, 1024521143, 3853173992, 2464948076, 3389132183, 3547013225,
    234123336, 3831897388, 3489704073, 1380592926,
    // dirtdiffusetex, last: glazing and railings whose only 6m texture is the
    // grime layer (opacity in alpha; GTA6-Asset-Info 12-materials.md)
    0x7485DAFB,
];

#[derive(Clone)]
struct Place {
    name: String,
    off: usize,
    pos: V3,
    rot: [f32; 4],
    sxy: f32,
    sz: f32,
    /// (map id, entity index) in the source placement file (world files only).
    key: Option<(String, usize)>,
    /// 0 = HD/orphan HD, 1 = LOD, 2 = SLOD1, 3 = SLOD2, ...
    level: u8,
    elod: f32,
    child_lod: f32,
    /// the 6m entity has a LOD parent (lod.parent_index or parent_instance)
    src_parent: bool,
}

fn level_of(s: &str) -> u8 {
    match s {
        "LOD" => 1,
        x if x.starts_with("SLOD") => 1 + x[4..].parse::<u8>().unwrap_or(1),
        _ => 0,
    }
}

const LEVEL_NAMES: [&str; 6] = ["LODTYPES_DEPTH_HD", "LODTYPES_DEPTH_LOD", "LODTYPES_DEPTH_SLOD1", "LODTYPES_DEPTH_SLOD2", "LODTYPES_DEPTH_SLOD3", "LODTYPES_DEPTH_SLOD4"];

fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
fn qrot(q: [f32; 4], v: V3) -> V3 {
    let r = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), [-q[0], -q[1], -q[2], q[3]]);
    [r[0], r[1], r[2]]
}
fn q4(v: &Value) -> Option<[f32; 4]> {
    let a = v.as_array()?;
    (a.len() == 4).then(|| std::array::from_fn(|k| a[k].as_f64().unwrap_or(0.0) as f32))
}

/// Keyed textures from the ttdb streaming pools and TTPF stores, decoded in
/// memory (fivem/texmem.rs); `dir` is only the virtual base of their
/// `<id>.dds` names.
struct Pools {
    game: PathBuf,
    catalog: Option<crate::texture::Catalog>,
    maps: Mutex<HashMap<PathBuf, Arc<Mmap>>>,
    dir: PathBuf,
    store: super::texmem::Store,
}

impl Pools {
    /// Pools over <game>/data/ttdb (+ TTPF stores) with records in `store`.
    fn new(game: &Path, store: super::texmem::Store) -> Pools {
        let ttdb = game.join("data").join("ttdb");
        Pools { game: game.to_path_buf(), catalog: crate::texture::Catalog::build(&ttdb).map_err(|e| eprintln!("{}: {e}", ttdb.display())).ok(), maps: Mutex::new(HashMap::new()), dir: PathBuf::from("texmem"), store }
    }
    fn path(&self, hash: u32) -> PathBuf {
        self.dir.join(format!("{hash:08x}.dds"))
    }
    /// Slot of a texture (fivem/texslot.rs): its 32-bit hash unless another
    /// texture owns it.
    fn slot(&self, suffix: &str, hash: u32, key: u64, h64: u64) -> u32 {
        super::texslot::slot(&self.store, suffix, hash, key, h64)
    }

    /// DDS of a recorded texture: from memory, else decoded again from its
    /// source in the game files.
    fn load(&self, p: &Path) -> Result<Arc<Vec<u8>>, String> {
        if let Some(b) = self.store.cached(p) {
            return Ok(b);
        }
        let rec = self.store.rec(p).ok_or_else(|| format!("{}: not a recorded texture", p.display()))?;
        use super::texmem::Src;
        let b = match &rec.src {
            Src::Pool { key } => self.pool_dds(*key)?,
            Src::Ttpf { h64 } => self.ttpf_dds(*h64)?,
            Src::Normal { hash, key, h64 } => {
                let (w, h, rgba) = self.raw_rgba(*hash, *key, *h64)?;
                rgba_dds(&rgba, w, h)
            }
            Src::Tint { base, rgb } => self.tint_dds(base, *rgb)?,
            Src::FlatBump => rgba_dds(&[128u8, 128, 255, 255].repeat(16 * 16), 16, 16),
            Src::FlatSpec => flat_spec_dds(),
            Src::Ma { hash, key, h64 } => self.ma_dds(*hash, *key, *h64)?,
            Src::Mem => return Err(format!("{}: memory-only texture gone", p.display())),
        };
        let b = Arc::new(b);
        self.store.hold(p, b.clone());
        Ok(b)
    }

    /// Texture (slot) for a colour map: ttdb pools by asset key, then the
    /// TTPF stores (sixng) by 64-bit hash. Pool textures can be BCPack-flat
    /// or a normal map too: the decoded image is checked first.
    fn get(&self, hash: u32, key: u64, h64: u64) -> Result<PathBuf, String> {
        let hash = self.slot("", hash, key, h64);
        let p = self.path(hash);
        if self.store.exists(&p) {
            return Ok(p);
        }
        if h64 == ENGINE_WHITE {
            let d = self.tint_dds("w", [255; 3])?;
            self.store.put(&p, d, super::texmem::Src::Tint { base: "w".into(), rgb: [255; 3] });
            return Ok(p);
        }
        let e1 = match self.pool_dds(key).and_then(|d| check_dds(&d).map(|_| d)) {
            Ok(d) => {
                self.store.put(&p, d, super::texmem::Src::Pool { key });
                return Ok(p);
            }
            Err(e) => e,
        };
        let d = self.ttpf_dds(h64).and_then(|d| check_dds(&d).map(|_| d)).map_err(|e2| format!("{e1}; ttpf: {e2}"))?;
        self.store.put(&p, d, super::texmem::Src::Ttpf { h64 });
        Ok(p)
    }

    /// Pool DDS of the slot's texture when recorded, else straight from the
    /// pools (not recorded: data textures skip the colour-map check).
    fn pool_or_slot(&self, hash: u32, key: u64) -> Result<Arc<Vec<u8>>, String> {
        let p = self.path(hash);
        if self.store.exists(&p) {
            return self.load(&p);
        }
        self.pool_dds(key).map(Arc::new)
    }

    /// Decoded RGBA of a data texture (no colour-map check), for maps that
    /// are sampled at build time (4-layer lookup weights).
    fn rgba(&self, hash: u32, key: u64, h64: u64) -> Result<(u32, u32, Vec<u8>), String> {
        let hash = self.slot("", hash, key, h64);
        let e1 = match self.pool_or_slot(hash, key).and_then(|b| crate::texture::dds_preview(&b).map_err(|e| e.to_string())) {
            Ok(x) => return Ok(x),
            Err(e) => e,
        };
        if h64 == 0 {
            return Err(e1);
        }
        let set = sixng_core::ttpf::global(&self.game);
        let (store, t) = set.get(h64).ok_or_else(|| format!("{e1}; not in any ttpf store"))?;
        if !t.decodable() {
            return Err(format!("{e1}; ttpf format not decodable"));
        }
        let (data, skipped) = store.data_ex(t).map_err(|e| e.to_string())?;
        let mip = if skipped == 0 { 0 } else { store.best_mip(t).ok_or("only BCPack mips")? };
        let (rgba, w, h) = sixng_core::ttpf::decode_rgba(t, &data, mip).map_err(|e| e.to_string())?;
        Ok((w, h, rgba))
    }

    /// Tangent-space normal map `<hash>_n` (texture `r6m_<hash>_n`); also
    /// makes sure the shared flat spec map exists. Flat decodes (undecoded
    /// BCPack) are rejected so they never bend the lighting.
    fn get_normal(&self, hash: u32, key: u64, h64: u64) -> Result<String, String> {
        let id = self.slot("_n", hash, key, h64);
        let name = format!("r6m_{id:08x}_n");
        let p = self.dir.join(format!("{id:08x}_n.dds"));
        if !self.store.exists(&p) {
            let (w, h, rgba) = self.raw_rgba(hash, key, h64)?;
            if bad_colour(&rgba) == Some("flat decode (undecoded BCPack data)") {
                return Err("flat normal decode".into());
            }
            let dds = rgba_dds(&rgba, w, h);
            ytd::from_dds("check", &dds)?;
            self.store.put(&p, dds, super::texmem::Src::Normal { hash, key, h64 });
        }
        self.flat_spec()?;
        Ok(name)
    }

    /// RDR2 `_ma` `r6m_<id>_ma` from the 6m speculartex (`redm prop` only;
    /// `rdr2::ytd::ma_from_spec`). Missing, undecodable (flat BCPack) or
    /// 8 px and smaller sources are errors: the caller binds blank_ma.
    fn get_ma(&self, hash: u32, key: u64, h64: u64) -> Result<String, String> {
        let id = self.slot("_ma", hash, key, h64);
        let p = self.dir.join(format!("{id:08x}_ma.dds"));
        if !self.store.exists(&p) {
            let dds = self.ma_dds(hash, key, h64)?;
            ytd::from_dds("check", &dds)?;
            self.store.put(&p, dds, super::texmem::Src::Ma { hash, key, h64 });
        }
        Ok(format!("r6m_{id:08x}_ma"))
    }

    fn ma_dds(&self, hash: u32, key: u64, h64: u64) -> Result<Vec<u8>, String> {
        let (w, h, rgba) = self.raw_rgba(hash, key, h64)?;
        if w.max(h) <= 8 {
            return Err(format!("speculartex {w}x{h}: engine default size"));
        }
        if bad_colour(&rgba) == Some("flat decode (undecoded BCPack data)") {
            return Err("flat speculartex decode".into());
        }
        Ok(rgba_dds(&crate::rdr2::ytd::ma_from_spec(&rgba), w, h))
    }

    /// Makes sure the shared flat spec map exists.
    fn flat_spec(&self) -> Result<(), String> {
        let sp = self.dir.join(format!("{}.dds", &ydr::FLAT_SPEC[4..]));
        if !self.store.exists(&sp) {
            self.store.put(&sp, flat_spec_dds(), super::texmem::Src::FlatSpec);
        }
        Ok(())
    }

    /// Decoded RGBA without the colour-map check: ttdb pools, then TTPF.
    fn raw_rgba(&self, hash: u32, key: u64, h64: u64) -> Result<(u32, u32, Vec<u8>), String> {
        let hash = self.slot("", hash, key, h64);
        let e1 = match self.pool_or_slot(hash, key).and_then(|b| crate::texture::dds_preview(&b).map_err(|e| e.to_string())) {
            Ok(x) => return Ok(x),
            Err(e) => e,
        };
        let ttpf = || -> Result<(u32, u32, Vec<u8>), String> {
            if h64 == 0 {
                return Err("no 64-bit hash".into());
            }
            let set = sixng_core::ttpf::global(&self.game);
            let (store, t) = set.get(h64).ok_or("not in any ttpf store")?;
            if !t.decodable() {
                return Err(format!("format {} / tile mode {} not decodable", t.dxgi_format, t.tile_mode()));
            }
            let (data, skipped) = store.data_ex(t).map_err(|e| e.to_string())?;
            let mip = if skipped == 0 { 0 } else { store.best_mip(t).ok_or("only BCPack mips")? };
            let (rgba, w, h) = sixng_core::ttpf::decode_rgba(t, &data, mip).map_err(|e| e.to_string())?;
            Ok((w, h, rgba))
        };
        ttpf().map_err(|e2| format!("{e1}; ttpf: {e2}"))
    }

    /// DDS of a TTPF texture (checked: placeholder / flat decodes rejected).
    fn ttpf_dds(&self, h64: u64) -> Result<Vec<u8>, String> {
        if h64 == 0 {
            return Err("no 64-bit hash".into());
        }
        let set = sixng_core::ttpf::global(&self.game);
        let (store, t) = set.get(h64).ok_or("not in any ttpf store")?;
        if !t.decodable() {
            return Err(format!("format {} / tile mode {} not decodable", t.dxgi_format, t.tile_mode()));
        }
        let (data, skipped) = store.data_ex(t).map_err(|e| e.to_string())?;
        if skipped > 0 {
            super::quality::hit("bcpack_skipped_mips", 1, || format!("{h64:016x} {}x{} fmt {}: {skipped} top mips without clean data", t.width, t.height, t.dxgi_format));
        }
        let mip = if skipped == 0 { 0 } else { store.best_mip(t).ok_or("only BCPack mips")? };
        let (rgba, w, h) = sixng_core::ttpf::decode_rgba(t, &data, mip).map_err(|e| e.to_string())?;
        if w <= 8 && h <= 8 {
            return Err("placeholder (8 px or less)".into());
        }
        if let Some(why) = bad_colour(&rgba) {
            super::quality::hit("flat_decode", 1, || format!("ttpf {h64:016x} {w}x{h}: {why}"));
            return Err(why.into());
        }
        // Ship the store's own blocks (BCn / RGBA8, every mip) when the whole
        // chain decoded; RGBA8 re-encoding made TTPF textures 4-8x their
        // size (300+ ytds over FiveM's 16 MB limit: evicted, blurry).
        let native = if skipped == 0 { native_dds(t, &data) } else { None };
        let dds = native.unwrap_or_else(|| rgba_dds(&rgba, w, h));
        ytd::from_dds("check", &dds)?;
        Ok(dds)
    }

    /// DDS of a ttdb pool texture by asset key.
    fn pool_dds(&self, key: u64) -> Result<Vec<u8>, String> {
        if key == 0 {
            return Err("no asset key".into());
        }
        let cat = self.catalog.as_ref().ok_or("no pool catalog")?;
        let loc = cat.find(key).iter().find(|l| l.extractable).ok_or("not in pools")?;
        let map = {
            let mut m = self.maps.lock().unwrap();
            match m.get(&loc.path) {
                Some(x) => x.clone(),
                None => {
                    let f = fs::File::open(&loc.path).map_err(|e| e.to_string())?;
                    // SAFETY: read-only map of a game file.
                    let x = Arc::new(unsafe { Mmap::map(&f) }.map_err(|e| e.to_string())?);
                    m.insert(loc.path.clone(), x.clone());
                    x
                }
            }
        };
        let pool = crate::texture::Pool::open(&map).map_err(|e| e.to_string())?;
        let t = pool.textures.get(loc.index).ok_or("pool index out of range")?;
        let dds = pool.create_dds(t).map_err(|e| e.to_string())?;
        ytd::from_dds("check", &dds)?;
        Ok(dds)
    }

    /// Colour texture `stem` ("w" = none: solid) multiplied by `c` (sRGB).
    fn tint_dds(&self, stem: &str, c: [u8; 3]) -> Result<Vec<u8>, String> {
        let (rgba, w, h) = if stem == "w" {
            ([255u8; 4].repeat(16), 4, 4)
        } else {
            let b = self.load(&self.dir.join(format!("{stem}.dds")))?;
            let (mut w, mut h, mut rgba) = crate::texture::dds_preview(&b).map_err(|e| e.to_string())?;
            // Tinted copies are many; halve them down to 512 px to keep the ytds small.
            while w.max(h) > 512 && w >= 2 && h >= 2 {
                let (nw, nh) = (w / 2, h / 2);
                let mut d = vec![0u8; (nw * nh * 4) as usize];
                for y in 0..nh as usize {
                    for x in 0..nw as usize {
                        for k in 0..4 {
                            let at = |xx: usize, yy: usize| rgba[(yy * w as usize + xx) * 4 + k] as u32;
                            d[(y * nw as usize + x) * 4 + k] = ((at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1) + 2) / 4) as u8;
                        }
                    }
                }
                (w, h, rgba) = (nw, nh, d);
            }
            (rgba, w, h)
        };
        let mut out = rgba;
        for px in out.chunks_exact_mut(4) {
            for k in 0..3 {
                px[k] = ((px[k] as u32 * c[k] as u32 + 127) / 255) as u8;
            }
        }
        Ok(rgba_dds(&out, w, h))
    }
}

fn flat_spec_dds() -> Vec<u8> {
    let px: Vec<u8> = (0..16 * 16).flat_map(|_| [32u8, 32, 32, 255]).collect();
    rgba_dds(&px, 16, 16)
}

/// Texture name of a cached source texture: `r6m_<cache id>`.
fn tex_name(p: &Path) -> String {
    format!("r6m_{}", p.file_stem().map_or(String::new(), |s| s.to_string_lossy().into_owned()))
}

/// Archetype-name suffix of a tinted placement variant: `<name>__tint<row>`.
const TINT_SEP: &str = "__tint";

/// Model name and palette row of a (possibly tinted) archetype name.
fn split_tint(n: &str) -> (&str, usize) {
    n.rsplit_once(TINT_SEP).and_then(|(b, r)| Some((b, r.parse().ok()?))).unwrap_or((n, 0))
}

/// Shader slot of the per-model tint palette (one-row embedded texture).
const TINT_PALETTE: u32 = 0x2c8d_7182;
/// 6m `speculartex` (R metal mask, G roughness, B AO: RDR2 `_ma` layout).
const SPEC_SLOT: u32 = 0xc6f4_6e79;
/// Colour of unused tint-palette entries: every 6m palette row ends in a
/// run of (255, 187, 255) after its last real colour (ap_04_hub_glass
/// 92..127, ss_rail_metror 80..127, ap_road_upper_a_decal 255 and rows 1-3,
/// prop_am_loader_baggage 4..7). Vertex blue pointing there means untinted;
/// baking it made glass, walls and decals magenta/navy.
const PALETTE_FILL: [u8; 3] = [255, 187, 255];
/// Entries per tint-palette row: multi-row palettes are rows of 256
/// (prop_am_bench_int_01a: 2051 = 8 x 256 + 3, columns 0 and 10 line up in
/// every row); a palette shorter than that is one short row.
pub const PALETTE_ROW: usize = 256;

/// Number of whole rows in a palette of `len` entries (a short palette is one row).
pub fn palette_rows(len: usize) -> usize {
    if len == 0 { 0 } else { (len / PALETTE_ROW).max(1) }
}

/// Palette index of (`row`, `column`).
pub fn palette_index(row: usize, col: usize) -> usize {
    row * PALETTE_ROW + col
}

/// Which palette row `rpf9_mesh` bakes, and whether tint colours keep their
/// exact palette value or are rounded to 32 steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TintPick {
    pub row: usize,
    pub exact: bool,
}

impl TintPick {
    /// `fivem build`: row 0, colours rounded to 32 steps (fewer tinted
    /// texture copies across a whole map).
    pub const BUILD: TintPick = TintPick { row: 0, exact: false };
}

/// Colour baked for palette entry `p` (None, filler or out of range: white).
fn tint_colour(p: Option<&[u8; 4]>, exact: bool) -> [u8; 3] {
    p.filter(|p| [p[0], p[1], p[2]] != PALETTE_FILL).map_or([255; 3], |p| {
        let c = [p[0], p[1], p[2]];
        if exact { c } else { c.map(|v| ((v as u32 + 16) / 32 * 32).min(255) as u8) }
    })
}

/// Copy of colour texture `tex` multiplied by `c` (sRGB), or a solid `c`
/// when the model has no usable colour texture (the palette then gives the
/// whole colour). Returns its texture name.
fn tinted(pools: &Pools, tex: &str, c: [u8; 3]) -> Result<String, String> {
    let stem = if tex == MISSING { "w".to_string() } else { tex[4..].to_string() };
    let name = format!("r6m_{stem}_t{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    let p = pools.dir.join(format!("{}.dds", &name[4..]));
    if !pools.store.exists(&p) {
        let d = pools.tint_dds(&stem, c)?;
        pools.store.put(&p, d, super::texmem::Src::Tint { base: stem, rgb: c });
    }
    Ok(name)
}

/// The tint palette embedded in the model resource: the one-row texture
/// that its shaders' palette slots point at (by 64-bit hash), read through
/// sixng-core as its GLB export does. All palette slots of a model share it.
fn sixng_palette(ctx: &Ctx, e: &Entry) -> Option<Vec<[u8; 4]>> {
    use sixng_core::{drawable, resource::Resource, texture};
    let r = Resource::load(&ctx.entry_path(e), e).ok()?;
    let emb: HashMap<u64, texture::TexInfo> = texture::find_textures(&r).into_iter().filter(|t| t.height == 1).filter_map(|t| Some((texture::embedded_hash(&r, &t)?, t))).collect();
    let h64 = drawable::find_shaders(&r).into_iter().flat_map(|s| s.textures).find(|t| u32::from_str_radix(&t.param_hash, 16).ok() == Some(TINT_PALETTE) && u64::from_str_radix(&t.hash64, 16).is_ok_and(|h| emb.contains_key(&h)))?.hash64;
    let rgba = texture::embedded_rgba(&r, emb.get(&u64::from_str_radix(&h64, 16).ok()?)?).ok()?;
    Some(rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect())
}

/// Tint palette bound by submesh `sm`'s shader: the model's own one-row
/// texture by name hash, else the sixng-core read (cached in `ext`).
fn shader_palette(ctx: &Ctx, m: &crate::model::Drawable, sm: &crate::model::Mesh, entry: Option<&Entry>, ext: &std::cell::OnceCell<Option<Vec<[u8; 4]>>>) -> Option<Vec<[u8; 4]>> {
    let t = sm.shader.and_then(|s| m.materials.as_ref()?.get(s)).and_then(|mat| mat.textures.iter().find(|t| t.param == TINT_PALETTE))?;
    m.palettes.iter().find(|p| p.0 == t.hash).map(|p| p.1.clone()).or_else(|| ext.get_or_init(|| entry.and_then(|e| sixng_palette(ctx, e))).clone())
}

/// The model of a decoded resource that `name` converts: the one named so,
/// else the one with the most vertices.
fn pick_model<'a>(models: &'a [crate::model::Drawable], name: &str) -> Option<&'a crate::model::Drawable> {
    let base = |n: &str| n.split(".#").next().unwrap_or(n).to_lowercase();
    let want = name.to_lowercase();
    models.iter().find(|m| base(&m.name) == want).or_else(|| models.iter().max_by_key(|m| m.meshes.iter().map(|m| m.positions.len()).sum::<usize>()))
}

fn placeholder() -> Tex {
    let mut data = Vec::new();
    let (mut w, mut levels) = (64usize, 0u8);
    while w >= 1 {
        for y in 0..w {
            for x in 0..w {
                let on = ((x * 8 / w.max(8)) + (y * 8 / w.max(8))) % 2 == 0;
                data.extend_from_slice(if on { &[200, 0, 200, 255] } else { &[90, 90, 90, 255] });
            }
        }
        levels += 1;
        w /= 2;
    }
    Tex { name: MISSING.into(), width: 64, height: 64, levels, format: TexFormat::A8R8G8B8, data, unk32: 0, usage_data: 0, extra_flags: 0 }
}

fn f3(v: &Value) -> V3 {
    std::array::from_fn(|k| v[k].as_f64().unwrap_or(0.0) as f32)
}

/// Placement JSON dir: given, else <work>/placements/levels. Everything it
/// depends on is prepared in-process from the sixng index and levels.rpf, so
/// no `sixng interior-entities` / `lod-links` / `placements` runs are needed:
/// each output is (re)built only when missing or older than its inputs, and
/// placements also when they were made for a region not covering --bbox
/// (recorded in .fivem_region.json).
/// Version of the small derived index tables (interior entities, light
/// index, LOD links, archetype LODs): with the sixng index version and
/// crate version it forms the stamp in <work>/index/derived_tables.json.
/// Tables made under another stamp are rebuilt. Bump on producer changes.
const DERIVED_V: &str = "derived3";

fn derived_stamp() -> String {
    format!("{}|{}|{DERIVED_V}", sixng_core::INDEX_VERSION, env!("CARGO_PKG_VERSION"))
}

/// True when the derived index tables were made by this code.
fn derived_ok(ctx: &Ctx) -> bool {
    fs::read_to_string(ctx.o.work.join("index").join("derived_tables.json")).is_ok_and(|s| s == derived_stamp())
}

fn mark_derived(ctx: &Ctx) {
    let _ = fs::write(ctx.o.work.join("index").join("derived_tables.json"), derived_stamp());
}

/// Ticks a progress bar when dropped (every exit of a closure).
struct Tick<'a>(&'a super::progress::Bar);

impl Drop for Tick<'_> {
    fn drop(&mut self) {
        self.0.tick();
    }
}

fn placements_dir(ctx: &Ctx, index: &ArchiveIndex) -> Result<PathBuf, Box<dyn Error>> {
    if let Some(d) = &ctx.o.placements {
        if sixng_core::vfs::exists(&d.join("index.jsonl")) {
            return Ok(d.clone());
        }
        return Err(format!("{}: no index.jsonl", d.display()).into());
    }
    let dir = ctx.o.work.join("placements").join("levels");
    let ws = sixng_core::export::Workspace::new(&ctx.o.game, &ctx.o.work);
    let a = ws.resolve("levels.rpf")?;
    let metas: Vec<&Entry> = index.entries.iter().filter(|e| e.kind == sixng_core::classify::Kind::Resource && e.sub == "7a").collect();
    let ix = ctx.o.work.join("index");
    let mtime = |p: &Path| sixng_core::vfs::modified(p);
    let fresh_code = derived_ok(ctx);
    let stale = |p: &Path, deps: &[Option<std::time::SystemTime>]| match mtime(p) {
        None => true,
        Some(t) => !fresh_code || t < ctx.input_time || deps.iter().flatten().any(|d| *d > t),
    };
    let ie = ix.join("interior_entities.jsonl");
    if stale(&ie, &[]) || !sixng_core::vfs::exists(&ix.join(sixng_core::interiorents::LIGHT_INDEX)) {
        let _p = super::Phase::new(ctx, "  prep interior-ents");
        sixng_core::interiorents::build(&ws, &a, &metas, super::progress::sixng("prep interior-ents"))?;
    }
    let lp = ix.join("lod_parents.jsonl");
    if stale(&lp, &[mtime(&ie)]) || stale(&ix.join("archetype_lod.jsonl"), &[]) {
        let _p = super::Phase::new(ctx, "  prep lod-links");
        sixng_core::lodlinks::build(&ws, &a, &metas, super::progress::sixng("prep lod-links"))?;
    }
    export_region(ctx, &ws, &a.path, &a.id, &metas, &dir, &[mtime(&ie), mtime(&lp)])?;
    Ok(dir)
}

/// Placement JSON for the map files of one archive that can touch the
/// build region, into `dir` (index.jsonl + <offset>.json); redone when the
/// recorded region does not cover --bbox or the output is older than the
/// archive or `deps`.
fn export_region(ctx: &Ctx, ws: &sixng_core::export::Workspace, archive: &Path, id: &str, metas: &[&Entry], dir: &Path, deps: &[Option<std::time::SystemTime>]) -> Result<(), Box<dyn Error>> {
    let mtime = |p: &Path| sixng_core::vfs::modified(p);
    let input = mtime(archive).map_or(ctx.input_time, |t| t.max(ctx.input_time));
    let stale = |p: &Path, deps: &[Option<std::time::SystemTime>]| match mtime(p) {
        None => true,
        Some(t) => t < input || deps.iter().flatten().any(|d| *d > t),
    };
    let region_f = dir.join(".fivem_region.json");
    let made_for: Option<Option<[f32; 4]>> = sixng_core::vfs::read(&region_f).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let covers = match (made_for, ctx.o.bbox) {
        (Some(None), _) => true,
        (Some(Some(m)), Some(b)) => m[0] <= b[0] && m[1] <= b[1] && m[2] >= b[2] && m[3] >= b[3],
        _ => false,
    };
    let pi = dir.join("index.jsonl");
    if !covers || stale(&pi, deps) {
        let _p = super::Phase::new(ctx, "  prep placements");
        // a region export is enough for this build; grown 1 km so nearby rebuilds reuse it
        let reg = ctx.o.bbox.map(|b| [b[0] - 1000.0, b[1] - 1000.0, b[2] + 1000.0, b[3] + 1000.0]);
        if let Some(r) = reg {
            sixng_core::export::set_region(r);
        }
        // only the map files that can touch the region (+ interior-space
        // files, few and needed by MLO instances): no whole-map export
        let sp = meta_spatial(ctx, archive, id, metas);
        let want: Vec<&Entry> = metas
            .iter()
            .copied()
            .filter(|e| match (sp.get(&e.off), reg) {
                (None, _) => false,
                (Some(_), None) => true,
                (Some((world, b)), Some(r)) => !*world || (b[0] <= r[2] && b[2] >= r[0] && b[1] <= r[3] && b[3] >= r[1]),
            })
            .collect();
        eprintln!("buildings: exporting {} of {} map files of {id}", want.len(), metas.len());
        fast_export(ws, archive, &want, dir)?;
        sixng_core::vfs::create_dir_all(dir)?;
        sixng_core::vfs::write(&region_f, serde_json::to_vec(&reg)?)?;
    }
    Ok(())
}

/// Placement dirs of the story / minigame packs (<work>/placements/<stem>):
/// their LOD links (per-archive sixng side files) and region export, built
/// like levels.rpf's, plus their MLO instances (sixng export_interiors).
fn pack_placements(ctx: &Ctx) -> Result<Vec<(PathBuf, String)>, Box<dyn Error>> {
    if ctx.o.placements.is_some() {
        return Ok(Vec::new());
    }
    let ws = sixng_core::export::Workspace::new(&ctx.o.game, &ctx.o.work);
    let mut out = Vec::new();
    let fresh_code = derived_ok(ctx);
    for pk in &ctx.packs {
        let a = ws.resolve(&pk.id)?;
        let dir = ctx.o.work.join("placements").join(pk.id.trim_end_matches(".rpf"));
        let metas: Vec<&Entry> = pk.index.entries.iter().filter(|e| e.kind == sixng_core::classify::Kind::Resource && e.sub == "7a").collect();
        let ix = ctx.o.work.join("index");
        let mtime = |p: &Path| sixng_core::vfs::modified(p);
        let lp = ix.join(sixng_core::game::per_archive_name("lod_parents", "jsonl", &pk.id));
        let al = ix.join(sixng_core::game::per_archive_name("archetype_lod", "jsonl", &pk.id));
        let src = mtime(&pk.path);
        let old = |p: &Path| mtime(p).map_or(true, |t| src.is_some_and(|s| t < s));
        if !fresh_code || old(&lp) || old(&al) {
            let _p = super::Phase::new(ctx, "  prep lod-links (pack)");
            sixng_core::lodlinks::build(&ws, &a, &metas, super::progress::sixng("prep lod-links"))?;
        }
        export_region(ctx, &ws, &pk.path, &pk.id, &metas, &dir, &[mtime(&lp)])?;
        // MLO instances of the pack (interiors.jsonl next to its placements)
        // interior collision: export_interiors links bounds by bbox from
        // <work>/collision/<archive>/index.jsonl (unnamed rageobj resources)
        let ci = ctx.o.work.join("collision").join(pk.id.trim_end_matches(".rpf")).join("index.jsonl");
        if old(&ci) {
            let _p = super::Phase::new(ctx, "  prep collision (pack)");
            let objs: Vec<&Entry> = pk.index.entries.iter().filter(|e| e.kind == sixng_core::classify::Kind::RageObj && e.name.is_none()).collect();
            sixng_core::export::export_collision(&a, &objs, &ctx.o.work, super::progress::sixng("prep collision"))?;
        }
        let ij = dir.join("interiors.jsonl");
        if old(&ij) || mtime(&ci).zip(mtime(&ij)).is_some_and(|(c, i)| c > i) {
            let _p = super::Phase::new(ctx, "  prep interiors (pack)");
            sixng_core::export::export_interiors(&a, &metas, &pk.index.entries, &ctx.o.work, super::progress::sixng("prep interiors"))?;
        }
        out.push((dir, pk.id.clone()));
    }
    Ok(out)
}

/// Per map file (entry offset): world-space flag and xy bbox of its entity
/// positions, from the system page only. Cached in <work>/fivem_meta_xy.json
/// per levels.rpf stamp; entries not in the cache are scanned, so this is a
/// one-off scan per game version (files without entities are recorded too).
fn meta_spatial(ctx: &Ctx, archive: &Path, id: &str, metas: &[&Entry]) -> HashMap<u64, (bool, [f32; 4])> {
    use sixng_core::meta7a::{Meta, ENTITY};
    use serde::{Deserialize, Serialize};
    #[derive(Serialize, Deserialize, Default)]
    struct C {
        stamp: String,
        rows: Vec<(u64, u8, [f32; 4])>,
    }
    // one cache per archive (offsets of different archives overlap); the
    // stamp is that archive's size:mtime (levels.rpf: same as ctx.input_stamp)
    let p = ctx.o.work.join(sixng_core::game::per_archive_name("fivem_meta_xy", "json", id));
    let stamp = fs::metadata(archive).ok().map(|m| format!("{}:{}", m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0))).unwrap_or_default();
    let mut c: C = sixng_core::vfs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok()).filter(|c: &C| c.stamp == stamp).unwrap_or_default();
    c.stamp = stamp;
    let have: HashSet<u64> = c.rows.iter().map(|r| r.0).collect();
    let todo: Vec<&&Entry> = metas.iter().filter(|e| !have.contains(&e.off)).collect();
    if !todo.is_empty() {
        let _p = super::Phase::new(ctx, "  prep map-file scan");
        let Ok(map) = fs::File::open(archive).and_then(|f| unsafe { Mmap::map(&f) }) else { return HashMap::new() };
        let new: Vec<(u64, u8, [f32; 4])> = todo
            .par_iter()
            .map(|e| {
                let none = (e.off, 2u8, [0f32; 4]);
                let Some(r) = sys_from_map(&map, e) else { return none };
                let Some(m) = Meta::parse(&r) else { return none };
                let ents = m.objects(ENTITY);
                if ents.is_empty() {
                    return none;
                }
                let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
                for &o in &ents {
                    let (x, y) = (r.f32(o + 0x38), r.f32(o + 0x3C));
                    b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
                }
                let world = sixng_core::interiorents::file_info(&m).space.starts_with("world");
                (e.off, if world { 1 } else { 0 }, b)
            })
            .collect();
        c.rows.extend(new);
        let _ = sixng_core::vfs::write(&p, &serde_json::to_vec(&c).unwrap());
    }
    c.rows.into_iter().filter(|r| r.1 != 2).map(|r| (r.0, (r.1 == 1, r.2))).collect()
}

/// sixng_core::export::export_placements for `entries` only, reading through
/// a memory map (sys_from_map) and loading only these files' LOD parents
/// (lines of the 300+ MB lod_parents.jsonl parsed in parallel, filtered by
/// map id first). Writes the same <offset>.json files and index.jsonl.
fn fast_export(ws: &sixng_core::export::Workspace, archive: &Path, entries: &[&Entry], dir: &Path) -> Result<(), Box<dyn Error>> {
    use sixng_core::{interiorents, meta7a};
    sixng_core::vfs::create_dir_all(dir)?;
    let map = unsafe { Mmap::map(&fs::File::open(archive)?)? };
    let names = sixng_core::export::model_names(ws);
    let ix = ws.index_dir();
    let ids: HashSet<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    // every archive's side file (map ids carry their archive)
    let par_lines = |f: &str| sixng_core::game::per_archive_files(&ix, f, "jsonl").iter().filter_map(|p| sixng_core::vfs::read_to_string(p).ok()).collect::<Vec<_>>().join("\n");
    let ptxt = par_lines("lod_parents");
    let parents: HashMap<String, HashMap<u64, Value>> = ptxt
        .par_lines()
        .filter(|l| l.find("\"map\":\"").and_then(|i| l[i + 7..].split('"').next()).is_some_and(|id| ids.contains(id)))
        .filter_map(|l| {
            let mut v: Value = serde_json::from_str(l).ok()?;
            let id = v["map"].as_str()?.to_string();
            let mut m = HashMap::new();
            for p in v["parents"].as_array_mut()?.iter_mut() {
                if let Some(i) = p["index"].as_u64() {
                    if let Some(o) = p.as_object_mut() {
                        o.remove("index");
                    }
                    m.insert(i, p.take());
                }
            }
            Some((id, m))
        })
        .collect();
    drop(ptxt);
    let atxt = par_lines("archetype_lod");
    let arch_lod: HashMap<u32, (f32, f32)> = atxt
        .par_lines()
        .filter_map(|l| {
            let v: Value = serde_json::from_str(l).ok()?;
            let h = u32::from_str_radix(v["archetype"].as_str()?, 16).ok()?;
            let f = |k: &str| v[k].as_f64().unwrap_or(-1.0) as f32;
            Some((h, (f("lod_dist"), f("hd_texture_dist"))))
        })
        .collect();
    drop(atxt);
    let mut rows: Vec<Value> = entries
        .par_iter()
        .filter_map(|e| {
            let r = sys_from_map(&map, e)?;
            let mf = meta7a::map_file(&r, &names)?;
            if mf.entities.is_empty() {
                return None;
            }
            let fname = format!("{:x}.json", e.off);
            let mut doc = serde_json::to_value(&mf).ok()?;
            doc["id"] = json!(e.id);
            let fi = meta7a::Meta::parse(&r).map(|m| interiorents::file_info(&m));
            let space = fi.as_ref().map(|f| f.space).unwrap_or("world");
            doc["space"] = json!(space);
            if let Some(f) = fi.as_ref().filter(|f| !f.interiors.is_empty()) {
                doc["interiors"] = json!(f.interiors);
            }
            if let Some(ents) = doc["entities"].as_array_mut() {
                if let Some(f) = &fi {
                    for (i, j) in ents.iter_mut().enumerate() {
                        if let Some(v) = f.entities.get(&i) {
                            let mut v = v.clone();
                            if let Some(o) = v.as_object_mut() {
                                o.remove("index");
                            }
                            j["interior"] = v;
                        }
                    }
                }
                let ps = parents.get(&e.id);
                for (i, (j, en)) in ents.iter_mut().zip(&mf.entities).enumerate() {
                    let lod = &mut j["lod"];
                    if let Some(&(ld, htd)) = arch_lod.get(&en.archetype) {
                        let own = lod["lod_dist"].as_f64().unwrap_or(-1.0);
                        lod["archetype_lod_dist"] = json!(ld);
                        lod["hd_texture_dist"] = json!(htd);
                        lod["effective_lod_dist"] = json!(if own > 0.0 { own as f32 } else { ld });
                    }
                    if let Some(p) = ps.and_then(|m| m.get(&(i as u64))) {
                        lod["parent"] = p.clone();
                    }
                }
            }
            sixng_core::vfs::write(dir.join(&fname), serde_json::to_vec(&doc).ok()?).ok()?;
            let mut lo = [f32::MAX; 3];
            let mut hi = [f32::MIN; 3];
            for en in &mf.entities {
                for k in 0..3 {
                    lo[k] = lo[k].min(en.position[k]);
                    hi[k] = hi[k].max(en.position[k]);
                }
            }
            Some(json!({"id": e.id, "file": format!("placements/{}/{fname}", dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), "name_hash": format!("{:08x}", mf.name_hash), "parent_hash": format!("{:08x}", mf.parent_hash),
                "entities": mf.entities.len(), "resolved": mf.entities.iter().filter(|x| x.name.is_some()).count(), "bbox": [lo, hi], "space": space}))
        })
        .collect();
    rows.sort_by(|x, y| x["id"].as_str().cmp(&y["id"].as_str()));
    let by_name: HashMap<String, String> = rows.iter().filter(|x| x["name_hash"] != "00000000").filter_map(|x| Some((x["name_hash"].as_str()?.to_string(), x["id"].as_str()?.to_string()))).collect();
    let mut out = String::new();
    for x in rows.iter_mut() {
        let p = x["parent_hash"].as_str().filter(|h| *h != "00000000").and_then(|h| by_name.get(h)).cloned();
        x["parent_id"] = json!(p);
        out += &x.to_string();
        out.push('\n');
    }
    sixng_core::vfs::write(dir.join("index.jsonl"), out)?;
    Ok(())
}

/// System page of a resource read from a memory-mapped archive, inflating
/// each stream from exactly its compressed bytes (sixng's file reader pulls
/// 4 MB per stream, which made whole-archive scans I/O-bound). Same sys
/// stream rule as sixng_core::resource::read_sys.
pub(crate) fn sys_from_map(map: &[u8], e: &Entry) -> Option<sixng_core::resource::Resource> {
    use std::io::Read;
    let inflate = |st: &[u64; 3]| -> Option<Vec<u8>> {
        let src = map.get(st[0] as usize..(st[0] + st[1]) as usize)?;
        let mut out = Vec::with_capacity(st[2] as usize);
        flate2::read::ZlibDecoder::new(src).read_to_end(&mut out).ok()?;
        Some(out)
    };
    let mut sys = inflate(e.streams.first()?)?;
    let mut n = 1;
    let pm = sys.get(8..16).map(|b| u64::from_le_bytes(b.try_into().unwrap())).unwrap_or(0);
    if pm >> 28 == 5 {
        let need = (pm & 0x0FFF_FFFF) as usize + 8;
        while sys.len() < need && n < e.streams.len() {
            sys.extend(inflate(&e.streams[n])?);
            n += 1;
        }
    }
    while n < e.streams.len() {
        let len = sys.len();
        let next = e.streams[n][2] as usize;
        let hits = sys
            .chunks_exact(8)
            .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
            .filter(|&p| p >> 28 == 5 && p & 3 == 0 && ((p & 0x0FFF_FFFF) as usize) >= len && ((p & 0x0FFF_FFFF) as usize) < len + next)
            .count();
        if hits < 2.max(next / 65536) {
            break;
        }
        sys.extend(inflate(&e.streams[n])?);
        n += 1;
    }
    Some(sixng_core::resource::Resource { sys, gfx: Vec::new(), gfx_base: 0 })
}

/// World placements from every placement dir (levels.rpf first, then the
/// story / minigame packs); interiors from the levels.rpf dir.
fn load_places(ctx: &Ctx, dirs: &[(PathBuf, String)], index: &ArchiveIndex) -> Result<Vec<Place>, Box<dyn Error>> {
    let mut files: Vec<PathBuf> = Vec::new();
    for (k, (d, _)) in dirs.iter().enumerate() {
        let idx = match sixng_core::vfs::read_to_string(d.join("index.jsonl")) {
            Ok(t) => t,
            Err(e) if k == 0 => return Err(e.into()),
            Err(_) => continue,
        };
        files.extend(
            idx.lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .filter(|v| v["space"] == "world" && ctx.o.in_bbox(f3(&v["bbox"][0]), f3(&v["bbox"][1])))
                .filter_map(|v| Some(d.join(Path::new(v["file"].as_str()?).file_name()?))),
        );
    }
    eprintln!("buildings: {} world placement files in region", files.len());
    let load = |f: &Path| sixng_core::vfs::read(f).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let per: Vec<(String, Vec<Value>)> = files
        .par_iter()
        .filter_map(|f| {
            let v = load(f)?;
            Some((v["id"].as_str().unwrap_or("").to_string(), v["entities"].as_array().cloned().unwrap_or_default()))
        })
        .collect();
    let mut out = Vec::new();
    let mut seen: HashMap<(String, i64, i64, i64), Vec<[f32; 4]>> = HashMap::new();
    let mut push = |e: &Value, pos: V3, stored: [f32; 4], key: Option<(String, usize)>, out: &mut Vec<Place>| {
        if !ctx.o.in_bbox(pos, pos) || (pos[0].abs() < 50.0 && pos[1].abs() < 50.0) {
            return;
        }
        let name = e["name"].as_str().map(str::to_string);
        if let (Some(f), Some(n)) = (&ctx.o.name, &name) {
            if !n.to_lowercase().contains(f.as_str()) {
                return;
            }
        } else if ctx.o.name.is_some() {
            return;
        }
        let Some(name) = name else {
            ctx.skip(&format!("hash_{:08x}", e["archetype"].as_u64().unwrap_or(0)), "unnamed hash", json!({"position": pos}));
            return;
        };
        let Some(off) = e["model_id"].as_str().and_then(|m| ctx.voff_of(m)) else {
            ctx.skip(&name, "model archive not loaded", json!({"model_id": e["model_id"]}));
            return;
        };
        // Same model at the same place (10 cm) with the same orientation is
        // one entity placed twice: 6m state-variant map files (adjacent
        // archive entries, different name hashes) repeat their shared
        // entities. Same spot but a different rotation is a separate entity
        // (several cable attachments on one pole), so it is kept.
        let k = (name.clone(), (pos[0] * 10.0).round() as i64, (pos[1] * 10.0).round() as i64, (pos[2] * 10.0).round() as i64);
        let rots: &mut Vec<[f32; 4]> = seen.entry(k).or_default();
        if rots.iter().any(|r| (0..4).map(|i| r[i] * stored[i]).sum::<f32>().abs() > 0.9995) {
            super::regionlog::bump("dedup_drops", "same model, spot and rotation", &name);
            return;
        }
        rots.push(stored);
        let rot = if ctx.o.conjugate { [-stored[0], -stored[1], -stored[2], stored[3]] } else { stored };
        let l = &e["lod"];
        // Tinted placements (palette row > 0) get their own archetype variant.
        let name = match e["tint_row"].as_u64() {
            Some(row) if row > 0 => format!("{name}{TINT_SEP}{row}"),
            _ => name,
        };
        out.push(Place {
            name,
            off,
            pos,
            rot,
            sxy: e["scale_xy"].as_f64().unwrap_or(1.0) as f32,
            sz: e["scale_z"].as_f64().unwrap_or(1.0) as f32,
            key,
            level: level_of(l["level"].as_str().unwrap_or("HD")),
            elod: l["lod_dist"].as_f64().unwrap_or(-1.0) as f32,
            child_lod: l["child_lod_dist"].as_f64().unwrap_or(0.0) as f32,
            src_parent: l["parent_index"].is_i64() || l["parent_instance"].is_i64(),
        });
    };
    for (map, ents) in &per {
        for (i, e) in ents.iter().enumerate() {
            let stored = q4(&e["rotation"]).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            push(e, f3(&e["position"]), stored, Some((map.clone(), i)), &mut out);
        }
    }
    // Interiors (MLO instances): interior-space entities moved into world space.
    let (mut n_int, mut n_ent) = (0, 0);
    for (dir, aid) in dirs.iter().filter(|_| ctx.o.interiors) {
        let Ok(text) = sixng_core::vfs::read_to_string(dir.join("interiors.jsonl")) else { continue };
        let prefix = format!("{aid}@");
        let mut cache: HashMap<String, Option<Arc<Value>>> = HashMap::new();
        for l in text.lines() {
            let Ok(inst) = serde_json::from_str::<Value>(l) else { continue };
            let ip = f3(&inst["position"]);
            if !ctx.o.in_bbox(ip, ip) {
                continue;
            }
            let Some(file) = inst["interior_file"].as_str().and_then(|f| f.strip_prefix(prefix.as_str())) else { continue };
            let Some(src) = cache.entry(file.to_string()).or_insert_with(|| load(&dir.join(format!("{file}.json"))).map(Arc::new)).clone() else {
                ctx.skip(&format!("interior {}", inst["archetype"].as_str().unwrap_or("?")), "interior entity file missing", json!({"file": file}));
                continue;
            };
            let empty = Vec::new();
            let ents = src["entities"].as_array().unwrap_or(&empty);
            let ri = q4(&inst["rotation_world"]).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            n_int += 1;
            for ix in inst["entities"].as_array().into_iter().flatten().chain(inst["entity_set_entities"].as_array().into_iter().flatten()) {
                let Some(e) = ix.as_u64().and_then(|i| ents.get(i as usize)) else { continue };
                let lp = f3(&e["position"]);
                let d = qrot(ri, lp);
                let pos = [ip[0] + d[0], ip[1] + d[1], ip[2] + d[2]];
                let lw = q4(&e["rotation_world"])
                    .or_else(|| q4(&e["rotation"]).map(|q| [-q[0], -q[1], -q[2], q[3]]))
                    .unwrap_or([0.0, 0.0, 0.0, 1.0]);
                let w = qmul(ri, lw);
                n_ent += 1;
                push(e, pos, [-w[0], -w[1], -w[2], w[3]], None, &mut out);
            }
        }
    }
    if n_int > 0 {
        eprintln!("buildings: {n_int} interiors, {n_ent} interior entities placed in world space");
    }
    for (e, pos, stored) in super::batches::props(ctx, index) {
        // keyed "batch:<map>:<model>" so HD entities whose 6m LOD parent is
        // an instance of this batch (lod_parents.jsonl kind "batch") find it
        let key = e["batch_map"].as_str().zip(e["batch_model"].as_str()).map(|(m, n)| (format!("batch:{m}:{n}"), 0));
        push(&e, pos, stored, key, &mut out);
    }
    Ok(out)
}

/// child (map, index) -> parent (map, index), from sixng lod-links
/// (<work>/index/lod_parents.jsonl), for the given maps only.
/// Second map: child -> (batch map, batchlod model) for parents of kind
/// "batch" (6m parents most HD props to an instance of a prop batch).
fn lod_parents(ctx: &Ctx, maps: &HashSet<&str>) -> (HashMap<(String, usize), (String, usize)>, HashMap<(String, usize), String>) {
    let mut out = HashMap::new();
    let mut batch = HashMap::new();
    let Some(text) = index_text(ctx, "lod_parents", "jsonl") else {
        ctx.skip("(lod chains)", "lod_parents.jsonl missing (run sixng lod-links); entities left as orphans", json!({}));
        return (out, batch);
    };
    for l in text.lines() {
        let Some(m) = l.strip_prefix("{\"map\":\"").and_then(|r| r.split('"').next()) else { continue };
        if !maps.contains(m) {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        for p in v["parents"].as_array().into_iter().flatten() {
            if p["kind"] == "batch" {
                if let (Some(i), Some(pm), Some(n)) = (p["index"].as_u64(), p["map"].as_str(), p["name"].as_str()) {
                    batch.insert((m.to_string(), i as usize), format!("batch:{pm}:{n}"));
                }
                continue;
            }
            if p["kind"] != "entity" {
                continue;
            }
            if let (Some(i), Some(pm), Some(pi)) = (p["index"].as_u64(), p["map"].as_str(), p["parent_index"].as_u64()) {
                out.insert((m.to_string(), i as usize), (pm.to_string(), pi as usize));
            }
        }
    }
    (out, batch)
}

/// All per-archive copies of a sixng index side file (levels.rpf's first),
/// concatenated; None when there is none.
fn index_text(ctx: &Ctx, base: &str, ext: &str) -> Option<String> {
    let v: Vec<String> = sixng_core::game::per_archive_files(&ctx.o.work.join("index"), base, ext).iter().filter_map(|p| sixng_core::vfs::read_to_string(p).ok()).collect();
    (!v.is_empty()).then(|| v.join("\n"))
}

/// archetype name -> lodDist from sixng lod-links (<work>/index/archetype_lod.jsonl).
fn archetype_lods(ctx: &Ctx, names: &[&String]) -> HashMap<String, f32> {
    let mut want: HashMap<String, Vec<&String>> = HashMap::new();
    for n in names {
        want.entry(format!("{:08x}", crate::gta5::rsc::joaat(split_tint(n).0))).or_default().push(*n);
    }
    let mut out = HashMap::new();
    let Some(text) = index_text(ctx, "archetype_lod", "jsonl") else { return out };
    for l in text.lines() {
        let Some(h) = l.strip_prefix("{\"archetype\":\"").and_then(|r| r.get(..8)) else { continue };
        let Some(ns) = want.get(h) else { continue };
        if let Ok(v) = serde_json::from_str::<Value>(l) {
            if let Some(d) = v["lod_dist"].as_f64().filter(|d| *d > 0.0) {
                for n in ns {
                    // levels.rpf's file first: earlier archives win
                    out.entry((*n).clone()).or_insert(d as f32);
                }
            }
        }
    }
    out
}

struct Arch {
    an: String,
    lo: V3,
    hi: V3,
    texs: Vec<String>,
    /// textures on non-opaque geometry (their alpha is read)
    alpha: Vec<String>,
}

type Built = (Mesh, Vec<String>, Vec<String>, bool);

/// 6m four-layer ground shaders (`standard_uber_4lyr_*_hbb`: 61e09df7,
/// b21f1ba5, 360e396b). Slot names, lookup-weight channels and the 64x
/// detail tiling are from SixNG (GPL-3.0, commits 1456a9b / 6046d19; uv3 variant da11771).
const FOUR_LAYER: [u32; 4] = [0x61e0_9df7, 0xb21f_1ba5, 0x360e_396b, 0x27e5_c49d];
const DETAIL_TILE: f32 = 64.0;
/// Writer version of four-layer ground ydrs (in their cache key only).
const LYR4_V: &str = "lyr4_3";

/// Four-layer ground submesh as terrain_cb_w_4lyr geometries: the layer
/// diffuses become the shader's four samplers, the section `lookuptexture`
/// (R, G, B = layers 0-2, remainder = layer 3, sampled at TEXCOORD_0) is
/// sampled per vertex and folded into GTA V's bilinear Colour1 blend
/// (G: layers 2/3 over 0/1, B: 1/3 over 0/2). None (single-diffuse
/// fallback) when fewer than two layers or the lookup do not resolve.
fn four_layer_geoms(an: &str, pools: &Pools, mat: &crate::model::Material, key_of: &dyn Fn(&crate::model::TexRef) -> u64, sm: &crate::model::Mesh) -> Option<(Vec<ydr::Geom>, Vec<String>)> {
    let uvs = sm.uvs.as_deref()?;
    let slot = |name: &str| mat.textures.iter().find(|t| t.param == crate::gta5::rsc::joaat(name));
    let names: Vec<Option<String>> = (0..4)
        .map(|i| {
            let t = slot(&format!("diffusetexture_layer{i}"))?;
            pools.get(t.hash, key_of(t), t.h64).ok().map(|p| tex_name(&p))
        })
        .collect();
    let first = names.iter().flatten().next()?.clone();
    if names.iter().flatten().count() < 2 {
        return None;
    }
    let layers: [String; 4] = std::array::from_fn(|i| names[i].clone().unwrap_or_else(|| first.clone()));
    // 6m layer normal maps (bumptexture_layerN); the flat normal where one
    // does not decode.
    let flat = flat_bump(pools).ok()?;
    let bumps: [String; 4] = std::array::from_fn(|i| {
        slot(&format!("bumptexture_layer{i}")).and_then(|t| pools.get_normal(t.hash, key_of(t), t.h64).ok()).unwrap_or_else(|| flat.clone())
    });
    let no_bump: Vec<usize> = (0..4).filter(|&i| bumps[i] == flat).collect();
    eprintln!(
        "[t6m:terrain] {an}: samplers {} (expect 8), layers without 6m bumptexture {no_bump:?}, vertex PNCCTX/56 Colour0=(ao,0,0,255) Colour1=(0,g,b,0)",
        layers.iter().chain(&bumps).filter(|t| !t.is_empty()).count()
    );
    let lt = slot("lookuptexture")?;
    let (lw, lh, px) = pools.rgba(lt.hash, key_of(lt), lt.h64).ok()?;
    if lw == 0 || lh == 0 || px.len() < (lw * lh * 4) as usize {
        return None;
    }
    let sample = |uv: [f32; 2]| -> [f32; 3] {
        // bilinear; wraps like the game's sampler (set pieces run past 0..1)
        let x = (uv[0].rem_euclid(1.0) * lw as f32 - 0.5).clamp(0.0, (lw - 1) as f32);
        let y = (uv[1].rem_euclid(1.0) * lh as f32 - 0.5).clamp(0.0, (lh - 1) as f32);
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(lw - 1), (y0 + 1).min(lh - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |xx: u32, yy: u32, c: usize| px[((yy * lw + xx) * 4) as usize + c] as f32 / 255.0;
        std::array::from_fn(|c| {
            let top = at(x0, y0, c) * (1.0 - fx) + at(x1, y0, c) * fx;
            let bot = at(x0, y1, c) * (1.0 - fx) + at(x1, y1, c) * fx;
            top * (1.0 - fy) + bot * fy
        })
    };
    // Detail mapping (SixNG material.rs four_layer_detail_uv): map section
    // tiles have TEXCOORD_0 = section position / 512 m (430-620 m per unit
    // allowing for quantisation) and repeat the layers 64x per unit; other
    // models (set pieces, rocks) tile from position, one repeat per 8 m.
    let (mut pa, mut ua) = (0f64, 0f64);
    for t in sm.indices.chunks_exact(3) {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| i as usize);
        let (Some(pa_), Some(pb), Some(pc)) = (sm.positions.get(a), sm.positions.get(b), sm.positions.get(c)) else { continue };
        let (Some(ta), Some(tb), Some(tc)) = (uvs.get(a), uvs.get(b), uvs.get(c)) else { continue };
        let (e1, e2) = ([pb[0] - pa_[0], pb[1] - pa_[1], pb[2] - pa_[2]], [pc[0] - pa_[0], pc[1] - pa_[1], pc[2] - pa_[2]]);
        let cr = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        pa += ((cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2]) as f64).sqrt();
        ua += (((tb[0] - ta[0]) * (tc[1] - ta[1]) - (tb[1] - ta[1]) * (tc[0] - ta[0])) as f64).abs();
    }
    let section_tile = ua > 0.0 && (430.0..=620.0).contains(&(pa / ua).sqrt());
    let mut out = Vec::new();
    push_geoms_n(&mut out, &sm.positions, &sm.indices, sm.normals.as_deref(), Some(uvs), sm.colors.as_deref(), &layers[0], 0);
    for g in &mut out {
        g.colours1 = g
            .uvs
            .iter()
            .map(|&uv| {
                let [r, gg, b] = sample(uv);
                let s = (r + gg + b).max(1.0);
                let (w1, w2, w3) = (gg / s, b / s, (1.0 - (r + gg + b) / s).max(0.0));
                let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
                (q(w2 + w3) << 8) | (q(w1 + w3) << 16)
            })
            .collect();
        // Detail layers tile at TEXCOORD_0 x 64 (SixNG 6046d19: one repeat
        // per 8 m on map section tiles). Per-layer tiling (c35e27af) is not
        // decoded yet (SixNG a225c66).
        if section_tile {
            for uv in &mut g.uvs {
                *uv = [uv[0] * DETAIL_TILE, uv[1] * DETAIL_TILE];
            }
        } else {
            for (uv, p) in g.uvs.iter_mut().zip(&g.positions) {
                *uv = [p[0] / 8.0, p[1] / 8.0];
            }
        }
        // Vertex colours as in GTA V's own terrain drawables: Colour0 =
        // (ambient, 0, 0, 255), Colour1 = (0, G, B, 0).
        for c in &mut g.colours {
            *c = (*c & 0xFF) | 0xFF00_0000;
        }
        g.layers = Some(layers.clone());
        g.layer_bumps = bumps.clone();
    }
    let mut texs: Vec<String> = Vec::new();
    for l in layers.iter().chain(&bumps).chain([&ydr::FLAT_SPEC.to_string()]) {
        if !texs.contains(l) {
            texs.push(l.clone());
        }
    }
    Some((out, texs))
}

/// 6m two-layer blend shaders (GTA6-Asset-Info 12-materials.md "Two-layer
/// multilayer shaders"; SixNG b412ee7 / eeb3076 / e63f21d): standard_2lyr_pxm
/// b217b263, _pxm_ground 1251cae5, _ground cd8ceb5e, standard_2lyr 77d8ab27
/// (layer 2 weight = COLOR0.a) and prop_2lyr_batch 89c8fe07 (weight =
/// 1 - COLOR0.a).
const TWO_LAYER: [u32; 4] = [0xb217_b263, 0x1251_cae5, 0xcd8c_eb5e, 0x77d8_ab27];
const TWO_LAYER_INV: u32 = 0x89c8_fe07;
const LYR0_DIFFUSE: u32 = 0x3463_94ea;
const LYR1_DIFFUSE: u32 = 0x7d4f_12e2;
const LYR0_NORMAL: u32 = 0x84c6_9350;
const LYR1_NORMAL: u32 = 0x58b6_e707;

/// Two-layer submesh as terrain_cb_w_4lyr geometries, GTA V's vertex-blend
/// shader: samplers 0/1 = base layer, 2/3 = layer 2, Colour1.g = the layer 2
/// weight (B = 0, so the shader's bilinear blend reduces to base/layer 2).
/// Was: base layer only, layer 2 dropped. Not carried: the grime overlay
/// (dirtdiffusetex on TEXCOORD_2; the terrain shader has no overlay
/// sampler) and layer 2's own UV set (TEXCOORD_1; one UV set here). None
/// when either layer's colour does not resolve or the weight never leaves 0.
fn two_layer_geoms(pools: &Pools, mat: &crate::model::Material, key_of: &dyn Fn(&crate::model::TexRef) -> u64, sm: &crate::model::Mesh) -> Option<(Vec<ydr::Geom>, Vec<String>)> {
    let inv = mat.shader_hash == TWO_LAYER_INV;
    let uvs = sm.uvs.as_deref()?;
    let cols = sm.colors.as_deref()?;
    let slot = |p: u32| mat.textures.iter().find(|t| t.param == p);
    let colour = |p: u32| slot(p).and_then(|t| pools.get(t.hash, key_of(t), t.h64).ok()).map(|p| tex_name(&p));
    let (base, top) = (colour(LYR0_DIFFUSE)?, colour(LYR1_DIFFUSE)?);
    let weight = |c: u32| { let a = c >> 24; if inv { 255 - a } else { a } };
    if base == top || !sm.indices.iter().any(|&i| cols.get(i as usize).is_some_and(|&c| weight(c) > 0)) {
        return None;
    }
    let flat = flat_bump(pools).ok()?;
    let bump = |p: u32| slot(p).and_then(|t| pools.get_normal(t.hash, key_of(t), t.h64).ok()).unwrap_or_else(|| flat.clone());
    let (b0, b1) = (bump(LYR0_NORMAL), bump(LYR1_NORMAL));
    let layers = [base.clone(), base, top.clone(), top];
    let bumps = [b0.clone(), b0, b1.clone(), b1];
    let mut out = Vec::new();
    push_geoms_n(&mut out, &sm.positions, &sm.indices, sm.normals.as_deref(), Some(uvs), Some(cols), &layers[0], 0);
    for g in &mut out {
        g.colours1 = g.colours.iter().map(|&c| weight(c) << 8).collect();
        for c in &mut g.colours {
            *c = (*c & 0xFF) | 0xFF00_0000;
        }
        g.layers = Some(layers.clone());
        g.layer_bumps = bumps.clone();
    }
    let mut texs: Vec<String> = Vec::new();
    for l in layers.iter().chain(&bumps).chain([&ydr::FLAT_SPEC.to_string()]) {
        if !texs.contains(l) {
            texs.push(l.clone());
        }
    }
    Some((out, texs))
}

/// Bone hash of each triangle of `sm` (its cluster's palette bone).
fn tri_bone_hashes(sm: &crate::model::Mesh) -> Option<Vec<u32>> {
    let (tb, pal) = (sm.tri_bones.as_ref()?, sm.palette.as_ref()?);
    Some(tb.iter().map(|&b| pal.get(b as usize).copied().unwrap_or(0)).collect())
}

/// Per-instance decal variants: sibling leaf bones whose triangles are all
/// in the decal bucket (6m prop_am_uld_02a: bones AirEmu, Caipira, FlyUs,
/// GoPostal, Sahara, Yankee under rsBone, each holding one airline logo
/// quad over the same spot of one decal geometry). The game shows one of
/// them per placed instance; drawing every bone stacked all logos. Returns
/// the hashes of all such bones.
fn variant_bones(m: &crate::model::Model) -> Vec<Vec<u32>> {
    let Some(joints) = m.joints.as_ref() else { return Vec::new() };
    let mut decal: HashMap<u32, bool> = HashMap::new();
    for sm in m.meshes.iter() {
        let Some(hs) = tri_bone_hashes(sm) else { continue };
        let is_decal = sm.shader.and_then(|s| m.materials.as_ref()?.get(s)).is_some_and(|mt| kind_of(mt.bucket) == 2);
        for h in hs {
            let e = decal.entry(h).or_insert(true);
            *e &= is_decal;
        }
    }
    let leaf = |i: usize| !joints.iter().any(|j| j.parent == Some(i));
    let mut sets: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
    for (i, j) in joints.iter().enumerate() {
        if let (Some(p), Some(true)) = (j.parent, decal.get(&j.hash)) {
            if leaf(i) {
                sets.entry(p).or_default().push(j.hash);
            }
        }
    }
    sets.into_values().filter(|v| v.len() >= 2).collect()
}

/// The variant bone kept for palette row `row` (placement tint value):
/// `(set size, kept bone hash)`. Which 6m entity field picks the variant is
/// not decoded yet; the tint value is the per-instance value that exists,
/// so variant = row mod set size (skeleton order), one per instance.
fn decal_variants(m: &crate::model::Model, row: usize) -> Option<(usize, u32)> {
    let sets = variant_bones(m);
    let set = sets.first()?;
    Some((set.len(), set[row % set.len()]))
}

/// `sm` without the triangles of variant bones other than `kept`.
fn keep_variant(sm: &crate::model::Mesh, set: &[u32], kept: u32) -> crate::model::Mesh {
    let mut out = sm.clone();
    let Some(hs) = tri_bone_hashes(sm) else { return out };
    let drop = |h: u32| h != kept && set.contains(&h);
    out.indices = sm.indices.chunks_exact(3).zip(hs).filter(|(_, h)| !drop(*h)).flat_map(|(t, _)| t.iter().copied()).collect();
    out.tri_bones = None;
    out
}

/// rpf9 decoder path; None when the model does not decode (or is skinned).
/// Also returns the model's lower LOD levels (Med/Low/VLow, empty when
/// absent) and its LodGroup distances.
/// `row` picks decal variants; `tint` the palette row and colour rounding;
/// `ma` adds RDR2 `_ma` maps from the speculartex (`Geom::spec`, `redm prop`).
fn rpf9_mesh(ctx: &Ctx, pools: &Pools, name: &str, row: usize, tint: TintPick, ma: bool, an: &str, off: usize, entry: Option<&Entry>) -> Result<Option<(Built, Vec<Mesh>, [f32; 4])>, String> {
    let ext_palettes = std::cell::OnceCell::new();
    // first speculartex fallback reported (one line, not one per LOD)
    let ma_err = std::cell::Cell::new(false);
    let an = an.to_string();
    let models = { let (m, o) = ctx.src_map(off); crate::model::load_models(m, o) };
    let Some(m) = pick_model(&models, name) else {
        return Ok(None);
    };
    if m.bones && !m.fragment {
        return Ok(None);
    }
    let frag_out = m.fragment && {
        let (dl, dh) = m.declared;
        let pad = 0.05 * (0..3).map(|k| dh[k] - dl[k]).fold(1.0f32, f32::max);
        (0..3).any(|k| m.min[k] < dl[k] - pad || m.max[k] > dh[k] + pad)
    };
    let mut levels: Vec<Mesh> = (0..4).map(|_| Mesh { name: an.clone(), geoms: Vec::new() }).collect();
    let (mut texs, mut missing) = (Vec::new(), Vec::new());
    let key_of = |t: &crate::model::TexRef| m.embedded.as_ref().and_then(|e| e.by_hash(t.hash)).and_then(|x| x.asset_key).unwrap_or(t.key);
    let picked = decal_variants(m, row);
    if let Some((n, kept)) = &picked {
        super::quality::hit("decal_variant", 1, || format!("{name}: {n} variant bones, row {row} keeps bone {kept:08x}"));
    }
    let owned: Vec<Vec<crate::model::Mesh>>;
    let srcs: [&Vec<crate::model::Mesh>; 4] = match &picked {
        Some((_, kept)) => {
            let drop = variant_bones(m).swap_remove(0);
            owned = [&m.meshes, &m.lower[0], &m.lower[1], &m.lower[2]].iter().map(|l| l.iter().map(|sm| keep_variant(sm, &drop, *kept)).collect()).collect();
            [&owned[0], &owned[1], &owned[2], &owned[3]]
        }
        None => [&m.meshes, &m.lower[0], &m.lower[1], &m.lower[2]],
    };
    for (mesh, sms) in levels.iter_mut().zip(srcs) {
    for sm in sms.iter() {
        if let Some(mat) = sm.shader.and_then(|s| m.materials.as_ref()?.get(s)).filter(|mt| FOUR_LAYER.contains(&mt.shader_hash)) {
            if let Some((geoms, lt)) = four_layer_geoms(&an, pools, mat, &key_of, sm) {
                mesh.geoms.extend(geoms);
                for t in lt {
                    if !texs.contains(&t) {
                        texs.push(t);
                    }
                }
                continue;
            }
        }
        if let Some(mat) = sm.shader.and_then(|s| m.materials.as_ref()?.get(s)).filter(|mt| TWO_LAYER.contains(&mt.shader_hash) || mt.shader_hash == TWO_LAYER_INV) {
            if let Some((geoms, lt)) = two_layer_geoms(pools, mat, &key_of, sm) {
                mesh.geoms.extend(geoms);
                for t in lt {
                    if !texs.contains(&t) {
                        texs.push(t);
                    }
                }
                continue;
            }
        }
        // Colour slots in priority order, then every other texture: the first
        // one that decodes to a usable colour map wins (a colour slot can hold
        // a normal map or a 4x4 default).
        let tex = sm.shader.and_then(|s| m.materials.as_ref()?.get(s)).map_or_else(
            || MISSING.to_string(),
            |mat| {
                let cands: Vec<_> = COLOR_SLOTS.iter().filter_map(|slot| mat.textures.iter().find(|t| t.param == *slot)).collect();
                let mut errs = Vec::new();
                for t in cands {
                    let key = m.embedded.as_ref().and_then(|e| e.by_hash(t.hash)).and_then(|x| x.asset_key).unwrap_or(t.key);
                    match pools.get(t.hash, key, t.h64) {
                        // fallback (non-colour slot): AO/roughness maps are
                        // grayscale and pass bad_colour; never use them as diffuse
                        Ok(p) if !COLOR_SLOTS.contains(&t.param) && near_gray(pools, &p) => errs.push(format!("{:08x}: grayscale map in non-colour slot", t.hash)),
                        Ok(p) => {
                            super::quality::hit("colour_pick", 1, || format!("{name} mat {:08x} slot {:08x} -> {:08x}", mat.shader_hash, t.param, t.hash));
                            return tex_name(&p);
                        }
                        Err(e) => {
                            if errs.is_empty() {
                                super::quality::missing(t.h64, key, t.hash, name, &e);
                            }
                            errs.push(format!("{:08x} h64 {:016x}: {e}", t.hash, t.h64))
                        }
                    }
                }
                missing.extend(errs.into_iter().take(1));
                MISSING.to_string()
            },
        );
        // 6m glass families (sixng material.rs) -> glass_env.sps (alpha bucket)
        // with the material's own diffusetex (its alpha is the pane opacity).
        let glass = sm.shader.and_then(|s| m.materials.as_ref()?.get(s)).is_some_and(|mat| is_glass(mat.shader_hash));
        if tex != MISSING && !texs.contains(&tex) {
            texs.push(tex.clone());
        }
        let kind = sm.shader.and_then(|s| m.materials.as_ref()?.get(s)).map_or(0, |mat| if glass { 5 } else if kind_of(mat.bucket) == 0 && is_emissive(mat.shader_hash) { 4 } else { kind_of(mat.bucket) });
        // normal map (opaque only): normal_spec.sps with the material's bump texture
        let nrm = if (kind != 0 && kind != 5) || tex == MISSING {
            None
        } else {
            sm.shader.and_then(|s| m.materials.as_ref()?.get(s)).and_then(|mat| {
                mat.textures.iter().filter(|t| NORMAL_SLOTS.contains(&t.param)).find_map(|t| {
                    let key = m.embedded.as_ref().and_then(|e| e.by_hash(t.hash)).and_then(|x| x.asset_key).unwrap_or(t.key);
                    pools.get_normal(t.hash, key, t.h64).ok()
                })
            })
        };
        let nrm = if kind == 5 && tex != MISSING { nrm.or_else(|| flat_bump(pools).ok()) } else { nrm };
        if kind == 5 && tex != MISSING {
            let _ = pools.flat_spec();
        }
        if let Some(n) = &nrm {
            for t in [n.clone(), ydr::FLAT_SPEC.to_string()] {
                if !texs.contains(&t) {
                    texs.push(t);
                }
            }
        }
        // RDR2 `_ma` of this material (opaque geometries with a normal map
        // become `standard`); None binds the vanilla blank
        let spec = if ma && kind == 0 && nrm.is_some() {
            sm.shader.and_then(|s| m.materials.as_ref()?.get(s)).and_then(|mat| mat.textures.iter().find(|t| t.param == SPEC_SLOT)).and_then(|t| {
                pools.get_ma(t.hash, key_of(t), t.h64).map_err(|e| if !ma_err.replace(true) { eprintln!("redm: {name}: speculartex {:016x}: {e} (blank_ma)", t.h64) }).ok()
            })
        } else {
            None
        };
        if let Some(s) = spec.as_ref().filter(|s| !texs.contains(s)) {
            texs.push(s.clone());
        }
        let g0 = mesh.geoms.len();
        // Tint palette: bake palette entry (placement row, column = vertex colour
        // blue) into tinted copies of the colour texture, one per colour.
        let palette = shader_palette(ctx, m, sm, entry, &ext_palettes);
        if let (Some(pal), Some(cols)) = (palette.as_ref(), sm.colors.as_deref()) {
            let mut groups: Vec<([u8; 3], Vec<u32>)> = Vec::new();
            // `fivem build`: row 0. Reading tintValue as the row renders wrong
            // (rocks near black, later rows mostly ffbbff filler:
            // GTA6-Asset-Info 12-materials.md "Render check"), so the row
            // lookup is not understood; row 0 is the default scheme. `redm
            // prop --tint-row` picks another row by hand. Only shaders that
            // bind the palette themselves get here: LOD drawables without one
            // carry a COLOR0.b that is no palette index.
            let base = palette_index(tint.row, 0);
            for t in sm.indices.chunks_exact(3) {
                let c = tint_colour(cols.get(t[0] as usize).and_then(|c| pal.get(base + (c >> 16 & 255) as usize)), tint.exact);
                match groups.iter_mut().find(|g| g.0 == c) {
                    Some(g) => g.1.extend_from_slice(t),
                    None => groups.push((c, t.to_vec())),
                }
            }
            let fill = sm.indices.chunks_exact(3).filter(|t| cols.get(t[0] as usize).and_then(|c| pal.get(base + (c >> 16 & 255) as usize)).is_some_and(|p| [p[0], p[1], p[2]] == PALETTE_FILL)).count();
            if fill > 0 {
                super::quality::hit("palette_fill", fill as u64, || format!("{name}: {fill} of {} triangles (row {})", sm.indices.len() / 3, tint.row));
            }
            for (c, idx) in groups {
                let t = if c == [255; 3] { tex.clone() } else { tinted(pools, &tex, c).unwrap_or_else(|_| tex.clone()) };
                if t != MISSING && !texs.contains(&t) {
                    texs.push(t.clone());
                }
                push_geoms_n(&mut mesh.geoms, &sm.positions, &idx, sm.normals.as_deref(), sm.uvs.as_deref(), sm.colors.as_deref(), &t, kind);
            }
            if let Some(n) = &nrm {
                mesh.geoms[g0..].iter_mut().for_each(|g| g.normal = n.clone());
            }
            if let Some(s) = &spec {
                mesh.geoms[g0..].iter_mut().for_each(|g| g.spec = s.clone());
            }
            continue;
        }
        push_geoms_n(&mut mesh.geoms, &sm.positions, &sm.indices, sm.normals.as_deref(), sm.uvs.as_deref(), sm.colors.as_deref(), &tex, kind);
        if let Some(n) = nrm {
            mesh.geoms[g0..].iter_mut().for_each(|g| g.normal = n.clone());
        }
        if let Some(s) = spec {
            mesh.geoms[g0..].iter_mut().for_each(|g| g.spec = s.clone());
        }
    }
    }
    let mesh = levels.remove(0);
    Ok(Some(((mesh, texs, missing, frag_out), levels, m.lod_dist)))
}

/// Fragment path through sixng-core's decoder: LOD0 models of the main
/// drawable (fragment children when it has none), raw bind pose, no physics.
fn sixng_mesh(ctx: &Ctx, pools: &Pools, an: &str, e: &Entry) -> Result<Built, String> {
    use sixng_core::{drawable, resource::Resource};
    let r = Resource::load(&ctx.entry_path(e), e).map_err(|x| x.to_string())?;
    let info = drawable::describe(&r);
    let slice = |i: usize| {
        let b = &info.buffers[i];
        let end = (b.gfx_off + b.bytes).min(r.gfx.len() as u64) as usize;
        &r.gfx[(b.gfx_off as usize).min(end)..end]
    };
    let main: Vec<_> = info.models.iter().filter(|m| !m.part && m.lod == 0).collect();
    let models = if main.iter().any(|m| !m.geometries.is_empty()) { main } else { info.models.iter().filter(|m| m.part).collect() };
    // (32-bit name hash, 64-bit key) of a shader texture: the key is at
    // descriptor +0x08, the name at -0x08 (+0x18 is the next texture's;
    // model/decode.rs read_materials).
    let own = |sh: &drawable::Shader, param: usize| -> Option<(u32, u64)> {
        let ta = r.sys_off(r.u64(sh.record_off as usize + 0x10))?;
        let t = r.sys_off(r.u64(ta + 8 * param))?;
        let d = r.sys_off(r.u64(t + 0x30))?.checked_sub(16)?;
        (r.u32(d) == 6).then(|| (r.u32(d + 8), r.u64(d + 24)))
    };
    let drawable = r.sys_off(r.u64(0x18)).unwrap_or(0);
    let emb = [drawable, 0].into_iter().find_map(|d| crate::texture::embedded_dictionary(&r.sys, &r.gfx, d));
    let mut mesh = Mesh { name: an.to_string(), geoms: Vec::new() };
    let (mut texs, mut missing) = (Vec::new(), Vec::new());
    for md in models {
        for (k, &gi) in md.geometries.iter().enumerate() {
            let g = &info.geometries[gi];
            let Some(h) = info.buffers[g.vertex_buffer].mesh_blob.as_ref() else { continue };
            let m = drawable::decode_mesh(slice(g.vertex_buffer), h, slice(g.index_buffer));
            if m.indices.is_empty() {
                continue;
            }
            let sh = md.shader_indices.get(k).and_then(|&s| info.shaders.get(s as usize));
            let tex = sh.map_or_else(
                || MISSING.to_string(),
                |sh| {
                    let ph = |t: &&sixng_core::drawable::TexRef| u32::from_str_radix(&t.param_hash, 16).ok();
                    let cands: Vec<_> = COLOR_SLOTS.iter().filter_map(|slot| sh.textures.iter().find(|t| ph(t) == Some(*slot))).collect();
                    let mut errs = Vec::new();
                    for t in cands {
                        let Some((hash, own_key)) = own(sh, t.param) else { continue };
                        let key = emb.as_ref().and_then(|e| e.by_hash(hash)).and_then(|x| x.asset_key).unwrap_or(own_key);
                        match pools.get(hash, key, own_key) {
                            Ok(p) => {
                                super::quality::hit("colour_pick", 1, || format!("{an} shader {:?} slot {} -> {hash:08x}", sh.name_hash, t.param_hash));
                                return tex_name(&p);
                            }
                            Err(e) => {
                                if errs.is_empty() {
                                    super::quality::missing(own_key, key, hash, an, &e);
                                }
                                errs.push(format!("{hash:08x} h64 {own_key:016x}: {e}"))
                            }
                        }
                    }
                    missing.extend(errs.into_iter().take(1));
                    MISSING.to_string()
                },
            );
            if tex != MISSING && !texs.contains(&tex) {
                texs.push(tex.clone());
            }
            let uv = m.uvs.iter().find(|u| u.0 == 3).map(|u| u.1.as_slice());
            let col: Option<Vec<u32>> = m.colors.as_ref().map(|c| c.iter().map(|x| u32::from_le_bytes(*x)).collect());
            let kind = sh.map_or(0, |sh| kind_of(sh.flags.get(4).copied().unwrap_or(0)));
            push_geoms(&mut mesh.geoms, &m.positions, &m.indices, uv, col.as_deref(), &tex, kind);
        }
    }
    let declared = drawable::drawable_bounds(&r);
    let (lo, hi) = aabb(mesh.geoms.iter().flat_map(|g| g.positions.iter()));
    let out = declared.is_some_and(|[dl, dh]| {
        let pad = 0.05 * (0..3).map(|k| dh[k] - dl[k]).fold(1.0f32, f32::max);
        (0..3).any(|k| lo[k] < dl[k] - pad || hi[k] > dh[k] + pad)
    });
    Ok((mesh, texs, missing, out))
}

fn convert(ctx: &Ctx, pools: &Pools, name: &str, off: usize, entry: Option<&Entry>) -> Result<Option<Arch>, String> {
    let an = safe_name(name);
    // fragment props become dynamic .yft (fivem/frags.rs); FRAG_V re-converts only them
    let frag = entry.is_some_and(|e| e.rtype.as_deref() == Some("ft"));
    let file = if frag { format!("{an}.yft") } else { format!("{an}.ydr") };
    // Four-layer ground models carry their own writer tag, so a change to
    // that path re-converts only them (shader records: name hash + "atem").
    let lyr4 = { let (m, o) = ctx.src_map(off); crate::model::system_page(m, o) }.is_some_and(|sys| sys.windows(8).any(|w| &w[4..] == b"atem" && FOUR_LAYER.contains(&u32::from_le_bytes([w[0], w[1], w[2], w[3]]))));
    let fp = if lyr4 { format!("{YDR_V}|{LYR4_V}|bld|{off:x}|{name}") } else { format!("{YDR_V}|bld|{off:x}|{name}") };
    let fp = if name.starts_with("b6m_") { format!("{}|{fp}", super::batches::BATCH_V) } else { fp };
    let fp = if frag { format!("{FRAG_V}|{fp}") } else { fp };
    let from_info = |i: &Value| Arch {
        an: an.clone(),
        lo: f3(&i["lo"]),
        hi: f3(&i["hi"]),
        texs: i["texs"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect()).unwrap_or_default(),
        alpha: i["alpha"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect()).unwrap_or_default(),
    };
    // Writer version stored in every entry; an entry without it (or, for a
    // fragment, without its verify record) predates the check: rebuild it.
    let writer = if frag { format!("{FRAG_V}|{YDR_V}") } else { YDR_V.to_string() };
    let trusted = |i: &Value| i.get("alpha").is_some() && i["writer"].as_str() == Some(writer.as_str()) && (!frag || i.get("t6m_vehicles").is_some());
    if let Some(info) = ctx.up_to_date(&file, &fp).filter(trusted) {
        let a = from_info(&info);
        if a.texs.iter().all(|t| pools.store.exists(&pools.dir.join(format!("{}.dds", &t[4..])))) {
            // per-file stats stored with the cache entry (older entries: unknown)
            let lods = info["lods"].as_u64().map_or("unknown (cached before stats)".to_string(), |n| n.to_string());
            super::regionlog::bump_n("ydr_lod_levels", &format!("{lods} (cached)"), &an, info["bytes"].as_u64().unwrap_or(0));
            if frag {
                super::vlog::cached(&file, info.get("t6m_vehicles"));
            }
            let s = ctx.emit(&file, &fp, info, None)?;
            ctx.report.lock().unwrap().converted.push(json!({"kind": "building", "name": an, "source": ctx.src_id(off), "ydr": s}));
            return Ok(Some(a));
        }
    }
    let src = ctx.src_id(off);
    let batch = super::batches::def(name);
    let (base, row) = match &batch {
        Some(d) => (d.model.as_str(), 0),
        None => split_tint(name),
    };
    let rpf = rpf9_mesh(ctx, pools, base, row, TintPick::BUILD, false, &an, off, entry)?;
    let mut lower: Vec<Mesh> = Vec::new();
    let mut lod_dist = [9998.0f32; 4];
    let (mesh, texs, missing, frag_out) = match (rpf, entry) {
        (Some((x, lw, ld)), _) => {
            (lower, lod_dist) = (lw, ld);
            x
        }
        (None, Some(e)) => match sixng_mesh(ctx, pools, &an, e) {
            Ok(x) => x,
            Err(err) => {
                ctx.skip(name, "model does not decode", json!({"source": src, "error": err}));
                return Ok(None);
            }
        },
        (None, None) => {
            ctx.skip(name, "model does not decode", json!({"source": src}));
            return Ok(None);
        }
    };
    let (mut mesh, mut texs) = (mesh, texs);
    promote_base_layer(&mut mesh);
    lower.iter_mut().for_each(promote_base_layer);
    if batch.is_some() {
        lower.clear();
    }
    if let Some(d) = &batch {
        (mesh, texs) = super::batches::bake(d, &mesh, &|t, c| tinted(pools, t, c).unwrap_or_else(|_| t.to_string()));
        texs.retain(|t| t != MISSING);
    }
    let is_frag = entry.is_some_and(|e| e.rtype.as_deref() == Some("ft"));
    if mesh.geoms.is_empty() {
        ctx.skip(name, "no triangles", json!({"source": ctx.src_id(off)}));
        return Ok(None);
    }
    if !missing.is_empty() {
        ctx.skip(name, "missing textures (placeholder used)", json!({"textures": missing}));
    }
    let (lo, hi) = aabb(mesh.geoms.iter().flat_map(|g| g.positions.iter()));
    let cols = entry.filter(|_| frag).map(|e| super::frags::colliders(&ctx.entry_path(e), e)).unwrap_or_default();
    // A fragment's drawable bounds (+0x30/+0x40) are not in the frame its
    // geometry and colliders share (doors: centred on the panel while the
    // mesh and the bone-placed colliders start at the hinge), so a fragment
    // with 6m colliders is flagged only when its mesh misses their union.
    let (frag_out, delta) = if cols.is_empty() {
        (frag_out, None)
    } else {
        let (cl, ch) = cols.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(a, b), c| (std::array::from_fn(|k| a[k].min(c.min[k])), std::array::from_fn(|k| b[k].max(c.max[k]))));
        let pad = 0.05 * (0..3).map(|k| ch[k] - cl[k]).fold(1.0f32, f32::max);
        // per axis: how far the mesh box lies outside the collider union
        let delta: [f32; 3] = std::array::from_fn(|k| (cl[k] - lo[k]).max(hi[k] - ch[k]).max(0.0));
        ((0..3).any(|k| lo[k] > ch[k] + pad || hi[k] < cl[k] - pad), Some(delta))
    };
    if frag_out {
        ctx.skip(name, "fragment geometry outside its collision (converted anyway, check placement)", json!({"source": src}));
    }
    let bytes = match entry.filter(|_| frag) {
        Some(_) => super::frags::build_prop_yft(&mesh, &cols, 9998.0),
        // the source model's own LOD levels and distances (LodGroup +0x50..+0x7C)
        None if lower.iter().any(|l| !l.geoms.is_empty()) => {
            let mut lv = vec![mesh.clone()];
            lv.extend(lower.iter().cloned());
            ydr::build_ydr_lods(&lv, lod_dist, None)
        }
        None => ydr::build_ydr(&mesh, 9998.0, None),
    }
    .map_err(|e| format!("{file}: {e}"))?;
    let n_lod = 1 + lower.iter().filter(|l| !l.geoms.is_empty()).count();
    super::regionlog::bump_n("ydr_lod_levels", &format!("{n_lod} (fresh)"), &an, bytes.len() as u64);
    let alpha: std::collections::BTreeSet<&String> = mesh.geoms.iter().filter(|g| g.kind != 0).map(|g| &g.texture).collect();
    let mut info = json!({"lo": lo, "hi": hi, "texs": texs, "alpha": alpha, "writer": writer, "lods": n_lod, "bytes": bytes.len()});
    if frag {
        // [t6m:vehicles] record, kept in the cache info for cached builds
        info["t6m_vehicles"] = super::vlog::fresh(&file, &bytes, cols.len().max(1), frag_out.then_some(delta));
    }
    let s = ctx.emit(&file, &fp, info.clone(), Some(bytes))?;
    ctx.report.lock().unwrap().converted.push(json!({"kind": "building", "name": an, "source": ctx.src_id(off), "ydr": s, "textures": texs.len(), "fragment": is_frag}));
    Ok(Some(from_info(&info)))
}

/// One 6m model converted to the in-memory writer inputs, outside a build.
pub struct PropSource {
    /// LOD levels (High first; Med/Low/VLow may be empty), named `r6m_<name>`.
    pub levels: Vec<Mesh>,
    pub lod_dist: [f32; 4],
    /// (texture name, DDS bytes) of every texture the levels reference.
    pub textures: Vec<(String, Arc<Vec<u8>>)>,
    /// textures that did not resolve (the geometry names `r6m_missing`)
    pub missing: Vec<String>,
    /// the sixng index entry kind: "dr" drawable or "ft" fragment
    pub rtype: String,
    /// skinned drawable (bones, not a fragment): the rpf9 path skips it and
    /// the levels come from sixng-core's decoder in bind pose
    pub skinned: bool,
}

/// Several props converted with one levels index, archive map and texture
/// pool (texture names stay consistent between the props: one texture,
/// one `r6m_<slot>` name).
pub struct PropBatch {
    ctx: Ctx,
    index: ArchiveIndex,
    pools: Pools,
}

impl PropBatch {
    /// levels.rpf plus the pack archives (PACK_ARCHIVES) that are indexed.
    pub fn open(game: &Path, work: &Path) -> Result<PropBatch, String> {
        let (index, _) = levels_index(work)?;
        let mut ctx = super::single_ctx(game, work)?;
        ctx.packs = super::open_packs(game, work, "its models are not converted");
        Ok(PropBatch { ctx, index, pools: Pools::new(game, super::texmem::Store::temp()) })
    }

    /// Archive ids whose models `source_model` can load.
    pub fn archives(&self) -> Vec<String> {
        std::iter::once("levels.rpf".to_string()).chain(self.ctx.packs.iter().map(|p| p.id.clone())).collect()
    }

    /// The model a placement names by sixng model id ("<archive>@<hex>"
    /// or "<archive>@<hex>#<member>", as `fivem build` resolves placements:
    /// `Ctx::voff_of`, then the index entry at that offset). `name` picks
    /// the member / model inside the resource (`pick_model`; the largest
    /// model when none has that name) and names the levels (`r6m_<name>`).
    pub fn source_model(&self, name: &str, model_id: &str, tint: TintPick) -> Result<PropSource, String> {
        let off = self.ctx.voff_of(model_id).ok_or_else(|| format!("{model_id}: archive not loaded (index it with `sixng index <archive>`)"))?;
        let (index, raw) = match off >> super::PACK_SHIFT {
            0 => (&self.index, off),
            s => (&self.ctx.packs[s - 1].index, off & ((1usize << super::PACK_SHIFT) - 1)),
        };
        let e = index.entries.iter().find(|e| e.off as usize == raw).ok_or_else(|| format!("{model_id}: no index entry at that offset"))?;
        source_of(&self.ctx, &self.pools, e, off, name, tint)
    }
}

/// Decode the levels.rpf drawable (or fragment main drawable, bind pose)
/// `name` from the sixng index in `<work>/index/levels.json`, with the same
/// mesh and texture path as `fivem build` (rpf9 decoder first, sixng-core
/// for fragments it does not take; colour, tint and normal maps).
/// `tint`: palette row (checked against `prop_tints`) and colour rounding;
/// decal variants stay on row 0.
pub fn prop_source(game: &Path, work: &Path, name: &str, tint: TintPick) -> Result<PropSource, String> {
    let (ctx, e) = prop_entry(game, work, name)?;
    let pools = Pools::new(game, super::texmem::Store::temp());
    source_of(&ctx, &pools, &e, e.off as usize, name, tint)
}

/// `off`: virtual offset of `e` (`Ctx::src_map`; = e.off in levels.rpf).
fn source_of(ctx: &Ctx, pools: &Pools, e: &Entry, off: usize, name: &str, tint: TintPick) -> Result<PropSource, String> {
    let want = name.to_ascii_lowercase();
    if tint.row > 0 {
        check_tint_row(name, &prop_tints_of(ctx, e, off, &want)?, tint.row)?;
    }
    let an = safe_name(name);
    let mut skinned = false;
    let (mut mesh, texs, missing, mut lower, lod_dist) = match rpf9_mesh(ctx, pools, &want, 0, tint, true, &an, off, Some(e))? {
        Some(((m, t, x, _), lw, ld)) => (m, t, x, lw, ld),
        None => {
            let models = { let (m, o) = ctx.src_map(off); crate::model::load_models(m, o) };
            skinned = pick_model(&models, &want).is_some_and(|m| m.bones && !m.fragment);
            let (m, t, x, _) = sixng_mesh(ctx, pools, &an, e)?;
            (m, t, x, Vec::new(), [9998.0; 4])
        }
    };
    promote_base_layer(&mut mesh);
    lower.iter_mut().for_each(promote_base_layer);
    if mesh.geoms.is_empty() {
        return Err(format!("{name}: no triangles"));
    }
    let mut textures = Vec::new();
    for t in texs.iter().filter(|t| t.as_str() != MISSING) {
        let b = pools.load(&pools.dir.join(format!("{}.dds", &t[4..])))?;
        textures.push((t.clone(), b));
    }
    let mut levels = vec![mesh];
    levels.extend(lower);
    Ok(PropSource { levels, lod_dist, textures, missing, rtype: e.rtype.clone().unwrap_or_default(), skinned })
}

/// The levels.rpf sixng index (<work>/index/levels.json) and its path.
fn levels_index(work: &Path) -> Result<(ArchiveIndex, PathBuf), String> {
    let ip = work.join("index").join("levels.json");
    let index = sixng_core::scan::load_index(&ip).map_err(|e| format!("{}: {e:#} (run `6ixpack sixng index levels --game .. --work ..` first)", ip.display()))?;
    Ok((index, ip))
}

/// Index entry (drawable first, else fragment) of model `name`.
fn find_prop<'a>(index: &'a ArchiveIndex, ip: &Path, name: &str) -> Result<&'a Entry, String> {
    let of = |t: &str| index.entries.iter().find(|e| e.rtype.as_deref() == Some(t) && e.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name)));
    of("dr").or_else(|| of("ft")).ok_or_else(|| format!("{name}: no drawable or fragment of that name in {}", ip.display()))
}

/// levels.rpf index entry (drawable first, else fragment) of `name`, and a
/// single-model context.
fn prop_entry(game: &Path, work: &Path, name: &str) -> Result<(Ctx, Entry), String> {
    let (index, ip) = levels_index(work)?;
    let e = find_prop(&index, &ip, name)?.clone();
    Ok((super::single_ctx(game, work)?, e))
}

/// One tint palette of a model and the palette columns (vertex colour
/// blue) its LOD0 triangles use, as `rpf9_mesh` reads them.
pub struct PaletteUse {
    pub palette: Vec<[u8; 4]>,
    pub columns: Vec<usize>,
}

impl PaletteUse {
    pub fn rows(&self) -> usize {
        palette_rows(self.palette.len())
    }

    /// Colour of `column` in `row`; None when filler, white or past the end (untinted).
    pub fn colour(&self, row: usize, column: usize) -> Option<[u8; 3]> {
        let c = tint_colour(self.palette.get(palette_index(row, column)), true);
        (c != [255; 3]).then_some(c)
    }
}

/// Error unless every palette of the model has row `row`.
pub fn check_tint_row(name: &str, uses: &[PaletteUse], row: usize) -> Result<(), String> {
    let Some(rows) = uses.iter().map(PaletteUse::rows).min() else {
        return Err(format!("{name}: no tint palette embedded, only row 0 (untinted) exists"));
    };
    if row >= rows {
        return Err(format!("{name}: tint row {row} out of range, valid rows 0..={} (palette of {} entries)", rows - 1, uses.iter().map(|u| u.palette.len()).max().unwrap_or(0)));
    }
    Ok(())
}

/// Tint palettes `redm prop <name>` would bake (empty: none embedded, or
/// the model is not on the rpf9 decoder path, which alone bakes tints).
pub fn prop_tints(game: &Path, work: &Path, name: &str) -> Result<Vec<PaletteUse>, String> {
    let (ctx, e) = prop_entry(game, work, name)?;
    prop_tints_of(&ctx, &e, e.off as usize, &name.to_ascii_lowercase())
}

fn prop_tints_of(ctx: &Ctx, e: &Entry, off: usize, name: &str) -> Result<Vec<PaletteUse>, String> {
    let models = { let (m, o) = ctx.src_map(off); crate::model::load_models(m, o) };
    let Some(m) = pick_model(&models, name).filter(|m| !m.bones || m.fragment) else {
        return Ok(Vec::new());
    };
    let ext = std::cell::OnceCell::new();
    let mut out: Vec<PaletteUse> = Vec::new();
    for sm in &m.meshes {
        let (Some(pal), Some(cols)) = (shader_palette(ctx, m, sm, Some(e), &ext), sm.colors.as_deref()) else { continue };
        let at = match out.iter().position(|u| u.palette == pal) {
            Some(i) => i,
            None => {
                out.push(PaletteUse { palette: pal, columns: Vec::new() });
                out.len() - 1
            }
        };
        for t in sm.indices.chunks_exact(3) {
            if let Some(c) = cols.get(t[0] as usize).map(|c| (c >> 16 & 255) as usize) {
                if !out[at].columns.contains(&c) {
                    out[at].columns.push(c);
                }
            }
        }
    }
    out.iter_mut().for_each(|u| u.columns.sort_unstable());
    Ok(out)
}

pub fn build(ctx: &Ctx, index: &ArchiveIndex) -> Result<(), Box<dyn Error>> {
    let p = super::Phase::new(ctx, "  placements");
    let dir = placements_dir(ctx, index)?;
    let mut dirs = vec![(dir.clone(), "levels.rpf".to_string())];
    dirs.extend(pack_placements(ctx)?);
    // levels and pack tables are now made by this code
    if !derived_ok(ctx) {
        mark_derived(ctx);
    }
    let places = load_places(ctx, &dirs, index)?;
    // interiors as MLOs (default; --interiors flattens them into world space instead)
    let mlos = if ctx.o.interiors {
        Vec::new()
    } else {
        let mut v = interiors::load(ctx, &dir, index, "levels.rpf");
        // pack dirs are pushed in ctx.packs order
        for ((d, id), pk) in dirs[1..].iter().zip(&ctx.packs) {
            interiors::merge(&mut v, interiors::load(ctx, d, &pk.index, id));
        }
        v
    };
    // archetype name -> model offset (first wins), most used first
    let grass = super::batches::grass_archetypes(ctx, index);
    let mut use_count: HashMap<&str, (usize, usize)> = HashMap::new();
    for (n, o) in &grass {
        use_count.entry(n).or_insert((*o, 0));
    }
    for p in &places {
        use_count.entry(&p.name).or_insert((p.off, 0)).1 += 1;
    }
    for c in mlos.iter().flat_map(|i| &i.children) {
        use_count.entry(&c.name).or_insert((c.off, 0));
    }
    let mut arch: Vec<(&str, usize)> = use_count.iter().map(|(n, (o, _))| (*n, *o)).collect();
    arch.sort_by(|a, b| use_count[b.0].1.cmp(&use_count[a.0].1).then(a.0.cmp(b.0)));
    eprintln!("buildings: {} placements, {} archetypes", places.len(), arch.len());
    if arch.is_empty() {
        return Ok(());
    }
    // source textures: decoded in memory (4 GB held), records in the index
    let store = super::texmem::Store::open(ctx.o.work.join("index").join(format!("fivem_textures_v{}s2{}.json", sixng_core::bcpack::BCPACK_DECODER_VERSION, sixng_core::bcpack_presets::tag())), 4 << 30);
    let pools = Pools::new(&ctx.o.game, store);
    let cache = pools.dir.clone();
    let mut by_off: HashMap<usize, &Entry> = index.entries.iter().map(|e| (e.off as usize, e)).collect();
    for (s, pk) in ctx.packs.iter().enumerate() {
        by_off.extend(pk.index.entries.iter().map(|e| (e.off as usize | ((s + 1) << super::PACK_SHIFT), e)));
    }
    drop(p);
    let p = super::Phase::new(ctx, "  models (ydr)");
    let bar = super::progress::Bar::new("buildings", arch.len());
    let res: Vec<(String, Result<Option<Arch>, String>)> = arch
        .par_iter()
        .map(|&(n, off)| {
            let r = (n.to_string(), convert(ctx, &pools, n, off, by_off.get(&off).copied()));
            bar.tick();
            r
        })
        .collect();
    let mut done: BTreeMap<String, Arch> = BTreeMap::new();
    let mut names: HashMap<String, String> = HashMap::new();
    for (n, r) in res {
        if let Some(a) = r? {
            if let Some(prev) = names.insert(a.an.clone(), n.clone()) {
                return Err(format!("archetype name clash: {prev} / {n} -> {}", a.an).into());
            }
            done.insert(n, a);
        }
    }

    // Texture dictionaries. A texture used by one name family (first two
    // tokens) goes in that family's t6m_txdNNNN (<= 48 MB each); a texture
    // used by several families goes once into the shared chain
    // t6m_shr0000 <- t6m_shr0001 <- ... (gtxd.meta parents), and every family
    // txd's parent is the last shared txd, so nothing is stored twice.
    // Every texture ships at its full 6m size, mips and format; only an
    // explicit --max-tex drops top mips (the txd split handles size limits).
    let cap_of = |_: &str| -> u16 { ctx.o.max_tex.unwrap_or(u16::MAX) };
    let tsize = |t: &str| pools.store.len(&cache.join(format!("{}.dds", &t[4..])));
    // Page-packed graphics size of a texture set (pixel data = DDS minus
    // its header; the header bytes are left in as slack).
    let packed = |ts: &mut dyn Iterator<Item = &String>| -> u64 { crate::gta5::rsc::gfx_pages_size(&ts.map(|t| tsize(t)).collect::<Vec<_>>(), TXD_SYS_PAGES).unwrap_or(u64::MAX) };
    let mut fam: BTreeMap<String, Vec<&Arch>> = BTreeMap::new();
    for a in done.values() {
        fam.entry(a.an[4..].split('_').take(2).collect::<Vec<_>>().join("_")).or_default().push(a);
    }
    let mut users: HashMap<&String, HashSet<&String>> = HashMap::new();
    for (f, v) in &fam {
        for a in v {
            for t in &a.texs {
                users.entry(t).or_default().insert(f);
            }
        }
    }
    // Shallow txd tree from 6m's own sharing (depth <= 2, gtxd.meta), so a
    // texture (one 64-bit identity = one r6m_ name, fivem/texslot.rs) used by
    // several families is stored once instead of copied into each family
    // txd (a full build shared 0 of 47,229 textures):
    // - root t6m_shr_root: textures used by >= ROOT_USERS families, most
    //   used first, up to TXD_CAP;
    // - one group txd per exact set of families using a texture
    //   (t6m_shrNNNN, split at TXD_CAP), parent = root;
    // - each family txd's parent is the group holding most of its shared
    //   bytes; shared textures of other groups are copied into it.
    // The old linear chain (each shr the parent of the next) made every
    // building load all of it (~390 MB at the airport). Per model now: own
    // txd + one group + root, each <= TXD_CAP packed.
    const ROOT_USERS: usize = 8;
    let mut by_use: Vec<(&String, usize)> = users.iter().filter(|(_, f)| f.len() >= ROOT_USERS).map(|(t, f)| (*t, f.len())).collect();
    by_use.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let mut root: Vec<String> = Vec::new();
    for (t, _) in by_use {
        if packed(&mut root.iter().chain([t])) <= TXD_CAP {
            root.push(t.clone());
        }
    }
    root.sort();
    let in_root: HashSet<String> = root.iter().cloned().collect();
    let root_name = (!root.is_empty()).then(|| "t6m_shr_root".to_string());
    // (file stem, textures, parent)
    let mut dicts: Vec<(String, Vec<String>, Option<String>)> = Vec::new();
    if let Some(r) = &root_name {
        dicts.push((r.clone(), root, None));
    }
    let mut buckets: BTreeMap<Vec<&String>, Vec<&String>> = BTreeMap::new();
    for (t, f) in &users {
        if f.len() > 1 && !in_root.contains(*t) {
            let mut k: Vec<&String> = f.iter().copied().collect();
            k.sort();
            buckets.entry(k).or_default().push(*t);
        }
    }
    // texture -> group txd
    let mut group_of: HashMap<String, String> = HashMap::new();
    for (_, mut ts) in buckets {
        ts.sort();
        let mut parts: Vec<Vec<String>> = vec![Vec::new()];
        for t in ts {
            let last = parts.last().unwrap();
            if !last.is_empty() && packed(&mut last.iter().chain([t])) > TXD_CAP {
                parts.push(Vec::new());
            }
            parts.last_mut().unwrap().push(t.clone());
        }
        for part in parts {
            let name = format!("t6m_shr{:04}", dicts.len());
            for t in &part {
                group_of.insert(t.clone(), name.clone());
            }
            dicts.push((name, part, root_name.clone()));
        }
    }
    let shared_n = in_root.len() + group_of.len();
    // family -> parent: the group with most of its shared bytes, else root
    let parent_of = |v: &Vec<&Arch>| -> Option<String> {
        let mut score: HashMap<&String, u64> = HashMap::new();
        let mut seen = HashSet::new();
        for a in v {
            for t in &a.texs {
                if seen.insert(t) {
                    if let Some(g) = group_of.get(t) {
                        *score.entry(g).or_default() += tsize(t);
                    }
                }
            }
        }
        score.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0))).map(|x| x.0.clone()).or_else(|| root_name.clone())
    };
    let mut txd_of: HashMap<String, String> = HashMap::new();
    let mut fam_n = 0usize;
    let (mut cur, mut size): (Vec<String>, u64) = (Vec::new(), 0);
    let mut members: Vec<String> = Vec::new();
    let mut cur_parent: Option<String> = None;
    let n_fam = fam.len();
    for (fi, (_, mut v)) in fam.into_iter().enumerate() {
        v.sort_by(|a, b| a.an.cmp(&b.an));
        let par = parent_of(&v);
        // small families are packed together only under the same parent
        if par != cur_parent && !members.is_empty() {
            if cur.is_empty() {
                for m in members.drain(..) {
                    txd_of.insert(m, cur_parent.clone().unwrap_or_default());
                }
            } else {
                let name = format!("t6m_txd{fam_n:04}");
                fam_n += 1;
                for m in members.drain(..) {
                    txd_of.insert(m, name.clone());
                }
                dicts.push((name, std::mem::take(&mut cur), cur_parent.clone()));
            }
            size = 0;
        }
        cur_parent = par.clone();
        let own = |t: &&String| !in_root.contains(*t) && group_of.get(*t) != par.as_ref();
        for a in v {
            let mut new: Vec<&String> = a.texs.iter().filter(|t| own(t) && !cur.contains(t)).collect();
            if !cur.is_empty() && packed(&mut cur.iter().chain(new.iter().copied())) > TXD_CAP {
                let name = format!("t6m_txd{fam_n:04}");
                fam_n += 1;
                for m in members.drain(..) {
                    txd_of.insert(m, name.clone());
                }
                dicts.push((name, std::mem::take(&mut cur), par.clone()));
                size = 0;
                new = a.texs.iter().filter(|t| own(t)).collect();
            }
            size += new.iter().map(|t| tsize(t)).sum::<u64>();
            cur.extend(new.into_iter().cloned());
            members.push(a.an.clone());
        }
        if size < TXD_PACK && fi + 1 < n_fam {
            continue;
        }
        if cur.is_empty() {
            // everything is in its parent: use the parent, no empty ytd
            for m in members.drain(..) {
                txd_of.insert(m, par.clone().unwrap_or_default());
            }
            continue;
        }
        let name = format!("t6m_txd{fam_n:04}");
        fam_n += 1;
        for m in members.drain(..) {
            txd_of.insert(m, name.clone());
        }
        dicts.push((name, std::mem::take(&mut cur), par.clone()));
        size = 0;
    }
    // Over-cap dictionaries (one archetype alone can exceed TXD_CAP; the
    // packing above only splits between archetypes, leaving 300+ ytds over
    // FiveM's 16 MB streaming limit, evicted: blur / fallbacks). Keep the
    // first TXD_CAP in the archetype's own txd and chain the rest as parents
    // (gtxd.meta): <stem> <- <stem>_p1 <- <stem>_p2 ... <- original parent.
    // A single texture over the cap still ships whole (no mip drop).
    {
        let mut extra: Vec<(String, Vec<String>, Option<String>)> = Vec::new();
        for d in dicts.iter_mut() {
            if packed(&mut d.1.iter()) <= TXD_CAP {
                continue;
            }
            let mut list = std::mem::take(&mut d.1);
            list.sort_by(|a, b| tsize(b).cmp(&tsize(a)).then(a.cmp(b)));
            let mut parts: Vec<Vec<String>> = vec![Vec::new()];
            for t in list {
                let last = parts.last().unwrap();
                if !last.is_empty() && packed(&mut last.iter().chain([&t])) > TXD_CAP {
                    parts.push(Vec::new());
                }
                parts.last_mut().unwrap().push(t);
            }
            let top = d.2.take();
            let n = parts.len();
            let mut it = parts.into_iter();
            d.1 = it.next().unwrap_or_default();
            d.2 = (n > 1).then(|| format!("{}_p1", d.0)).or(top.clone());
            for (k, part) in it.enumerate() {
                let k = k + 1;
                let par = if k + 1 < n { Some(format!("{}_p{}", d.0, k + 1)) } else { top.clone() };
                extra.push((format!("{}_p{k}", d.0), part, par));
            }
        }
        dicts.extend(extra);
    }
    let total: usize = dicts.iter().map(|d| d.1.len()).sum();
    eprintln!("buildings: {} textures ({} shared) in {} dictionaries", total, shared_n, dicts.len());
    {
        let mut by: Vec<(&String, usize, u64)> = dicts.iter().map(|d| (&d.0, d.1.len(), d.1.iter().map(|t| tsize(t)).sum())).collect();
        by.sort_by(|a, b| b.2.cmp(&a.2));
        let mb = |b: u64| (b as f64 / 1048576.0 * 10.0).round() / 10.0;
        let mut fams: HashMap<&String, usize> = HashMap::new();
        for v in txd_of.values() {
            *fams.entry(v).or_default() += 1;
        }
        super::regionlog::set("txd_per_family", json!({"dictionaries": dicts.len(), "textures": total, "shared": shared_n, "models_per_txd_max": fams.values().max().copied().unwrap_or(0),
            "largest": by.iter().take(5).map(|d| json!({"txd": d.0, "textures": d.1, "mb": mb(d.2), "models": fams.get(d.0).copied().unwrap_or(0)})).collect::<Vec<_>>()}));
    }
    let alpha_tex: HashSet<&String> = done.values().flat_map(|a| &a.alpha).collect();
    drop(p);
    let p = super::Phase::new(ctx, "  textures (ytd)");
    let bar = super::progress::Bar::new("textures", dicts.len());
    let results: Vec<Result<String, String>> = dicts
        .par_iter()
        .map(|(stem, list, _)| {
            let _tick = Tick(&bar);
            let file = format!("{stem}.ytd");
            let mut sorted = list.clone();
            sorted.sort();
            let fp = sha(&format!("{TEX_V}{}{}|{}|{:?}|{}", sixng_core::bcpack::BCPACK_DECODER_VERSION, sixng_core::bcpack_presets::tag(), texopt::TAG, ctx.o.max_tex, sorted.iter().map(|t| format!("{t}:{}:{}:{}", tsize(t), cap_of(t), alpha_tex.contains(t))).collect::<Vec<_>>().join(",")));
            if ctx.up_to_date(&file, &fp).is_some() {
                return ctx.emit(&file, &fp, json!({"textures": sorted.len()}), None);
            }
            let mut texs = vec![placeholder()];
            // per texture in parallel too: a few big dictionaries otherwise leave cores idle
            let loaded: Vec<Result<_, String>> = sorted
                .par_iter()
                .map(|t| {
                    let p = cache.join(format!("{}.dds", &t[4..]));
                    let Ok(b) = pools.load(&p) else {
                        // named by a cached model but not recorded / not
                        // decodable any more (decoder version bump): placeholder
                        ctx.skip(t, "texture not in the texture records (placeholder used)", json!({"dds": p}));
                        let mut ph = placeholder();
                        ph.name = t.clone();
                        return Ok(ph);
                    };
                    let mut tex = ytd::from_dds(t, &b)?;
                    tex.limit_size(cap_of(t));
                    Ok(tex)
                })
                .collect();
            for t in loaded {
                texs.push(t?);
            }
            let bytes = ytd::build_ytd(&texs).map_err(|e| format!("{file}: {e}"))?;
            ctx.emit(&file, &fp, json!({"textures": sorted.len()}), Some(bytes))
        })
        .collect();
    pools.store.save()?;
    for r in results {
        r?;
    }
    drop(p);
    let _p = super::Phase::new(ctx, "  meta (ytyp/ymap)");
    // gtxd.meta (CMapParentTxds), referenced from fxmanifest.lua
    let gp = ctx.o.out.join("gtxd.meta");
    let rel: Vec<(&String, &String)> = dicts.iter().filter_map(|(c, _, p)| p.as_ref().map(|p| (p, c))).collect();
    if rel.is_empty() {
        let _ = fs::remove_file(&gp);
    } else {
        let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<CMapParentTxds>\n <txdRelationships>\n");
        for (p, c) in rel {
            x += &format!("  <Item>\n   <parent>{p}</parent>\n   <child>{c}</child>\n  </Item>\n");
        }
        x += " </txdRelationships>\n</CMapParentTxds>\n";
        fs::write(&gp, x)?;
    }


    // ytyp
    let names: Vec<&String> = done.keys().collect();
    let alod = archetype_lods(ctx, &names);
    // Size floor: 6m's lodDists assume its own LOD chain; in GTA a big
    // building must not stop drawing before the small props on it, so
    // lodDist >= 10x bounding radius (150..1500 m), >= 300 m when large.
    let radius = |a: &Arch| (0..3).map(|k| (a.hi[k] - a.lo[k]).powi(2)).sum::<f32>().sqrt() / 2.0;
    let floor = |a: &Arch| {
        let r = radius(a);
        let f = (r.max(1.0) * 10.0).clamp(150.0, 1500.0);
        if r >= 15.0 { f.max(300.0) } else { f }
    };
    // Clutter (bounding radius < 3 m: cones, crates, tools, signs) streams
    // only near the player: lodDist 40..100 m whatever 6m says.
    let small = |a: &Arch| radius(a) < 3.0;
    let clamp_small = |a: &Arch, d: f32| if small(a) { d.clamp(40.0, 100.0) } else { d };
    // 6m's own lodDist (archetype def `82d6fc83` +0x38, via archetype_lod.jsonl)
    // wherever it has one: the old 10x-radius floor and clutter clamp were
    // stand-ins for the missing LOD chain (batch parents were dropped, see
    // lod_parents), so they now only apply when 6m gives no value.
    let lod = |n: &str, a: &Arch| match alod.get(n).copied().filter(|d| *d > 0.0) {
        Some(d) => d,
        None if small(a) => clamp_small(a, radius(a) * 30.0),
        None => floor(a),
    };
    let archetypes: Vec<Archetype> = done
        .iter()
        .map(|(n, a)| {
            let r = ((0..3).map(|k| (a.hi[k] - a.lo[k]).powi(2)).sum::<f32>().sqrt() / 2.0).max(1.0);
            let src = if alod.get(n).is_some_and(|d| *d > 0.0) { "6m" } else if small(a) { "fallback (clutter 30x radius)" } else { "fallback (10x radius floor)" };
            super::regionlog::bump("lod_dist_source", src, &a.an);
            serde_json::from_value(json!({"name": a.an, "lod_dist": lod(n, a), "bb_min": a.lo, "bb_max": a.hi, "bs_radius": r, "hd_texture_dist": 100.0,
                "texture_dictionary": txd_of[&a.an]}))
            .unwrap()
        })
        .collect();
    let yt = meta::build_ytyp(&MapTypes { name: "t6m_bld".into(), archetypes: archetypes.clone() })?;
    emit_meta(ctx, "t6m_bld.ytyp", yt)?;
    interiors::emit(ctx, &mlos, &done.iter().map(|(n, a)| (n.clone(), a.an.clone())).collect(), &archetypes)?;
    lodlights::emit(ctx, &dir, index)?;
    {
        // for --split (split.rs): which txd each archetype uses
        let mut l = ctx.layout.lock().unwrap();
        *l = Default::default();
        l.txd = txd_of.iter().map(|(a, t)| (a.clone(), t.clone())).collect();
    }

    // LOD chains: level 0 in 512 m cells, each LOD level in one ymap
    // (t6m_lod1 = LOD, t6m_lod2 = SLOD1, ...), each the parent of the one below.
    let mut kept_own: Vec<Place> = places.iter().filter(|p| done.contains_key(&p.name)).cloned().collect();
    let maps: HashSet<&str> = kept_own.iter().filter_map(|p| p.key.as_ref().map(|k| k.0.as_str())).collect();
    let (links, blinks) = lod_parents(ctx, &maps);
    // 6m parents ~94% of HD props to an instance of a prop batch (lod
    // parent kind "batch", e.g. *_batchlod); GTA V cannot parent to a batch
    // instance, so the merged batch entity becomes the LOD parent: nearest
    // batch with that map + model. Dropping these left them ORPHANHD.
    let mut bcand: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, p) in kept_own.iter().enumerate() {
        if let Some((k, _)) = p.key.as_ref().filter(|k| k.0.starts_with("batch:")) {
            bcand.entry(k.clone()).or_default().push(i);
        }
    }
    let d2 = |a: V3, b: V3| (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f32>();
    let bparent: Vec<Option<usize>> = kept_own
        .iter()
        .map(|p| {
            if ctx.o.no_lod || p.level != 0 {
                return None;
            }
            let c = bcand.get(blinks.get(p.key.as_ref()?)?)?;
            c.iter().copied().min_by(|&a, &b| d2(kept_own[a].pos, p.pos).total_cmp(&d2(kept_own[b].pos, p.pos)))
        })
        .collect();
    for q in bparent.iter().flatten() {
        kept_own[*q].level = 1;
    }
    drop(bcand);
    let kept: Vec<&Place> = kept_own.iter().collect();
    let by_key: HashMap<&(String, usize), usize> = kept.iter().enumerate().filter_map(|(i, p)| p.key.as_ref().map(|k| (k, i))).collect();
    let mut parent: Vec<Option<usize>> = kept
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let q = bparent[i].or_else(|| by_key.get(links.get(p.key.as_ref()?)?).copied())?;
            (!ctx.o.no_lod && kept[q].level == p.level + 1).then_some(q)
        })
        .collect();
    // drop LOD-level entities whose children are all gone (they would draw over HD), bottom up
    let top = kept.iter().map(|p| p.level).max().unwrap_or(0);
    // --no-lod: HD only, every entity ORPHANHD
    let mut alive: Vec<bool> = kept.iter().map(|p| !ctx.o.no_lod || p.level == 0).collect();
    for lvl in 1..=top {
        let mut has = vec![false; kept.len()];
        for (i, pa) in parent.iter().enumerate() {
            if let (true, Some(q)) = (alive[i], pa) {
                has[*q] = true;
            }
        }
        for i in 0..kept.len() {
            if kept[i].level == lvl && !has[i] {
                alive[i] = false;
            }
        }
    }
    let mut dropped = 0;
    for i in 0..kept.len() {
        if !alive[i] {
            dropped += 1;
        }
        if parent[i].is_some_and(|q| !alive[q]) {
            parent[i] = None;
        }
    }
    if dropped > 0 {
        ctx.skip(&format!("({dropped} placements)"), "LOD/SLOD entity without converted children", json!({"count": dropped}));
    }
    let elod = |i: usize| {
        let p = kept[i];
        let a = &done[&p.name];
        // entity lodDist (`bf74dfa7`; -1 = use the archetype's)
        if p.elod > 0.0 {
            p.elod
        } else {
            lod(&p.name, a)
        }
    };
    let mut kids = vec![0u32; kept.len()];
    let mut kid_lod = vec![0f32; kept.len()];
    for (i, pa) in parent.iter().enumerate() {
        if let (true, Some(q)) = (alive[i], *pa) {
            kids[q] += 1;
            kid_lod[q] = kid_lod[q].max(elod(i));
        }
    }
    let ent = |i: usize, guid: usize, pidx: i32| -> Entity {
        let p = kept[i];
        let a = &done[&p.name];
        // A parent must cover its children: childLodDist >= every child's
        // lodDist and its own lodDist beyond that, or the HD never streams.
        let child = if kids[i] > 0 { p.child_lod.max(kid_lod[i]) } else { 0.0 };
        let d = elod(i).max(child * 1.25);
        let level = if p.level == 0 && parent[i].is_none() { "LODTYPES_DEPTH_ORPHANHD" } else { LEVEL_NAMES[(p.level as usize).min(5)] };
        serde_json::from_value(json!({"archetype_name": a.an, "flags": if p.level > 0 { 1572864 + 32 } else { 32 }, "guid": guid, "position": p.pos, "rotation": p.rot,
            "scale_xy": p.sxy, "scale_z": p.sz, "lod_dist": d, "parent_index": pidx, "lod_level": level, "num_children": kids[i], "child_lod_dist": child}))
        .unwrap()
    };
    // index of each entity within its level's ymap, top level first
    let mut slot = vec![0usize; kept.len()];
    let lvl_name = |l: u8| format!("t6m_lod{l}");
    let mut loads: Vec<(String, usize, [[f32; 3]; 2])> = Vec::new();
    for lvl in (1..=top).rev() {
        let ids: Vec<usize> = (0..kept.len()).filter(|&i| alive[i] && kept[i].level == lvl).collect();
        if ids.is_empty() {
            let _ = fs::remove_file(ctx.o.out.join(format!("{}.ymap", lvl_name(lvl))));
            continue;
        }
        for (k, &i) in ids.iter().enumerate() {
            slot[i] = k;
        }
        let has_parent = ids.iter().any(|&i| parent[i].is_some());
        let entities: Vec<Entity> = ids.iter().enumerate().map(|(k, &i)| ent(i, k + 1, parent[i].map_or(-1, |q| slot[q] as i32))).collect();
        let name = lvl_name(lvl);
        ctx.layout.lock().unwrap().lod_ymaps.insert(name.clone(), ids.iter().map(|&i| done[&kept[i].name].an.clone()).collect());
        let pname = if has_parent { lvl_name(lvl + 1) } else { String::new() };
        let map = MapData { entities, ..serde_json::from_value(json!({"name": name, "parent": pname, "exported_by": "6ixpack fivem"})).unwrap() };
        let map = with_content_flags(map);
        if let Ok((st, _)) = meta::compute_extents(&map.entities, &archetypes) {
            loads.push((name.clone(), map.entities.len(), st));
        }
        emit_meta(ctx, &format!("{name}.ymap"), meta::build_ymap_with(&map, &archetypes)?)?;
    }
    // HD cells: entities with lodDist <= NEAR_DIST (clutter, small props:
    // ~90% of them) go in t6m_bn_XXXX_YYYY near ymaps whose streaming extents
    // reach only NEAR_DIST past the cell, so only the cells around the player
    // load; the rest in 512 m t6m_bld_XX_YY ymaps. One mixed ymap per 512 m
    // cell made every cell's extents span kilometres (a few 1500 m buildings),
    // so the whole airport (~68k entities) loaded at once: "Building Pool Full".
    const NEAR_DIST: f32 = 150.0;
    // Near cells are sized by entity count: a 512 m cell splits into
    // quadrants while it holds over NEAR_MAX entities, down to NEAR_CELL, so
    // sparse areas get one ymap instead of many near-empty 128 m ones and hot
    // spots get small ones. Keys are the cell origin in NEAR_CELL units.
    const NEAR_MAX: usize = 600;
    let mut cells: BTreeMap<(bool, i64, i64), Vec<usize>> = BTreeMap::new();
    let mut near_roots: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for i in 0..kept.len() {
        if alive[i] && kept[i].level == 0 {
            let p = kept[i];
            let c = ((p.pos[0] / 512.0).floor() as i64, (p.pos[1] / 512.0).floor() as i64);
            if ent(i, 0, -1).lod_dist <= NEAR_DIST {
                near_roots.entry(c).or_default().push(i);
            } else {
                cells.entry((false, c.0, c.1)).or_default().push(i);
            }
        }
    }
    let unit = |v: f32| (v / NEAR_CELL).floor() as i64;
    let per = (512.0 / NEAR_CELL) as i64;
    let mut stack: Vec<(i64, i64, i64, Vec<usize>)> = near_roots.into_iter().map(|((x, y), v)| (x * per, y * per, per, v)).collect();
    while let Some((x, y, s, v)) = stack.pop() {
        if v.len() <= NEAR_MAX || s == 1 {
            cells.insert((true, x, y), v);
            continue;
        }
        let h = s / 2;
        let mut q: [Vec<usize>; 4] = Default::default();
        for i in v {
            let p = kept[i];
            q[((unit(p.pos[0]) - x >= h) as usize) | (((unit(p.pos[1]) - y >= h) as usize) << 1)].push(i);
        }
        for (k, v) in q.into_iter().enumerate().filter(|(_, v)| !v.is_empty()) {
            stack.push((x + h * (k as i64 & 1), y + h * (k as i64 >> 1), h, v));
        }
    }
    // Wide ground-anchored models (asphalt tiles, aprons, pads, buildings):
    // footprint and underside z,
    // so the coarse terrain can be sunk below them (terrain::sink).
    {
        let mut ground: Vec<[f32; 5]> = Vec::new();
        for (i, p) in kept.iter().enumerate() {
            let Some(a) = done.get(&p.name) else { continue };
            if !alive[i] || p.level != 0 {
                continue;
            }
            let s = p.sxy.max(0.01);
            let (dx, dy, dz) = ((a.hi[0] - a.lo[0]) * s, (a.hi[1] - a.lo[1]) * s, (a.hi[2] - a.lo[2]) * p.sz);
            // ground-anchored (model starts at its origin's height) and wide;
            // tall ones are fine too: terrain under a building is hidden anyway
            let _ = dz;
            if dx.min(dy) >= 16.0 && (-1.5..=1.0).contains(&(a.lo[2] * p.sz)) {
                // axis-aligned box of the rotated footprint
                let r = (dx * dx + dy * dy).sqrt() / 2.0;
                let c = [p.pos[0] + (a.lo[0] + a.hi[0]) / 2.0 * s, p.pos[1] + (a.lo[1] + a.hi[1]) / 2.0 * s];
                let flat = p.rot[0].abs() < 0.01 && p.rot[1].abs() < 0.01 && (p.rot[2].abs() < 0.01 || p.rot[3].abs() < 0.01);
                let (hx, hy) = if flat { (dx / 2.0, dy / 2.0) } else { (r, r) };
                ground.push([c[0] - hx, c[1] - hy, c[0] + hx, c[1] + hy, p.pos[2] + a.lo[2] * p.sz]);
            }
        }
        let gp = ctx.o.work.join("fivem_ground.json");
        sixng_core::vfs::write(&gp, serde_json::to_string(&ground)?)?;
        eprintln!("buildings: {} flat ground tiles recorded for terrain", ground.len());
    }
    let links_n = (0..kept.len()).filter(|&i| alive[i] && parent[i].is_some()).count();
    let bplaced: HashSet<&str> = places.iter().filter_map(|q| q.key.as_ref().map(|k| k.0.as_str())).filter(|m| m.starts_with("batch:")).collect();
    let bkeys: HashSet<&str> = kept.iter().filter_map(|q| q.key.as_ref().map(|k| k.0.as_str())).filter(|m| m.starts_with("batch:")).collect();
    for i in (0..kept.len()).filter(|&i| alive[i] && kept[i].level == 0 && parent[i].is_none()) {
        // why the 6m link did not survive (lod_parents.jsonl, per entity)
        let k = kept[i].key.as_ref();
        let why = if ctx.o.no_lod {
            "--no-lod"
        } else if let Some(b) = k.and_then(|k| blinks.get(k)) {
            if bkeys.contains(b.as_str()) {
                "batch parent dropped"
            } else if bplaced.contains(b.as_str()) {
                "batch parent model not built (see skips)"
            } else {
                "batch parent not exported (see batch_skip)"
            }
        } else if let Some(t) = k.and_then(|k| links.get(k)) {
            match by_key.get(t) {
                None => "entity parent not converted",
                Some(&q) if kept[q].level != 1 => "entity parent level not LOD",
                Some(_) => "entity parent dropped",
            }
        } else if k.is_none() {
            "no source key"
        } else if kept[i].src_parent {
            "6m parent not resolved by lod-links"
        } else {
            "6m has no LOD parent"
        };
        super::regionlog::bump("orphan_hd", why, &done[&kept[i].name].an);
    }
    eprintln!("buildings: {} entities, {links_n} LOD links, {top} LOD levels", alive.iter().filter(|x| **x).count());
    for ((near, cx, cy), ids) in cells {
        let name = if near { format!("t6m_bn_{:04}_{:04}", cx + NEAR_OFF, cy + NEAR_OFF) } else { format!("t6m_bld_{:02}_{:02}", cx + 50, cy + 50) };
        ctx.layout.lock().unwrap().ymaps.insert(name.clone(), ids.iter().map(|&i| done[&kept[i].name].an.clone()).collect());
        let has_parent = ids.iter().any(|&i| parent[i].is_some());
        let entities: Vec<Entity> = ids.iter().enumerate().map(|(j, &i)| ent(i, j + 1, parent[i].map_or(-1, |q| slot[q] as i32))).collect();
        let pname = if has_parent { lvl_name(1) } else { String::new() };
        let map = MapData { entities, ..serde_json::from_value(json!({"name": name, "parent": pname, "exported_by": "6ixpack fivem"})).unwrap() };
        let map = with_content_flags(map);
        if let Ok((st, _)) = meta::compute_extents(&map.entities, &archetypes) {
            loads.push((name.clone(), map.entities.len(), st));
        }
        let ym = meta::build_ymap_with(&map, &archetypes)?;
        emit_meta(ctx, &format!("{name}.ymap"), ym)?;
    }
    entity_load(ctx, &loads);
    // HD cell ymaps from an earlier layout would place their entities twice
    let keep: HashSet<String> = loads.iter().map(|l| format!("{}.ymap", l.0)).collect(); // (LOD names never match the HD prefixes)
    for e in fs::read_dir(&ctx.stream)?.flatten() {
        let f = e.file_name().to_string_lossy().into_owned();
        if (f.starts_with("t6m_bld_") || f.starts_with("t6m_bn_")) && f.ends_with(".ymap") && !keep.contains(&f) {
            let _ = fs::remove_file(e.path());
        }
    }
    Ok(())
}

/// CMapData flags / contentFlags from the entities' lod levels, as in the
/// base game's ymaps (CodeWalker CalcFlags): HD/ORPHANHD -> content bit 1,
/// LOD -> 2, SLOD1 -> 16, SLOD2-4 -> 4; any LOD level sets flags bit 2.
/// Vanilla: HD maps 0/1(+64), LOD+SLOD1 maps 2/18, SLOD maps 2/20. Ours were
/// 0/0 for HD cells and 2/2 for every LOD/SLOD level.
fn with_content_flags(mut map: MapData) -> MapData {
    let (mut f, mut c) = (0u32, 0u32);
    for e in &map.entities {
        match meta::enum_val(&e.lod_level, meta::LOD_TYPES).unwrap_or(0) {
            0 | 5 => c |= 1,
            1 => (c, f) = (c | 2, f | 2),
            2 => (c, f) = (c | 16, f | 2),
            _ => (c, f) = (c | 4, f | 2),
        }
    }
    map.flags = f;
    map.content_flags = c;
    map
}

/// Smallest HD near cell (metres) and the unit of t6m_bn_XXXX_YYYY names
/// (cell origin in NEAR_CELL units + NEAR_OFF); split.rs and minimap.rs map
/// them back to 512 m cells. A near cell never crosses a 512 m cell.
pub const NEAR_CELL: f32 = 64.0;
pub const NEAR_OFF: i64 = 800;

/// Report entities per ymap and the most loaded at once: at each 128 m grid
/// point, the sum over ymaps (HD cells and LOD/SLOD levels) whose streaming
/// extents contain it. Warns above 40k (GTA V's building pool
/// is 65k including the base game's own entities).
fn entity_load(ctx: &Ctx, loads: &[(String, usize, [[f32; 3]; 2])]) {
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for (_, _, e) in loads {
        for k in 0..2 {
            lo[k] = lo[k].min(e[0][k]);
            hi[k] = hi[k].max(e[1][k]);
        }
    }
    let (mut worst, mut at) = (0usize, [0.0f32; 2]);
    let mut x = lo[0];
    while x <= hi[0] {
        let mut y = lo[1];
        while y <= hi[1] {
            let n: usize = loads.iter().filter(|(_, _, e)| x >= e[0][0] && x <= e[1][0] && y >= e[0][1] && y <= e[1][1]).map(|l| l.1).sum();
            if n > worst {
                (worst, at) = (n, [x, y]);
            }
            y += 128.0;
        }
        x += 128.0;
    }
    let total: usize = loads.iter().map(|l| l.1).sum();
    let biggest = loads.iter().max_by_key(|l| l.1);
    eprintln!("buildings: {total} entities in {} ymaps; at most {worst} streamed at once (near {:.0},{:.0}); biggest ymap {}", loads.len(), at[0], at[1], biggest.map_or(String::new(), |b| format!("{} ({})", b.0, b.1)));
    if worst > 40_000 {
        eprintln!("WARNING: {worst} entities streamed at once near {:.0},{:.0}: over 40k risks \"Building Pool Full\" (65k incl. the base game)", at[0], at[1]);
        ctx.report.lock().unwrap().warnings.push(json!({"warning": "entities streamed at once over 40k", "count": worst, "at": at}));
    }
    ctx.report.lock().unwrap().outputs.push(json!({"file": "(entity load)", "status": "stats", "max_at_once": worst, "at": at, "total": total,
        "per_ymap": loads.iter().map(|l| (l.0.clone(), json!(l.1))).collect::<serde_json::Map<_, _>>()}));
}

/// Meta files are cheap to rebuild: rewrite only when the bytes differ.
pub fn emit_meta(ctx: &Ctx, file: &str, b: Vec<u8>) -> Result<String, String> {
    let same = fs::read(ctx.stream.join(file)).is_ok_and(|x| x == b);
    ctx.emit(file, "meta", Value::Null, (!same).then_some(b))
}
