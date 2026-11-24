//! RDR2 `_manifest.ymf` (PSO `CPackFileMetaData`) writer and reader: which
//! ytyps a ymap needs (imapDependencies_2), which ytyps an interior ytyp
//! needs (itypDependencies_2), and an interior's collision files
//! (Interiors). Format facts from the user's working RedM manifests and
//! their own writer (big-endian throughout):
//!
//! - `PSIN`: 16-byte header (`PSIN`, total length, 8 zero bytes), then data
//!   blocks 16-aligned: 1 the root (0xB8 bytes; 16-byte array headers:
//!   MapDataGroups +0x10, imapDependencies_2 +0x30, itypDependencies_2
//!   +0x40, Interiors +0x50; the byte at +0xA0 = 1), then the present ones
//!   of MapDataGroups[] (kept opaque), CImapDependencies[] (0x18: name,
//!   manifestFlags, itypDepArray), one u32[] block (every dependency /
//!   bounds hash, in the order the arrays are written), CItypDependencies[]
//!   (0x18), CInteriorBoundsFiles[] (0x18: name, 0, Bounds). An array header
//!   is `u32 (offset << 20 | block), u32 0, u16 count, u16 capacity, u32 0`.
//! - `PMAP`: root block 1, u32 0, u16 block count, `pppppp`, then per block
//!   (struct hash, offset, 0, length).
//! - `PSCH`: the structure schema, game data: `PSCH` below is the 620-byte
//!   section of a working manifest with exactly the block types written here
//!   (root, CImapDependencies, u32, CItypDependencies, CInteriorBoundsFiles).

use crate::gta5::rsc::joaat;

pub const ROOT: u32 = 0x93A6_8A2F;
pub const MAP_DATA_GROUPS: u32 = 0x5986_9C63;
pub const IMAP: u32 = 0xC11F_3EE1;
pub const U32: u32 = 0x0000_0006;
pub const ITYP: u32 = 0x5A56_4E50;
pub const BOUNDS: u32 = 0x2C32_5290;
/// manifestFlags value of interior data.
pub const INTERIOR_DATA: u32 = 1;
const ROOT_SIZE: usize = 0xB8;

/// PSCH section (schema of root, CImapDependencies, u32[],
/// CItypDependencies, CInteriorBoundsFiles) from a working manifest; must
/// be paired with exactly those block types.
pub const PSCH: &[u8] = &super::manifest_psch::PSCH;

/// One dependency entry: name hash, manifestFlags, dependency hashes.
#[derive(Debug, Clone, PartialEq)]
pub struct Dep {
    pub name: u32,
    pub flags: u32,
    pub deps: Vec<u32>,
}

/// A manifest's contents (hashes).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Manifest {
    /// MapDataGroups items, kept opaque: (count, the block's bytes up to the
    /// next block, the block length PMAP declares: the sample's PMAP length
    /// is larger than the bytes before the next block); not written by
    /// `redm interior`
    pub map_data_groups: Option<(u16, Vec<u8>, usize)>,
    pub imaps: Vec<Dep>,
    pub ityps: Vec<Dep>,
    /// interior name hash -> bounds file hashes
    pub interiors: Vec<(u32, Vec<u32>)>,
}

impl Manifest {
    /// The manifest of one streamed interior, as the working resources
    /// declare theirs: the milo ymap with INTERIOR_DATA depending on the
    /// interior ytyp, the ytyp with INTERIOR_DATA (and its prop-pack
    /// dependencies), and the interior's collision pair `[hi@<name>, <name>]`.
    pub fn interior(milo: &str, ytyp: &str, interior: &str, ytyp_deps: &[&str]) -> Manifest {
        Manifest {
            map_data_groups: None,
            imaps: vec![Dep { name: joaat(milo), flags: INTERIOR_DATA, deps: vec![joaat(ytyp)] }],
            ityps: vec![Dep { name: joaat(ytyp), flags: INTERIOR_DATA, deps: ytyp_deps.iter().map(|d| joaat(d)).collect() }],
            interiors: vec![(joaat(interior), vec![joaat(&format!("hi@{interior}")), joaat(interior)])],
        }
    }
}

