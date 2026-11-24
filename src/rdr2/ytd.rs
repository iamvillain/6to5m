//! RDR2 `.ytd` (texture dictionary, RSC8 version 2) writer.
//!
//! Input textures are the GTA V writer's `gta5::ytd::Tex` (DDS mip chain in
//! memory) plus an sRGB flag. Layout worked out from vanilla files and
//! checked against CodeX-written ones: a 64-byte dictionary root
//! (joaat hash array, sorted, and a texture pointer array in the same
//! order), 176-byte textures with an embedded 64-byte SRV at +0x68, DXGI
//! format codes, `BlockCount`/`BlockStride` sizing (every mip edge padded to
//! 8 blocks) and the linear (unswizzled) DDS mip chain in the physical
//! segment, zero-padded to `BlockCount * BlockStride`.

use super::rsc8;
use crate::gta5::{
    rsc::{joaat, BlockId, Builder, Seg, W},
    ytd::{self as gytd, Tex, TexFormat},
};

const VFT_TEXTURE: u64 = 0x1_4091_00B0;
pub(super) const VFT_SRV: u64 = 0x1_4091_0080;
/// Texture flags on every game texture.
const TEX_FLAGS: u32 = 0x1800_8002;
/// TileMode "standard" (linear, unswizzled).
const TILE_STANDARD: u8 = 0x0D;
/// SRV dimension code of a plain 2D texture.
const SRV_2D: u64 = 0x401;

/// A texture plus the colour space its format code gets.
pub struct Texture {
    pub tex: Tex,
    pub srgb: bool,
}

/// Data maps are linear: normal maps (`_n`, `_nm`), packed material maps
/// (`_ma`, `_mb`), flat spec / bump stand-ins. Everything else (albedo) is
/// sRGB, as in vanilla RDR2 props (`_ab` BC1_SRGB, `_nm` / `_ma` BC1).
pub fn guess_srgb(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    !(["_n", "_nm", "_ma", "_mb", "_s"].iter().any(|s| n.ends_with(s)) || n.contains("flatspec") || n.contains("flatbump"))
}

impl Texture {
    /// Parse a DDS. A DX10 header decides the colour space from its DXGI
    /// code; legacy headers use `guess_srgb` on the name.
    pub fn from_dds(name: &str, bytes: &[u8]) -> Result<Texture, String> {
        let tex = gytd::from_dds(name, bytes)?;
        let dx10 = bytes.len() >= 148 && &bytes[84..88] == b"DX10";
        let srgb = if dx10 {
            let code = u32::from_le_bytes(bytes[128..132].try_into().unwrap());
            matches!(code, 29 | 72 | 75 | 78 | 91 | 93 | 99)
        } else {
            guess_srgb(name)
        };
        Ok(Texture { tex, srgb })
    }
}

/// RDR2 format code, bytes per block (or pixel) and pixels per block edge.
pub fn format_of(t: &Texture) -> Result<(u8, u32, u32), String> {
    let s = t.srgb;
    use TexFormat::*;
    Ok(match t.tex.format {
        DXT1 => (if s { 0x48 } else { 0x47 }, 8, 4),
        DXT3 => (if s { 0x4B } else { 0x4A }, 16, 4),
        DXT5 => (if s { 0x4E } else { 0x4D }, 16, 4),
        ATI1 => (0x50, 8, 4),
        ATI2 => (0x53, 16, 4),
        BC7 => (if s { 0x63 } else { 0x62 }, 16, 4),
        // D3DFMT A8B8G8R8 is DXGI R8G8B8A8, A8R8G8B8 is B8G8R8A8
        A8B8G8R8 => (if s { 0x1D } else { 0x1C }, 4, 1),
        A8R8G8B8 => (if s { 0x5B } else { 0x57 }, 4, 1),
        A8 => (0x41, 1, 1),
        L8 => (0x3D, 1, 1),
        f => return Err(format!("{}: format {f:?} has no RDR2 code here", t.tex.name)),
    })
}

