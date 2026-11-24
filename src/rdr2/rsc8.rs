//! RDR2 RSC8 resource container: write path for a `gta5::rsc::Builder`
//! block graph, plus a small reader (header, inflate, pointer resolve) used
//! for self-checks and tests.
//!
//! Layout (worked out from vanilla and CodeX-written files, checked on bytes):
//! 16-byte header `RSC8`, `[version][compressor-1][encryption+1][0]`, then
//! two flag words that are the 16-aligned virtual / physical segment sizes
//! with the version's high / low nibble in their low 4 bits. The body is a
//! raw deflate stream of the virtual segment followed by the physical one.
//! Pointers use the same 0x50000000 / 0x60000000 bases as RSC7, with linear
//! offsets (no page indirection). Blocks: the root at virtual offset 0, the
//! others sorted by size (descending), each 16-aligned; no page packing.
//! RSC7 output (`Builder::build`) is untouched by this module.

use crate::gta5::rsc::{Builder, Seg, GFX_BASE, SYS_BASE};
use flate2::{read::DeflateDecoder, write::DeflateEncoder, Compression};
use std::io::{Read, Write};

pub const MAGIC: [u8; 4] = *b"RSC8";
/// Resource versions: drawable and texture dictionary.
pub const YDR_VERSION: u8 = 162;
pub const YTD_VERSION: u8 = 2;

fn align16(v: u64) -> u64 {
    v.div_ceil(16) * 16
}

/// Segment length rule: end of the last block rounded to 16 bytes, or to
/// 64 KB once it is past 32 KB.
fn seg_len(end: u64) -> u64 {
    if end <= 0x8000 { align16(end) } else { end.div_ceil(0x10000) * 0x10000 }
}

/// Lay out, patch and write an RSC8 file. The root must be the first system
/// block. `b.pages_info` names the BlockMap block: its bytes 8/9 get the
/// page counts (virtual 1, physical 1 when the physical segment has data).
pub fn build(b: Builder, version: u8) -> Result<Vec<u8>, String> {
    let n = b.blocks.len();
    let sys: Vec<usize> = (0..n).filter(|&i| b.blocks[i].seg == Seg::Sys).collect();
    let gfx: Vec<usize> = (0..n).filter(|&i| b.blocks[i].seg == Seg::Gfx).collect();
    if sys.first() != Some(&0) {
        return Err("the root block must be the first system block".into());
    }
    if let Some(i) = (0..n).find(|&i| b.blocks[i].data.is_empty()) {
        return Err(format!("block {i} is empty (a pointer to it would alias the next block)"));
    }
    let mut pos = vec![0u64; n];
    // stable sort: equal sizes keep creation order (deterministic output)
    let place = |ids: &[usize], keep_first: bool, pos: &mut [u64]| -> u64 {
        let mut order: Vec<usize> = ids.iter().copied().skip(usize::from(keep_first)).collect();
        order.sort_by(|&a, &c| b.blocks[c].data.len().cmp(&b.blocks[a].data.len()));
        if keep_first {
            order.insert(0, ids[0]);
        }
        let mut cur = 0u64;
        for i in order {
            cur = align16(cur);
            pos[i] = cur;
            cur += b.blocks[i].data.len() as u64;
        }
        if ids.is_empty() { 0 } else { seg_len(cur) }
    };
    let vsize = place(&sys, true, &mut pos);
    let psize = place(&gfx, false, &mut pos);
    if vsize > 0x0FFF_FFF0 || psize > 0x0FFF_FFF0 {
        return Err(format!("segment too large (virtual {vsize}, physical {psize})"));
    }
    let base = |s: Seg| if s == Seg::Sys { SYS_BASE } else { GFX_BASE };
    let mut virt = vec![0u8; vsize as usize];
    let mut phys = vec![0u8; psize as usize];
    let mut blocks = b.blocks;
    if let Some(pi) = b.pages_info {
        let d = &mut blocks[pi.0].data;
        if d.len() >= 10 {
            d[8] = 1;
            d[9] = u8::from(psize > 0);
        }
    }
    for (i, blk) in blocks.iter().enumerate() {
        let buf = if blk.seg == Seg::Sys { &mut virt } else { &mut phys };
        let at = pos[i] as usize;
        buf[at..at + blk.data.len()].copy_from_slice(&blk.data);
        for &(off, t, toff) in &blk.ptrs {
            if off + 8 > blk.data.len() {
                return Err(format!("block {i}: pointer slot {off} past its end"));
            }
            let v = base(blocks[t.0].seg) + pos[t.0] + toff;
            buf[at + off..at + off + 8].copy_from_slice(&v.to_le_bytes());
        }
    }
    let mut raw = virt;
    raw.extend_from_slice(&phys);
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
    enc.write_all(&raw).map_err(|e| e.to_string())?;
    let comp = enc.finish().map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(16 + comp.len());
    out.extend_from_slice(&MAGIC);
    // compressor id 1 (deflate) stored minus one; no encryption (key -1, +1)
    out.extend_from_slice(&[version, 0, 0, 0]);
    out.extend_from_slice(&(vsize as u32 | (version as u32 >> 4)).to_le_bytes());
    out.extend_from_slice(&(psize as u32 | (version as u32 & 0xF)).to_le_bytes());
    out.extend_from_slice(&comp);
    Ok(out)
}

