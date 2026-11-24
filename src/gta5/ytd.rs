//! GTA V (legacy PC) .ytd texture dictionary writer.
//! Port of CodeWalker TextureDictionary / Texture / TextureData (non-Gen9)
//! and the DDS reading parts of DDSIO.

use super::rsc::{joaat, BlockId, Builder, Seg, W};

/// Texture format as the game stores it (D3DFMT value / FourCC).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum TexFormat {
    A8R8G8B8 = 21,
    X8R8G8B8 = 22,
    A1R5G5B5 = 25,
    A8 = 28,
    A8B8G8R8 = 32,
    L8 = 50,
    DXT1 = 0x3154_5844,
    DXT3 = 0x3354_5844,
    DXT5 = 0x3554_5844,
    ATI1 = 0x3149_5441,
    ATI2 = 0x3249_5441,
    BC7 = 0x2037_4342,
}

impl TexFormat {
    /// (block bytes, is block-compressed) or bits per pixel for uncompressed.
    fn block_bytes(self) -> Option<usize> {
        match self {
            TexFormat::DXT1 | TexFormat::ATI1 => Some(8),
            TexFormat::DXT3 | TexFormat::DXT5 | TexFormat::ATI2 | TexFormat::BC7 => Some(16),
            _ => None,
        }
    }
    fn bpp(self) -> usize {
        match self {
            TexFormat::A8 | TexFormat::L8 => 8,
            TexFormat::A1R5G5B5 => 16,
            _ => 32,
        }
    }
    /// DirectXTex ComputePitch: (row pitch, slice pitch).
    pub fn pitch(self, w: usize, h: usize) -> (usize, usize) {
        match self.block_bytes() {
            Some(bb) => {
                let r = ((w + 3) / 4).max(1) * bb;
                (r, r * ((h + 3) / 4).max(1))
            }
            None => {
                let r = (w * self.bpp() + 7) / 8;
                (r, r * h)
            }
        }
    }
}

pub struct Tex {
    pub name: String,
    pub width: u16,
    pub height: u16,
    pub levels: u8,
    pub format: TexFormat,
    /// All mips, as in the DDS payload.
    pub data: Vec<u8>,
    /// TextureBase.Unknown_32h ("Unk32" in CodeWalker XML).
    pub unk32: u16,
    /// UsageData (usage & 0x1F | flags << 5).
    pub usage_data: u32,
    pub extra_flags: u32,
}

impl Tex {
    /// CodeWalker DDSIO stride: slicePitch / height.
    pub fn stride(&self) -> u16 {
        let (_, s) = self.format.pitch(self.width as usize, self.height as usize);
        (s / (self.height.max(1) as usize)) as u16
    }
    /// Byte size of the full mip chain.
    pub fn mip_chain_len(&self) -> usize {
        (0..self.levels as usize)
            .map(|i| self.format.pitch((self.width as usize >> i).max(1), (self.height as usize >> i).max(1)).1)
            .sum()
    }
    /// Drop top mips until the largest side is at most `max` (keeps at least
    /// one mip). Returns true if anything was dropped.
    pub fn limit_size(&mut self, max: u16) -> bool {
        let mut dropped = false;
        while self.levels > 1 && self.width.max(self.height) > max {
            let top = self.format.pitch(self.width as usize, self.height as usize).1;
            if top > self.data.len() {
                break;
            }
            self.data.drain(..top);
            self.width = (self.width / 2).max(1);
            self.height = (self.height / 2).max(1);
            self.levels -= 1;
            dropped = true;
        }
        dropped
    }
    /// Length the game/CodeWalker reader takes: stride*height, /4 per mip.
    fn reader_len(&self) -> usize {
        let mut l = self.stride() as usize * self.height as usize;
        let mut t = 0;
        for _ in 0..self.levels {
            t += l;
            l /= 4;
        }
        t
    }
}

fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn dxgi_format(code: u32) -> Option<TexFormat> {
    use TexFormat::*;
    Some(match code {
        70 | 71 | 72 => DXT1,
        73 | 74 | 75 => DXT3,
        76 | 77 | 78 => DXT5,
        79 | 80 => ATI1,
        82 | 83 => ATI2,
        97 | 98 | 99 => BC7,
        27 | 28 | 29 => A8B8G8R8,
        87 | 90 | 91 => A8R8G8B8,
        88 | 92 | 93 => X8R8G8B8,
        86 => A1R5G5B5,
        65 => A8,
        60 | 61 => L8,
        _ => return None,
    })
}