fn arr(block: u32, offset: usize, count: usize) -> Vec<u8> {
    let p = if count == 0 { 0 } else { (offset as u32) << 20 | block };
    let mut b = p.to_be_bytes().to_vec();
    b.extend([0; 4]);
    b.extend((count as u16).to_be_bytes());
    b.extend((count as u16).to_be_bytes());
    b.extend([0; 4]);
    b
}

/// The PSO file with schema section `psch`.
pub fn build_with(m: &Manifest, psch: &[u8]) -> Result<Vec<u8>, String> {
    if psch.get(..4) != Some(b"PSCH") {
        return Err("manifest: schema section does not start with PSCH".into());
    }
    let mut hashes: Vec<u32> = Vec::new();
    let any_deps = m.imaps.iter().chain(&m.ityps).any(|d| !d.deps.is_empty()) || m.interiors.iter().any(|i| !i.1.is_empty());
    let mut order = vec![ROOT];
    if m.map_data_groups.is_some() {
        order.push(MAP_DATA_GROUPS);
    }
    if !m.imaps.is_empty() {
        order.push(IMAP);
    }
    if any_deps {
        order.push(U32);
    }
    if !m.ityps.is_empty() {
        order.push(ITYP);
    }
    if !m.interiors.is_empty() {
        order.push(BOUNDS);
    }
    let bid = |t: u32| order.iter().position(|&x| x == t).map_or(0, |i| i as u32 + 1);
    let mut items = |list: &[(u32, u32, &Vec<u32>)]| -> Vec<u8> {
        let mut b = Vec::new();
        for &(n, f, d) in list {
            let start = hashes.len() * 4;
            hashes.extend(d);
            b.extend(n.to_be_bytes());
            b.extend(f.to_be_bytes());
            b.extend(arr(bid(U32), start, d.len()));
        }
        b
    };
    let imap_b = items(&m.imaps.iter().map(|d| (d.name, d.flags, &d.deps)).collect::<Vec<_>>());
    let ityp_b = items(&m.ityps.iter().map(|d| (d.name, d.flags, &d.deps)).collect::<Vec<_>>());
    let int_b = items(&m.interiors.iter().map(|(n, b)| (*n, 0, b)).collect::<Vec<_>>());
    let u32_b: Vec<u8> = hashes.iter().flat_map(|h| h.to_be_bytes()).collect();
    let mut root = vec![0u8; ROOT_SIZE];
    if let Some((n, _, _)) = &m.map_data_groups {
        root[0x10..0x20].copy_from_slice(&arr(bid(MAP_DATA_GROUPS), 0, *n as usize));
    }
    if !m.imaps.is_empty() {
        root[0x30..0x40].copy_from_slice(&arr(bid(IMAP), 0, m.imaps.len()));
    }
    if !m.ityps.is_empty() {
        root[0x40..0x50].copy_from_slice(&arr(bid(ITYP), 0, m.ityps.len()));
    }
    if !m.interiors.is_empty() {
        root[0x50..0x60].copy_from_slice(&arr(bid(BOUNDS), 0, m.interiors.len()));
    }
    root[0xA0] = 1;
    let mut body = Vec::new();
    let mut pmap = Vec::new();
    let mut o = 0x10usize;
    for &t in &order {
        let declared = m.map_data_groups.as_ref().filter(|_| t == MAP_DATA_GROUPS).map(|g| g.2);
        let b: &[u8] = match t {
            ROOT => &root,
            MAP_DATA_GROUPS => &m.map_data_groups.as_ref().unwrap().1,
            IMAP => &imap_b,
            U32 => &u32_b,
            ITYP => &ityp_b,
            _ => &int_b,
        };
        pmap.push((t, o, declared.unwrap_or(b.len())));
        body.extend_from_slice(b);
        let pad = (16 - b.len() % 16) % 16;
        body.extend(std::iter::repeat_n(0u8, pad));
        o += b.len() + pad;
    }
    let mut out = b"PSIN".to_vec();
    out.extend(((16 + body.len()) as u32).to_be_bytes());
    out.extend([0u8; 8]);
    out.extend(body);
    let mut pm = Vec::new();
    pm.extend(1u32.to_be_bytes());
    pm.extend(0u32.to_be_bytes());
    pm.extend((pmap.len() as u16).to_be_bytes());
    pm.extend(b"pppppp");
    for (t, off, len) in pmap {
        pm.extend(t.to_be_bytes());
        pm.extend((off as u32).to_be_bytes());
        pm.extend(0u32.to_be_bytes());
        pm.extend((len as u32).to_be_bytes());
    }
    out.extend(b"PMAP");
    out.extend(((8 + pm.len()) as u32).to_be_bytes());
    out.extend(pm);
    out.extend_from_slice(psch);
    Ok(out)
}

