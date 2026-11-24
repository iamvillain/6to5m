//! Self-check reader for the RSC8 files this crate writes: walks every
//! pointer of a .ytd / .ydr (static drawable subset) / .ytyp (meta) and
//! returns a summary.
//! Any pointer outside its segment, or a size that does not match its
//! table, is an error.

use super::rsc8::Rsc8;
use crate::gta5::rsc::Seg;

#[derive(Debug, Clone)]
pub struct TexInfo {
    pub name: String,
    pub width: u16,
    pub height: u16,
    pub mips: u8,
    pub format: u8,
    pub block_count: u32,
    pub block_stride: u32,
    pub flags: u32,
    /// byte offset of the pixel data in the physical segment
    pub data_off: usize,
}

#[derive(Debug, Clone)]
pub struct DictInfo {
    pub hashes: Vec<u32>,
    pub textures: Vec<TexInfo>,
    pub block_map: bool,
}

#[derive(Debug, Clone)]
pub struct ShaderInfo {
    pub hash: u32,
    pub bucket: u8,
    pub data_size: u16,
    pub buffers: u8,
    pub samplers: Vec<u8>,
    /// texture name per texture parameter (None = null pointer)
    pub textures: Vec<Option<String>>,
    /// render bucket mask byte (shader +0x3C)
    pub bucket_mask: u8,
    /// parameter table: (name hash, packed info)
    pub params: Vec<(u32, u32)>,
    /// cbuffer sizes from the first copy's pointer deltas
    pub buffer_sizes: Vec<usize>,
    /// cbuffer parameters: (hash, buffer, offset, values from the first copy)
    pub cbuffers: Vec<(u32, u8, u16, Vec<f32>)>,
}

#[derive(Debug, Clone)]
pub struct GeomInfo {
    pub lod: usize,
    pub shader: u16,
    pub vertices: u32,
    pub stride: u16,
    pub indices: u32,
    pub format_data: u32,
    /// (slot, offset, size, type) of the used vertex elements
    pub elements: Vec<(usize, u32, u8, u8)>,
    /// offset of the vertex data block (virtual)
    pub vdata: usize,
    pub idata: usize,
}

#[derive(Debug, Clone)]
pub struct YdrInfo {
    pub name: String,
    pub vsize: usize,
    pub psize: usize,
    pub centre: [f32; 3],
    pub radius: f32,
    pub bbox: ([f32; 3], [f32; 3]),
    /// LOD switch distances (High, Med, Low, VLow)
    pub lod_dist: [f32; 4],
    pub mask_flags: [u32; 4],
    pub shaders: Vec<ShaderInfo>,
    pub geoms: Vec<GeomInfo>,
    pub dict: Option<DictInfo>,
}

fn arr(r: &Rsc8, o: usize) -> (u64, u16, u16) {
    (r.u64(o), r.u16(o + 8), r.u16(o + 10))
}

fn v3(r: &Rsc8, o: usize) -> [f32; 3] {
    [r.f32(o), r.f32(o + 4), r.f32(o + 8)]
}

/// One 176-byte texture (or 80-byte reference when `stub`).
fn texture(r: &Rsc8, o: usize, stub: bool) -> Result<TexInfo, String> {
    let name = r.cstr(r.u64(o + 0x28))?;
    let t = TexInfo {
        name,
        width: r.u16(o + 0x18),
        height: r.u16(o + 0x1A),
        mips: r.u8(o + 0x22),
        format: r.u8(o + 0x1F),
        block_count: r.u32(o + 0x08),
        block_stride: r.u32(o + 0x0C),
        flags: r.u32(o + 0x10),
        data_off: 0,
    };
    if stub {
        return Ok(t);
    }
    let srv = r.sys(r.u64(o + 0x30), 64)?;
    if srv != o + 0x68 {
        return Err(format!("{}: SRV at 0x{srv:X}, not embedded at +0x68", t.name));
    }
    let len = t.block_count as usize * t.block_stride as usize;
    let (seg, data_off) = r.resolve(r.u64(o + 0x38), len)?;
    if seg != Seg::Gfx {
        return Err(format!("{}: pixel data not in the physical segment", t.name));
    }
    Ok(TexInfo { data_off, ..t })
}