fn legacy_format(b: &[u8]) -> Result<TexFormat, String> {
    use TexFormat::*;
    let flags = rd32(b, 80);
    let fourcc = rd32(b, 84);
    let bits = rd32(b, 88);
    let m = [rd32(b, 92), rd32(b, 96), rd32(b, 100), rd32(b, 104)];
    if flags & 0x4 != 0 {
        return Ok(match &fourcc.to_le_bytes() {
            b"DXT1" => DXT1,
            b"DXT2" | b"DXT3" => DXT3,
            b"DXT4" | b"DXT5" => DXT5,
            b"ATI1" | b"BC4U" => ATI1,
            b"ATI2" | b"BC5U" => ATI2,
            b"BC7 " | b"BC7\0" => BC7,
            _ => match fourcc {
                21 => A8R8G8B8,
                22 => X8R8G8B8,
                25 => A1R5G5B5,
                28 => A8,
                32 => A8B8G8R8,
                50 => L8,
                _ => return Err(format!("unsupported DDS fourcc 0x{fourcc:08X}")),
            },
        });
    }
    let fmt = if flags & 0x40 != 0 {
        match (bits, m) {
            (32, [0xff, 0xff00, 0xff0000, 0xff000000]) => Some(A8B8G8R8),
            (32, [0xff0000, 0xff00, 0xff, 0xff000000]) => Some(A8R8G8B8),
            (32, [0xff0000, 0xff00, 0xff, 0]) => Some(X8R8G8B8),
            (16, [0x7c00, 0x3e0, 0x1f, 0x8000]) => Some(A1R5G5B5),
            _ => None,
        }
    } else if flags & 0x20000 != 0 && bits == 8 {
        Some(L8)
    } else if flags & 0x2 != 0 && bits == 8 {
        Some(A8)
    } else {
        None
    };
    fmt.ok_or_else(|| format!("unsupported DDS pixel format (flags 0x{flags:X}, {bits} bpp)"))
}

/// Parse a DDS file into a texture. DX10 headers (incl. sRGB/typeless DXGI
/// codes) map to the matching game format. A payload shorter than the full
/// mip chain is zero-padded.
pub fn from_dds(name: &str, bytes: &[u8]) -> Result<Tex, String> {
    if bytes.len() < 128 || &bytes[0..4] != b"DDS " {
        return Err(format!("{name}: not a DDS file"));
    }
    let height = rd32(bytes, 12);
    let width = rd32(bytes, 16);
    let mips = rd32(bytes, 28).max(1);
    let caps2 = rd32(bytes, 112);
    let mut off = 128;
    let format = if rd32(bytes, 84) == u32::from_le_bytes(*b"DX10") && rd32(bytes, 80) & 4 != 0 {
        if bytes.len() < 148 {
            return Err(format!("{name}: truncated DX10 header"));
        }
        off = 148;
        let code = rd32(bytes, 128);
        let dim = rd32(bytes, 132);
        let misc = rd32(bytes, 136);
        let arr = rd32(bytes, 140);
        if dim != 3 || misc & 4 != 0 || arr > 1 {
            return Err(format!("{name}: only plain 2D textures are supported"));
        }
        dxgi_format(code).ok_or_else(|| format!("{name}: unsupported DXGI format {code}"))?
    } else {
        if caps2 & 0x200 != 0 || caps2 & 0x200000 != 0 {
            return Err(format!("{name}: cubemaps/volumes are not supported"));
        }
        legacy_format(bytes).map_err(|e| format!("{name}: {e}"))?
    };
    if width == 0 || height == 0 || width > 0xFFFF || height > 0xFFFF || mips > 0xFF {
        return Err(format!("{name}: bad dimensions {width}x{height} mips {mips}"));
    }
    let mut t = Tex {
        name: name.to_string(),
        width: width as u16,
        height: height as u16,
        levels: mips as u8,
        format,
        data: bytes[off..].to_vec(),
        unk32: 0,
        usage_data: 0,
        extra_flags: 0,
    };
    let need = t.mip_chain_len().max(t.reader_len());
    if t.data.len() < need {
        t.data.resize(need, 0);
    }
    Ok(t)
}