/// The PSO file with the embedded schema (no MapDataGroups).
pub fn build(m: &Manifest) -> Result<Vec<u8>, String> {
    if m.map_data_groups.is_some() {
        return Err("manifest: MapDataGroups need a schema that has them (the embedded one does not)".into());
    }
    build_with(m, PSCH)
}

/// Top-level sections (tag, bytes incl. header).
pub fn sections(d: &[u8]) -> Result<Vec<([u8; 4], &[u8])>, String> {
    let mut out = Vec::new();
    let mut o = 0;
    while o + 8 <= d.len() {
        let tag: [u8; 4] = d[o..o + 4].try_into().unwrap();
        let len = u32::from_be_bytes(d[o + 4..o + 8].try_into().unwrap()) as usize;
        if len < 8 || o + len > d.len() {
            return Err(format!("section {} at {o}: length {len} past the file", String::from_utf8_lossy(&tag)));
        }
        out.push((tag, &d[o..o + len]));
        o += len;
    }
    if o != d.len() {
        return Err(format!("{} trailing bytes", d.len() - o));
    }
    Ok(out)
}

/// Parse a manifest (blocks by PMAP, arrays checked against their blocks).
pub fn read(d: &[u8]) -> Result<Manifest, String> {
    let secs = sections(d)?;
    let get = |t: &[u8; 4]| secs.iter().find(|s| &s.0 == t).map(|s| s.1).ok_or(format!("no {} section", String::from_utf8_lossy(t)));
    let (psin, pm) = (get(b"PSIN")?, get(b"PMAP")?);
    get(b"PSCH")?;
    let be32 = |b: &[u8], o: usize| u32::from_be_bytes(b[o..o + 4].try_into().unwrap());
    let be16 = |b: &[u8], o: usize| u16::from_be_bytes(b[o..o + 2].try_into().unwrap());
    if pm.len() < 24 || &pm[18..24] != b"pppppp" {
        return Err("PMAP header".into());
    }
    let n = be16(pm, 16) as usize;
    if pm.len() < 24 + 16 * n {
        return Err("PMAP shorter than its block list".into());
    }
    let mut blocks = Vec::new();
    for i in 0..n {
        let e = 24 + 16 * i;
        let (t, off, len) = (be32(pm, e), be32(pm, e + 4) as usize, be32(pm, e + 12) as usize);
        let b = psin.get(off..off + len).ok_or(format!("block {} past PSIN", i + 1))?;
        blocks.push((t, b));
    }
    let root_id = be32(pm, 8) as usize;
    let root = blocks.get(root_id.wrapping_sub(1)).filter(|b| b.0 == ROOT && b.1.len() >= ROOT_SIZE).ok_or("root block")?.1;
    // array header at `o` of `b` -> (block, offset, count)
    let array = |b: &[u8], o: usize, ty: u32, esize: usize| -> Result<Option<(&[u8], usize, usize)>, String> {
        let (p, cnt) = (be32(b, o), be16(b, o + 8) as usize);
        if p == 0 || cnt == 0 {
            return Ok(None);
        }
        let (id, off) = ((p & 0xFFFFF) as usize, (p >> 20) as usize);
        let blk = blocks.get(id.wrapping_sub(1)).ok_or(format!("array pointer 0x{p:08X}: no block {id}"))?;
        if blk.0 != ty || off + cnt * esize > blk.1.len() {
            return Err(format!("array pointer 0x{p:08X} x {cnt}: block {id} is 0x{:08X} of {} bytes", blk.0, blk.1.len()));
        }
        Ok(Some((blk.1, off, cnt)))
    };
    let deps = |list: Option<(&[u8], usize, usize)>| -> Result<Vec<Dep>, String> {
        let Some((b, off, cnt)) = list else { return Ok(Vec::new()) };
        (0..cnt)
            .map(|k| {
                let o = off + 0x18 * k;
                let hs = match array(b, o + 8, U32, 4)? {
                    Some((u, uo, un)) => (0..un).map(|j| be32(u, uo + 4 * j)).collect(),
                    None => Vec::new(),
                };
                Ok(Dep { name: be32(b, o), flags: be32(b, o + 4), deps: hs })
            })
            .collect()
    };
    let map_data_groups = match blocks.iter().position(|b| b.0 == MAP_DATA_GROUPS) {
        Some(i) => {
            let (p, c) = (be32(root, 0x10), be16(root, 0x18));
            if p & 0xFFFFF != i as u32 + 1 {
                return Err("MapDataGroups array does not point at its block".into());
            }
            let e = 24 + 16 * i;
            let (off, len) = (be32(pm, e + 4) as usize, be32(pm, e + 12) as usize);
            // bytes up to the next block (the PMAP length can overrun it)
            let next = (0..n).map(|k| be32(pm, 24 + 16 * k + 4) as usize).filter(|&x| x > off).min().unwrap_or(psin.len());
            Some((c, psin[off..next.min(off + len)].to_vec(), len))
        }
        None => None,
    };
    Ok(Manifest {
        map_data_groups,
        imaps: deps(array(root, 0x30, IMAP, 0x18)?)?,
        ityps: deps(array(root, 0x40, ITYP, 0x18)?)?,
        interiors: deps(array(root, 0x50, BOUNDS, 0x18)?)?.into_iter().map(|d| (d.name, d.deps)).collect(),
    })
}