/// Texture dictionary at virtual offset `o`.
pub fn dict(r: &Rsc8, o: usize) -> Result<DictInfo, String> {
    let (hp, hn, _) = arr(r, o + 0x20);
    let (tp, tn, _) = arr(r, o + 0x30);
    if hn != tn || hn == 0 {
        return Err(format!("dictionary: {hn} hashes, {tn} textures"));
    }
    let ho = r.sys(hp, 4 * hn as usize)?;
    let to = r.sys(tp, 8 * tn as usize)?;
    let hashes: Vec<u32> = (0..hn as usize).map(|i| r.u32(ho + 4 * i)).collect();
    if hashes.windows(2).any(|w| w[0] >= w[1]) {
        return Err("dictionary hashes not strictly ascending".into());
    }
    let mut textures = Vec::new();
    for i in 0..tn as usize {
        let t = texture(r, r.sys(r.u64(to + 8 * i), 0xB0)?, false)?;
        if crate::gta5::rsc::joaat(&t.name) != hashes[i] {
            return Err(format!("texture {} does not match hash 0x{:08X}", t.name, hashes[i]));
        }
        textures.push(t);
    }
    Ok(DictInfo { hashes, textures, block_map: r.u64(o + 8) != 0 })
}

pub fn ytd(bytes: &[u8]) -> Result<DictInfo, String> {
    let r = Rsc8::parse(bytes)?;
    if r.version != 2 {
        return Err(format!("ytd version {} (want 2)", r.version));
    }
    if r.virt.len() < 64 {
        return Err("virtual segment too small".into());
    }
    let d = dict(&r, 0)?;
    block_map(&r, 0)?;
    Ok(d)
}