fn list_header(w: &mut W, target: Option<BlockId>, n: usize) {
    w.ptr(target).u16(n as u16).u16(n as u16).u32(0);
}

fn write_dict(b: &mut Builder, id: BlockId, texs: &[Tex], pages: Option<BlockId>) -> Result<(), String> {
    let mut order: Vec<(u32, &Tex)> = texs.iter().map(|t| (joaat(&t.name.to_lowercase()), t)).collect();
    order.sort_by_key(|e| e.0);
    for p in order.windows(2) {
        if p[0].0 == p[1].0 {
            return Err(format!("duplicate texture name hash: {} / {}", p[0].1.name, p[1].1.name));
        }
    }
    let mut hashes = W::new();
    let mut ptrs = W::new();
    for &(h, t) in &order {
        hashes.u32(h);
        let tid = b.reserve(Seg::Sys);
        let name = b.string(&t.name);
        let data = b.add(Seg::Gfx, W { b: t.data.clone(), ptrs: Vec::new() });
        let mut w = W::new();
        w.u32(0).u32(1).zeros(32).ptr(Some(name));
        w.u16(1).u16(t.unk32).u32(0).u32(0).u32(0);
        w.u32(t.usage_data).u32(0).u32(t.extra_flags).u32(0);
        w.u16(t.width).u16(t.height).u16(1).u16(t.stride());
        w.u32(t.format as u32).u8(0).u8(t.levels).u16(0);
        w.zeros(16).ptr(Some(data)).zeros(24);
        debug_assert_eq!(w.len(), 144);
        b.set(tid, w);
        ptrs.ptr(Some(tid));
    }
    let n = order.len();
    let (hb, pb) = if n > 0 { (Some(b.add(Seg::Sys, hashes)), Some(b.add(Seg::Sys, ptrs))) } else { (None, None) };
    let mut w = W::new();
    w.u32(0).u32(1).ptr(pages);
    w.u32(0).u32(0).u32(1).u32(0);
    list_header(&mut w, hb, n);
    list_header(&mut w, pb, n);
    b.set(id, w);
    Ok(())
}

/// Add a TextureDictionary (no pages info, as embedded in a drawable's
/// shader group) to an existing builder; returns its block.
pub fn add_dict(b: &mut Builder, texs: &[Tex]) -> Result<BlockId, String> {
    let id = b.reserve(Seg::Sys);
    write_dict(b, id, texs, None)?;
    Ok(id)
}

/// Build a standalone .ytd (resource version 13).
pub fn build_ytd(texs: &[Tex]) -> Result<Vec<u8>, String> {
    let mut b = Builder::new();
    let root = b.reserve(Seg::Sys);
    let pages = b.sys_bytes(vec![0u8; 16 + 8 * 128]);
    b.pages_info = Some(pages);
    write_dict(&mut b, root, texs, Some(pages))?;
    b.build(13)
}

/// Texture name from a file stem: strips a leading "NNNN_" numeric prefix.
pub fn tex_name(stem: &str) -> String {
    if let Some(i) = stem.find('_') {
        if i > 0 && stem[..i].bytes().all(|c| c.is_ascii_digit()) && i + 1 < stem.len() {
            return stem[i + 1..].to_string();
        }
    }
    stem.to_string()
}

pub fn cli(args: &[String]) -> Result<(), String> {
    if args.len() != 2 {
        return Err("usage: ytd <dds-dir> <out.ytd>".into());
    }
    let mut files: Vec<_> = std::fs::read_dir(&args[0])
        .map_err(|e| format!("{}: {e}", args[0]))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().map_or(false, |x| x.eq_ignore_ascii_case("dds")))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("no .dds files in {}", args[0]));
    }
    let mut texs = Vec::new();
    for p in &files {
        let stem = p.file_stem().unwrap().to_string_lossy();
        let bytes = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
        texs.push(from_dds(&tex_name(&stem), &bytes)?);
    }
    let out = build_ytd(&texs)?;
    std::fs::write(&args[1], &out).map_err(|e| format!("{}: {e}", args[1]))?;
    println!("wrote {} ({} textures, {} bytes)", args[1], texs.len(), out.len());
    Ok(())
}