/// The schema section of a manifest file.
pub fn psch(d: &[u8]) -> Result<Vec<u8>, String> {
    sections(d)?.into_iter().find(|s| &s.0 == b"PSCH").map(|s| s.1.to_vec()).ok_or("no PSCH section".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interior_manifest_round_trip() {
        let m = Manifest::interior("int_t_milo_", "int_t", "int_t", &[]);
        let b = build(&m).unwrap();
        assert_eq!(&b[..4], b"PSIN");
        assert_eq!(read(&b).unwrap(), m);
        let s = sections(&b).unwrap();
        assert_eq!(s.iter().map(|x| x.0).collect::<Vec<_>>(), vec![*b"PSIN", *b"PMAP", *b"PSCH"]);
        assert_eq!(s[2].1, PSCH);
        assert_eq!(m.interiors[0].1, vec![joaat("hi@int_t"), joaat("int_t")]);
        assert_eq!((m.imaps[0].flags, m.ityps[0].flags), (INTERIOR_DATA, INTERIOR_DATA));
        // empty dependency lists: null arrays, no u32 block needed beyond the others
        let mut e = m.clone();
        e.ityps[0].deps = vec![joaat("p_door")];
        assert_eq!(read(&build(&e).unwrap()).unwrap(), e);
        assert!(read(&b[..b.len() - 1]).is_err());
    }

    /// The user's working manifests (not in the repo): rebuilt from their
    /// own decoded values, byte for byte. `R6M_MANIFESTS=<dir with *.ymf>`.
    #[test]
    fn samples_rebuild_byte_exact() {
        let Some(dir) = std::env::var_os("R6M_MANIFESTS") else { return };
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            if e.path().extension().is_some_and(|x| x == "ymf") {
                let d = std::fs::read(e.path()).unwrap();
                let m = read(&d).unwrap();
                let r = build_with(&m, &psch(&d).unwrap()).unwrap();
                let at = r.iter().zip(&d).position(|(a, b)| a != b);
                assert!(r == d, "{}: {} vs {} bytes, first difference at {at:?}", e.path().display(), r.len(), d.len());
                n += 1;
            }
        }
        assert!(n > 0);
    }
}