fn block_map(r: &Rsc8, root: usize) -> Result<(), String> {
    let bm = r.sys(r.u64(root + 8), 16)?;
    let (v, p) = (r.u8(bm + 8), r.u8(bm + 9));
    if v != 1 || p != u8::from(!r.phys.is_empty()) {
        return Err(format!("block map pages {v}/{p}"));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct MetaClass {
    pub name: u32,
    pub key: u32,
    pub size: u32,
    /// (name hash, offset, type, ref index, type name hash)
    pub members: Vec<(u32, i32, u8, u16, u32)>,
}

#[derive(Debug, Clone)]
pub struct ArchInfo {
    pub load_flags: u64,
    pub lod_dist: f32,
    pub flags: u32,
    pub avoidance: u32,
    pub bb_min: [f32; 4],
    pub bb_max: [f32; 4],
    pub bs_centre: [f32; 3],
    pub bs_radius: f32,
    pub hd_texture_dist: f32,
    pub name: u32,
    pub texture_dictionary: u32,
    pub clip_dictionary: u32,
    pub drawable_dictionary: u32,
    pub physics_dictionary: u32,
    pub asset_type: u32,
    pub asset_name: u32,
    pub extensions: u16,
    pub entity_type: u32,
}

#[derive(Debug, Clone)]
pub struct YtypInfo {
    pub vsize: usize,
    pub root_block: u32,
    pub classes: Vec<MetaClass>,
    /// (name hash, key, values (value, name hash))
    pub enums: Vec<(u32, u32, Vec<(i64, u32)>)>,
    /// (type hash, length, virtual offset)
    pub blocks: Vec<(u32, u32, usize)>,
    /// meta pointers followed (array data and StructPtr elements)
    pub meta_ptrs: usize,
    /// CMapTypes `name`
    pub name: u32,
    pub archetypes: Vec<ArchInfo>,
}

/// Meta pointer `(offset << 20) | block id` -> (block index, offset), with
/// `len` bytes in range of that data block.
fn meta(blocks: &[(u32, u32, usize)], p: u64, len: usize) -> Result<(usize, usize), String> {
    let (id, off) = ((p & 0xFFFF) as usize, (p >> 20) as usize);
    if p >> 16 & 0xF != 0 || id == 0 || id > blocks.len() {
        return Err(format!("bad meta pointer 0x{p:X}"));
    }
    if off + len > blocks[id - 1].1 as usize {
        return Err(format!("meta pointer 0x{p:X} (+{len}) past data block {id} ({} bytes)", blocks[id - 1].1));
    }
    Ok((id - 1, off))
}

/// Walk an RSC8 meta .ytyp: root, class / enum tables, data blocks, every
/// array of every struct in a data block of a known class (meta pointers
/// and StructPtr elements in range, element types matching), archetypes.
pub fn ytyp(bytes: &[u8]) -> Result<YtypInfo, String> {
    let r = Rsc8::parse(bytes)?;
    if r.version != 3 {
        return Err(format!("ytyp version {} (want 3)", r.version));
    }
    if !r.phys.is_empty() {
        return Err(format!("ytyp has a {} byte physical segment", r.phys.len()));
    }
    if r.virt.len() < 0x60 {
        return Err("virtual segment too small".into());
    }
    if r.u32(0x10) != 0x5052_4430 || r.u16(0x14) != 0x79 {
        return Err(format!("meta magic 0x{:08X} platform 0x{:X}", r.u32(0x10), r.u16(0x14)));
    }
    block_map(&r, 0)?;
    r.sys(r.u64(0x40), 12)?; // UnkData1
    let (nc, ne, nd) = (r.u16(0x48) as usize, r.u16(0x4A) as usize, r.u16(0x4C) as usize);
    let co = r.sys(r.u64(0x28), 32 * nc)?;
    let mut classes = Vec::new();
    for i in 0..nc {
        let c = co + 32 * i;
        let n = r.u16(c + 30) as usize;
        let eo = r.sys(r.u64(c + 16), 16 * n)?;
        let members: Vec<_> = (0..n).map(|k| eo + 16 * k).map(|e| (r.u32(e), r.u32(e + 4) as i32, r.u8(e + 8), r.u16(e + 10), r.u32(e + 12))).collect();
        let size = r.u32(c + 24);
        for (k, m) in members.iter().enumerate() {
            if m.0 != 0x100 && (m.1 < 0 || m.1 as u32 >= size) {
                return Err(format!("class 0x{:08X} member {k} at {} outside size {size}", r.u32(c), m.1));
            }
            if m.2 == 0x52 && m.3 as usize >= n {
                return Err(format!("class 0x{:08X} member {k}: element ref {} of {n}", r.u32(c), m.3));
            }
        }
        classes.push(MetaClass { name: r.u32(c), key: r.u32(c + 4), size, members });
    }
    let eo = r.sys(r.u64(0x30), 24 * ne)?;
    let mut enums = Vec::new();
    for i in 0..ne {
        let e = eo + 24 * i;
        let n = r.u32(e + 16) as usize;
        let vo = r.sys(r.u64(e + 8), 16 * n)?;
        enums.push((r.u32(e), r.u32(e + 4), (0..n).map(|k| (r.u64(vo + 16 * k) as i64, r.u32(vo + 16 * k + 8))).collect()));
    }
    let lo = r.sys(r.u64(0x38), 16 * nd)?;
    let mut blocks = Vec::new();
    for i in 0..nd {
        let (ty, len) = (r.u32(lo + 16 * i), r.u32(lo + 16 * i + 4));
        blocks.push((ty, len, r.sys(r.u64(lo + 16 * i + 8), len as usize)?));
    }
    let class_of = |h: u32| classes.iter().find(|c| c.name == h);
    // every array of every object in a class-typed data block
    let mut meta_ptrs = 0;
    for &(ty, len, bo) in &blocks {
        let Some(c) = class_of(ty) else { continue };
        if c.size == 0 || len % c.size != 0 {
            return Err(format!("data block 0x{ty:08X}: {len} bytes, not a multiple of {}", c.size));
        }
        for obj in (0..len as usize).step_by(c.size as usize) {
            for m in c.members.iter().filter(|m| m.2 == 0x52) {
                let a = bo + obj + m.1 as usize;
                let (p, n, cap) = (r.u64(a), r.u16(a + 8) as usize, r.u16(a + 10) as usize);
                if p == 0 {
                    if n != 0 {
                        return Err(format!("null array with count {n} at 0x{a:X}"));
                    }
                    continue;
                }
                if cap < n {
                    return Err(format!("array at 0x{a:X}: count {n} > capacity {cap}"));
                }
                let el = c.members[m.3 as usize];
                let esize = match el.2 {
                    0x07 => 8,
                    0x4A | 0x15 => 4,
                    0x05 => class_of(el.4).map(|c| c.size as usize).ok_or(format!("array of unknown struct 0x{:08X}", el.4))?,
                    t => return Err(format!("array element type 0x{t:02X} not handled")),
                };
                let (bi, off) = meta(&blocks, p, n * esize)?;
                meta_ptrs += 1;
                if el.2 == 0x07 {
                    if blocks[bi].0 != 7 {
                        return Err(format!("StructPtr array in data block of type 0x{:08X}", blocks[bi].0));
                    }
                    for k in 0..n {
                        let q = r.u64(blocks[bi].2 + off + 8 * k);
                        let tgt = blocks[meta(&blocks, q, 0)?.0].0;
                        let sz = class_of(tgt).map(|c| c.size as usize).ok_or(format!("StructPtr to unknown type 0x{tgt:08X}"))?;
                        let (_, to) = meta(&blocks, q, sz)?;
                        if to % sz != 0 {
                            return Err(format!("StructPtr 0x{q:X} not on an object boundary"));
                        }
                        meta_ptrs += 1;
                    }
                }
            }
        }
    }
    let root_block = r.u32(0x20);
    let rb = blocks.get((root_block as usize).wrapping_sub(1)).filter(|b| b.0 == 0xD98B_B561).ok_or(format!("RootBlockId {root_block} is not a CMapTypes block"))?;
    let at = rb.2;
    let mut archetypes = Vec::new();
    let (p, n) = (r.u64(at + 24), r.u16(at + 32) as usize);
    if p != 0 {
        let (bi, off) = meta(&blocks, p, 8 * n)?;
        for k in 0..n {
            let (ai, ao) = meta(&blocks, r.u64(blocks[bi].2 + off + 8 * k), 160)?;
            if blocks[ai].0 != 0x82D6_FC83 {
                return Err(format!("archetype {k} in a data block of type 0x{:08X}", blocks[ai].0));
            }
            let o = blocks[ai].2 + ao;
            let v4 = |o: usize| [r.f32(o), r.f32(o + 4), r.f32(o + 8), r.f32(o + 12)];
            archetypes.push(ArchInfo {
                load_flags: r.u64(o + 8),
                lod_dist: r.f32(o + 16),
                flags: r.u32(o + 20),
                avoidance: r.u32(o + 24),
                bb_min: v4(o + 32),
                bb_max: v4(o + 48),
                bs_centre: v3(&r, o + 64),
                bs_radius: r.f32(o + 80),
                hd_texture_dist: r.f32(o + 84),
                name: r.u32(o + 88),
                texture_dictionary: r.u32(o + 92),
                clip_dictionary: r.u32(o + 96),
                drawable_dictionary: r.u32(o + 100),
                physics_dictionary: r.u32(o + 104),
                asset_type: r.u32(o + 108),
                asset_name: r.u32(o + 112),
                extensions: r.u16(o + 128),
                entity_type: r.u32(o + 144),
            });
        }
    }
    Ok(YtypInfo { vsize: r.virt.len(), root_block, classes, enums, blocks, meta_ptrs, name: r.u32(at + 56), archetypes })
}

pub fn ydr(bytes: &[u8]) -> Result<YdrInfo, String> {
    let r = Rsc8::parse(bytes)?;
    if r.version != 162 {
        return Err(format!("ydr version {} (want 162)", r.version));
    }
    if r.virt.len() < 0xD0 {
        return Err("virtual segment too small".into());
    }
    block_map(&r, 0)?;
    let name = r.cstr(r.u64(0xA8))?;
    let sg = r.sys(r.u64(0x10), 64)?;
    let dict = match r.u64(sg + 8) {
        0 => None,
        p => Some(dict(&r, r.sys(p, 64)?)?),
    };
    let (sp, sn, _) = arr(&r, sg + 0x10);
    let so = r.sys(sp, 8 * sn as usize)?;
    let mut shaders = Vec::new();
    for i in 0..sn as usize {
        let s = r.sys(r.u64(so + 8 * i), 64)?;
        let size = r.u16(s + 0x3A) as usize;
        let pb = r.sys(r.u64(s + 8), size)?;
        let pd = r.sys(r.u64(s + 0x20), 8)?;
        let (nb, nt, nu, ns) = (r.u8(pd) as usize, r.u8(pd + 1) as usize, r.u8(pd + 2) as usize, r.u8(pd + 3) as usize);
        let np = r.u32(pd + 4) as usize;
        r.sys(r.u64(s + 0x20), 8 + 8 * np)?;
        let mut sizes = vec![0usize; nb];
        let mut texs = 0;
        for k in 0..np {
            let info = r.u32(pd + 8 + 8 * k + 4);
            match info & 3 {
                0 => texs += 1,
                3 => {
                    let (b, off, len) = ((info >> 2 & 0x3F) as usize, (info >> 8 & 0xFFF) as usize, (info >> 20) as usize);
                    if b >= nb {
                        return Err(format!("shader {i}: cbuffer param on buffer {b} of {nb}"));
                    }
                    sizes[b] = sizes[b].max(off + len);
                }
                _ => {}
            }
        }
        if texs != nt {
            return Err(format!("shader {i}: {texs} texture params, header says {nt}"));
        }
        // cbuffer sizes from the first copy's pointer deltas
        let copy0: Vec<usize> = (0..nb).map(|j| r.sys(r.u64(pb + 8 * j), 0)).collect::<Result<_, _>>()?;
        let total = if nb > 0 { r.sys(r.u64(pb + 8 * nb), 0).map(|c1| c1 - copy0[0]).unwrap_or(0) } else { 0 };
        if nb > 0 && copy0[0] != pb + 32 * nb {
            return Err(format!("shader {i}: buffer data not after its pointers"));
        }
        let tex_off = 32 * nb + 4 * total;
        if tex_off + 32 * nt + 32 * nu + ns != size {
            return Err(format!("shader {i}: block size {size} != {tex_off} + {} + {} + {ns}", 32 * nt, 32 * nu));
        }
        if r.sys(r.u64(s + 0x10), 0)? != pb + tex_off {
            return Err(format!("shader {i}: texture refs pointer off its block"));
        }
        let mut textures = Vec::new();
        for k in 0..nt {
            textures.push(match r.u64(pb + tex_off + 8 * k) {
                0 => None,
                p => Some(texture(&r, r.sys(p, 80)?, true)?.name),
            });
        }
        let samplers = (0..ns).map(|k| r.u8(pb + tex_off + 32 * nt + 32 * nu + k)).collect();
        let buffer_sizes: Vec<usize> = (0..nb).map(|j| if j + 1 < nb { copy0[j + 1] - copy0[j] } else { pb + 32 * nb + total - copy0[j] }).collect();
        if let Some(j) = (0..nb).find(|&j| sizes[j] > buffer_sizes[j]) {
            return Err(format!("shader {i}: cbuffer params reach {} in buffer {j} of {}", sizes[j], buffer_sizes[j]));
        }
        let params: Vec<(u32, u32)> = (0..np).map(|k| (r.u32(pd + 8 + 8 * k), r.u32(pd + 8 + 8 * k + 4))).collect();
        let cbuffers = params
            .iter()
            .filter(|p| p.1 & 3 == 3)
            .map(|&(h, info)| {
                let (b, off, len) = ((info >> 2 & 0x3F) as usize, (info >> 8 & 0xFFF) as usize, (info >> 20) as usize);
                let at = copy0[b] + off;
                (h, b as u8, off as u16, (0..len / 4).map(|k| r.f32(at + 4 * k)).collect())
            })
            .collect();
        shaders.push(ShaderInfo { hash: r.u32(s), bucket: r.u8(s + 0x39), data_size: size as u16, buffers: nb as u8, samplers, textures, bucket_mask: r.u8(s + 0x3C), params, buffer_sizes, cbuffers });
    }
    let mut geoms = Vec::new();
    for lod in 0..4 {
        let lp = r.u64(0x50 + 8 * lod);
        if lp == 0 {
            continue;
        }
        let l = r.sys(lp, 16)?;
        let (mp, mn, _) = arr(&r, l);
        let mo = r.sys(mp, 8 * mn as usize)?;
        for mi in 0..mn as usize {
            let m = r.sys(r.u64(mo + 8 * mi), 64)?;
            let (gp, gn, _) = arr(&r, m + 8);
            let go = r.sys(gp, 8 * gn as usize)?;
            let nbx = if gn > 1 { gn as usize + 1 } else { gn as usize };
            r.sys(r.u64(m + 0x18), 32 * nbx)?;
            let maps = r.sys(r.u64(m + 0x20), 2 * gn as usize)?;
            if r.u16(m + 0x36) != gn {
                return Err(format!("model {mi}: shader mapping count {} != {gn}", r.u16(m + 0x36)));
            }
            for gi in 0..gn as usize {
                let g = r.sys(r.u64(go + 8 * gi), 64)?;
                let shader = r.u16(maps + 2 * gi);
                if shader >= sn {
                    return Err(format!("geometry {gi}: shader {shader} of {sn}"));
                }
                let vb = r.sys(r.u64(g + 8), 64)?;
                let ib = r.sys(r.u64(g + 0x10), 64)?;
                let (nv, stride) = (r.u32(vb + 8), r.u16(vb + 0xC));
                let vdata = r.sys(r.u64(vb + 0x18), nv as usize * stride as usize)?;
                r.sys(r.u64(vb + 0x30), 64)?;
                let f = r.sys(r.u64(vb + 0x38), 316)?;
                let format_data = r.u32(f + 312);
                if format_data != 1 | (stride as u32) << 2 | nv << 10 {
                    return Err(format!("geometry {gi}: vertex format data 0x{format_data:X} != stride {stride} / count {nv}"));
                }
                let mut elements = Vec::new();
                let mut sum = 0usize;
                for slot in 0..52 {
                    let (sz, ty) = (r.u8(f + 208 + slot), r.u8(f + 260 + slot));
                    if sz == 0 {
                        continue;
                    }
                    let off = r.u32(f + 4 * slot);
                    if off as usize != sum * nv as usize {
                        return Err(format!("geometry {gi}: element {slot} at {off}, SOA order wants {}", sum * nv as usize));
                    }
                    sum += sz as usize;
                    elements.push((slot, off, sz, ty));
                }
                if sum != stride as usize {
                    return Err(format!("geometry {gi}: element sizes {sum} != stride {stride}"));
                }
                let ni = r.u32(ib + 8);
                if r.u16(ib + 0xC) != 2 {
                    return Err(format!("geometry {gi}: index size {}", r.u16(ib + 0xC)));
                }
                let idata = r.sys(r.u64(ib + 0x18), 2 * ni as usize)?;
                r.sys(r.u64(ib + 0x30), 64)?;
                if let Some(k) = (0..ni as usize).find(|&k| r.u16(idata + 2 * k) as u32 >= nv) {
                    return Err(format!("geometry {gi}: index {k} out of range"));
                }
                geoms.push(GeomInfo { lod, shader, vertices: nv, stride, indices: ni, format_data, elements, vdata, idata });
            }
        }
    }
    Ok(YdrInfo {
        name,
        vsize: r.virt.len(),
        psize: r.phys.len(),
        centre: v3(&r, 0x20),
        radius: r.f32(0x2C),
        bbox: (v3(&r, 0x30), v3(&r, 0x40)),
        lod_dist: [r.f32(0x70), r.f32(0x74), r.f32(0x78), r.f32(0x7C)],
        mask_flags: [r.u32(0x80), r.u32(0x84), r.u32(0x88), r.u32(0x8C)],
        shaders,
        geoms,
        dict,
    })
}