/// Padded block total: each mip level's block grid rounded up to 8 blocks
/// per edge (16 for 1-byte formats); non-power-of-two sizes with mips are
/// counted at the next power of two.
pub fn block_count(width: u32, height: u32, mips: u32, stride: u32, block_px: u32) -> u32 {
    let align = if stride == 1 { 16 } else { 8 };
    let (mut w, mut h) = (width.max(1), height.max(1));
    if mips > 1 {
        w = w.next_power_of_two();
        h = h.next_power_of_two();
    }
    let mut total = 0;
    for _ in 0..mips.max(1) {
        let bx = w.div_ceil(block_px).max(1).div_ceil(align) * align;
        let by = h.div_ceil(block_px).max(1).div_ceil(align) * align;
        total += bx * by;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    total
}

/// Block-compressed chains end at 4x4, as in vanilla RDR2 textures; lower
/// mips are dropped. The data is cut to the kept chain.
fn trim_mips(t: &Tex, block_px: u32) -> (u8, Vec<u8>) {
    let levels = kept_mips(t, block_px);
    let len: usize = (0..levels as usize).map(|i| t.format.pitch((t.width as usize >> i).max(1), (t.height as usize >> i).max(1)).1).sum();
    (levels, t.data[..len.min(t.data.len())].to_vec())
}

/// Mip levels kept by `trim_mips`.
fn kept_mips(t: &Tex, block_px: u32) -> u8 {
    let mut levels = t.levels.max(1);
    if block_px == 4 {
        while levels > 1 && ((t.width >> (levels - 1)) < 4 || (t.height >> (levels - 1)) < 4) {
            levels -= 1;
        }
    }
    levels
}

/// Bytes of pixel data `t` takes in a dictionary's physical segment
/// (`BlockCount * BlockStride`, as `write_texture` stores it).
pub fn stored_size(t: &Texture) -> Result<u64, String> {
    let (_, stride, bp) = format_of(t)?;
    let mips = kept_mips(&t.tex, bp);
    Ok(block_count(t.tex.width as u32, t.tex.height as u32, mips as u32, stride, bp) as u64 * stride as u64)
}

/// Write one 176-byte texture into reserved block `id` (pixel data as a
/// new physical block).
fn write_texture(b: &mut Builder, id: BlockId, t: &Texture) -> Result<(), String> {
    let (code, stride, bp) = format_of(t)?;
    let (mips, mut data) = trim_mips(&t.tex, bp);
    let count = block_count(t.tex.width as u32, t.tex.height as u32, mips as u32, stride, bp);
    let size = count as usize * stride as usize;
    if data.len() > size {
        return Err(format!("{}: mip chain {} bytes > {} blocks x {}", t.tex.name, data.len(), count, stride));
    }
    data.resize(size, 0);
    let name = b.string(&t.tex.name.to_ascii_lowercase());
    let pix = b.add(Seg::Gfx, W { b: data, ptrs: Vec::new() });
    let mut w = W::new();
    w.u64(VFT_TEXTURE).u32(count).u32(stride).u32(TEX_FLAGS).u32(0);
    w.u16(t.tex.width).u16(t.tex.height).u16(1).u8(1).u8(code);
    w.u8(TILE_STANDARD).u8(0).u8(mips).u8(0).u8(0).u8(0).u16(1);
    w.ptr(Some(name)).ptr_at(Some(id), 0x68).ptr(Some(pix));
    w.u32(0).u32(0).zeros(32);
    debug_assert_eq!(w.len(), 0x68);
    write_srv(&mut w, SRV_2D);
    w.u64(0);
    debug_assert_eq!(w.len(), 0xB0);
    b.set(id, w);
    Ok(())
}

/// 64-byte shader resource view (2D texture 0x401, buffer 0x104).
pub(super) fn write_srv(w: &mut W, dim: u64) {
    w.u64(VFT_SRV).u64(0).u64(dim).u64(5).zeros(32);
}

/// Texture dictionary into reserved block `id`. `block_map` is the
/// standalone file's BlockMap (None when embedded in a drawable). Returns
/// (lower-case name, texture block) per texture.
pub fn write_dict(b: &mut Builder, id: BlockId, texs: &[Texture], block_map: Option<BlockId>) -> Result<Vec<(String, BlockId)>, String> {
    if texs.is_empty() {
        return Err("texture dictionary needs at least one texture".into());
    }
    let mut order: Vec<(u32, &Texture)> = texs.iter().map(|t| (joaat(&t.tex.name), t)).collect();
    order.sort_by_key(|e| e.0);
    if let Some(p) = order.windows(2).find(|p| p[0].0 == p[1].0) {
        return Err(format!("duplicate texture name hash: {} / {}", p[0].1.tex.name, p[1].1.tex.name));
    }
    let mut hashes = W::new();
    let mut ptrs = W::new();
    let mut out = Vec::new();
    for &(h, t) in &order {
        hashes.u32(h);
        let tid = b.reserve(Seg::Sys);
        write_texture(b, tid, t)?;
        ptrs.ptr(Some(tid));
        out.push((t.tex.name.to_ascii_lowercase(), tid));
    }
    let n = order.len() as u16;
    let hb = b.add(Seg::Sys, hashes);
    let pb = b.add(Seg::Sys, ptrs);
    let mut w = W::new();
    w.u64(0).ptr(block_map).u64(0).u64(1);
    w.ptr(Some(hb)).u16(n).u16(n).u32(0);
    w.ptr(Some(pb)).u16(n).u16(n).u32(0);
    b.set(id, w);
    Ok(out)
}

/// Standalone .ytd (RSC8 version 2).
pub fn build_ytd(texs: &[Texture]) -> Result<Vec<u8>, String> {
    let mut b = Builder::new();
    let root = b.reserve(Seg::Sys);
    let bm = b.sys_bytes(vec![0u8; 48]);
    b.pages_info = Some(bm);
    write_dict(&mut b, root, texs, Some(bm))?;
    rsc8::build(b, rsc8::YTD_VERSION)
}

/// 4x4 BC1 texture of one block.
fn bc1_4x4(name: &str, block: [u8; 8], srgb: bool) -> Texture {
    let tex = Tex { name: name.into(), width: 4, height: 4, levels: 1, format: TexFormat::DXT1, data: block.to_vec(), unk32: 0, usage_data: 0, extra_flags: 0 };
    Texture { tex, srgb }
}

/// `_mb` stand-in for `standard`: the same single BC1 block as vanilla
/// `blank_mb` (colour 0x000F / 0x0010, all indices 2: about RGB 0,0,128).
pub fn blank_mb(name: &str) -> Texture {
    bc1_4x4(name, [0x0F, 0x00, 0x10, 0x00, 0xAA, 0xAA, 0xAA, 0xAA], false)
}

/// Vanilla RDR2 `blank_ma` (p_gre_pla_rd_sgn.ytd): R metalness 0, G
/// roughness 243, B AO 255.
pub const BLANK_MA_RGBA: [u8; 4] = [0, 243, 255, 255];

/// `_ma` stand-in for `standard` without a usable 6m speculartex: the same
/// single BC1 block as vanilla `blank_ma`, colour 0x079F (R 0, G 60/63 =
/// 243, B 31 = 255) on all texels.
pub fn blank_ma(name: &str) -> Texture {
    bc1_4x4(name, [0x9F, 0x07, 0x9F, 0x07, 0, 0, 0, 0], false)
}

/// True when a speculartex B channel is the flat-128 "no AO authored"
/// placeholder (std below 6 and mean within 8 of 128).
pub fn flat_128(b: impl Iterator<Item = u8> + Clone) -> bool {
    let n = b.clone().count().max(1) as f64;
    let mean = b.clone().map(f64::from).sum::<f64>() / n;
    let var = b.map(|v| (f64::from(v) - mean).powi(2)).sum::<f64>() / n;
    var.sqrt() < 6.0 && (mean - 128.0).abs() < 8.0
}

/// RDR2 `_ma` pixels from a 6m speculartex (RGBA8): R metal mask and G
/// roughness copied (same layout), B AO copied unless it is flat 128 (then
/// 255, open), A 255.
pub fn ma_from_spec(rgba: &[u8]) -> Vec<u8> {
    let open = flat_128(rgba.chunks_exact(4).map(|p| p[2]));
    rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], if open { 255 } else { p[2] }, 255]).collect()
}

/// Solid grey placeholder for materials whose texture did not resolve.
pub fn grey(name: &str) -> Texture {
    // 565 0x8410 = (16,32,16)/(31,63,31): mid grey
    bc1_4x4(name, [0x10, 0x84, 0x10, 0x84, 0, 0, 0, 0], true)
}