/// A decompressed RSC8 file.
pub struct Rsc8 {
    pub version: u8,
    pub virt: Vec<u8>,
    pub phys: Vec<u8>,
}

impl Rsc8 {
    /// Parse a loose deflate RSC8 file (Oodle bodies are an error).
    pub fn parse(bytes: &[u8]) -> Result<Rsc8, String> {
        if bytes.len() < 16 || bytes[..4] != MAGIC {
            return Err("not an RSC8 file".into());
        }
        let version = bytes[4];
        // compressor id (1 deflate, 2 Oodle), stored minus one
        let compressor = bytes[5] + 1;
        if bytes[6] != 0 {
            return Err(format!("encrypted resource (key byte {})", bytes[6]));
        }
        let vf = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let pf = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        let ver2 = ((vf & 0xF) << 4 | (pf & 0xF)) as u8;
        if ver2 != version {
            return Err(format!("version byte {version} but flag nibbles say {ver2}"));
        }
        let (vsize, psize) = ((vf & !0xF) as usize, (pf & !0xF) as usize);
        if compressor != 1 {
            return Err(format!("compressor {compressor} not supported (deflate only)"));
        }
        let mut raw = Vec::with_capacity(vsize + psize);
        DeflateDecoder::new(&bytes[16..]).read_to_end(&mut raw).map_err(|e| format!("inflate: {e}"))?;
        if raw.len() != vsize + psize {
            return Err(format!("payload {} bytes, flags say {} + {}", raw.len(), vsize, psize));
        }
        let phys = raw.split_off(vsize);
        Ok(Rsc8 { version, virt: raw, phys })
    }

    /// Segment and offset of a pointer whose target has `len` bytes in range.
    pub fn resolve(&self, p: u64, len: usize) -> Result<(Seg, usize), String> {
        let (seg, off, size) = match p >> 28 {
            5 => (Seg::Sys, (p & 0x0FFF_FFFF) as usize, self.virt.len()),
            6 => (Seg::Gfx, (p & 0x0FFF_FFFF) as usize, self.phys.len()),
            _ => return Err(format!("bad pointer 0x{p:X}")),
        };
        if off + len > size {
            return Err(format!("pointer 0x{p:X} (+{len}) past its segment ({size} bytes)"));
        }
        Ok((seg, off))
    }

    /// Virtual offset of a non-null pointer to `len` bytes of system data.
    pub fn sys(&self, p: u64, len: usize) -> Result<usize, String> {
        match self.resolve(p, len)? {
            (Seg::Sys, o) => Ok(o),
            _ => Err(format!("pointer 0x{p:X} is physical, expected virtual")),
        }
    }

    pub fn u8(&self, o: usize) -> u8 {
        self.virt[o]
    }
    pub fn u16(&self, o: usize) -> u16 {
        u16::from_le_bytes(self.virt[o..o + 2].try_into().unwrap())
    }
    pub fn u32(&self, o: usize) -> u32 {
        u32::from_le_bytes(self.virt[o..o + 4].try_into().unwrap())
    }
    pub fn u64(&self, o: usize) -> u64 {
        u64::from_le_bytes(self.virt[o..o + 8].try_into().unwrap())
    }
    pub fn f32(&self, o: usize) -> f32 {
        f32::from_le_bytes(self.virt[o..o + 4].try_into().unwrap())
    }

    /// NUL-terminated string behind a virtual pointer.
    pub fn cstr(&self, p: u64) -> Result<String, String> {
        let o = self.sys(p, 1)?;
        let end = self.virt[o..].iter().position(|&c| c == 0).ok_or("unterminated string")?;
        Ok(String::from_utf8_lossy(&self.virt[o..o + end]).into_owned())
    }
}
