//! RDR2 `.ybn` (static bounds, RSC8 version 47) writer and reader: a
//! Composite of GeometryBVH children (triangles), as interior collision
//! (`<interior>.ybn` + `hi@<interior>.ybn`, interior-local space).
//!
//! Derived data is built the way CodeX builds it (format facts from the
//! ybn spec, checked byte for byte against CodeX-written goldens): quantum
//! = half box extent / 32767, CenterGeom = box middle, vertex shorts
//! truncated toward zero, polygon boxes = float vertices +- margin, BVH
//! with item threshold 4 and mean splits (fallback: sorted halves),
//! children sorted by item count, polygons in BVH leaf order, edge
//! neighbours, triangle area word 1 (float.Epsilon), materials padded to
//! 4. Everything is in the virtual segment; container pointers only.

use super::rsc8::{self, Rsc8};
use crate::gta5::rsc::{BlockId, Builder, Seg, W};

pub const YBN_VERSION: u8 = 47;
/// Collision type / include flags of a walkable + weapon child and of a
/// weapon-only (`hi@`) child (vanilla interiors, the user's pairs).
pub const TYPE_MOVER_WEAPON: u64 = 0x3E;
pub const INCLUDE_MOVER_WEAPON: u64 = 0x7F3_BEC0;
pub const TYPE_WEAPON: u64 = 0x2;
pub const INCLUDE_WEAPON: u64 = 0x61_0000;
/// Margin of geometry children (all vanilla and user geometry).
pub const MARGIN: f32 = 0.005;
/// Per child: polygon ids are i16 in BVH nodes, vertex indices u16.
const MAX_POLYS: usize = 32767;
const MAX_VERTS: usize = 65535;

/// Collision material id of a materials.meta name: `(hash(name) mod 0x7FED)
/// + 18`, hash = the ELF-style `h = c + 16h` fold; "" = DEFAULT (12667).
pub fn material_id(name: &str) -> u16 {
    let name = if name.is_empty() { "DEFAULT" } else { name };
    let mut h: u32 = 0;
    for &c in name.as_bytes() {
        h = (c as u32).wrapping_add(h.wrapping_mul(16));
        let top = h & 0xF000_0000;
        if top != 0 {
            h ^= top ^ (top >> 24);
        }
    }
    (h % 0x7FED + 18) as u16
}

/// Material1 word: material id, room id in bits 24..28.
pub fn material_word(id: u16, room: u8) -> u32 {
    id as u32 | ((room as u32 & 31) << 24)
}

/// One GeometryBVH child: box (vertex bounds +- margin), vertices relative
/// to the box middle, triangles with a material index each.
pub struct Geometry {
    pub box_min: [f32; 3],
    pub box_max: [f32; 3],
    pub margin: f32,
    pub verts: Vec<[f32; 3]>,
    pub tris: Vec<[u32; 3]>,
    pub tri_mat: Vec<u8>,
    /// (Material1, Material2)
    pub materials: Vec<(u32, u32)>,
    pub type_flags: u64,
    pub include_flags: u64,
}

fn mid(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| (a[k] + b[k]) * 0.5)
}

impl Geometry {
    /// Children from absolute (interior-space) triangles: vertices welded
    /// (bit-identical positions), split so each child stays within the
    /// u16 vertex / i16 polygon limits.
    pub fn from_triangles(verts: &[[f32; 3]], tris: &[[u32; 3]], tri_mat: &[u8], materials: &[(u32, u32)], type_flags: u64, include_flags: u64) -> Vec<Geometry> {
        let mut out = Vec::new();
        for chunk in (0..tris.len()).collect::<Vec<_>>().chunks(MAX_POLYS) {
            let mut map = std::collections::HashMap::new();
            let mut vs: Vec<[f32; 3]> = Vec::new();
            let mut ts = Vec::new();
            let mut ms = Vec::new();
            for &t in chunk {
                if vs.len() + 3 > MAX_VERTS {
                    break; // never hit below MAX_POLYS triangles (3 x 32767 > 65535 only without welding)
                }
                let tri = tris[t].map(|i| {
                    let v = verts[i as usize];
                    *map.entry(v.map(f32::to_bits)).or_insert_with(|| {
                        vs.push(v);
                        vs.len() as u32 - 1
                    })
                });
                ts.push(tri);
                ms.push(tri_mat[t]);
            }
            if ts.is_empty() {
                continue;
            }
            let mut lo = [f32::MAX; 3];
            let mut hi = [f32::MIN; 3];
            for v in &vs {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
            let (box_min, box_max) = (lo.map(|x| x - MARGIN), hi.map(|x| x + MARGIN));
            let c = mid(box_min, box_max);
            let rel = vs.iter().map(|v| std::array::from_fn(|k| v[k] - c[k])).collect();
            out.push(Geometry { box_min, box_max, margin: MARGIN, verts: rel, tris: ts, tri_mat: ms, materials: materials.to_vec(), type_flags, include_flags });
        }
        out
    }
}

/// Derived arrays of one GeometryBVH, as CodeX writes them.
pub struct Derived {
    pub quantum: [f32; 3],
    pub center: [f32; 3],
    /// vertex shorts
    pub verts: Vec<[i16; 3]>,
    /// polygons in final (BVH) order: vertices, edge neighbours, material index
    pub polys: Vec<([u32; 3], [i16; 3], u8)>,
    /// 16-byte nodes
    pub nodes: Vec<[i16; 8]>,
    /// BVH header: min, max, center, quantum inverse, quantum (x, y, z)
    pub bvh: [[f32; 3]; 5],
}

struct Item {
    min: [f32; 3],
    max: [f32; 3],
    poly: usize,
}

struct Node {
    min: [f32; 3],
    max: [f32; 3],
    items: Vec<Item>,
    children: Vec<Node>,
}

impl Node {
    fn total_items(&self) -> usize {
        self.items.len() + self.children.iter().map(Node::total_items).sum::<usize>()
    }
    fn total_nodes(&self) -> usize {
        1 + self.children.iter().map(Node::total_nodes).sum::<usize>()
    }
}

fn centre(it: &Item) -> [f32; 3] {
    std::array::from_fn(|k| (it.min[k] + it.max[k]) * 0.5)
}

fn build_node(items: Vec<Item>, thr: usize) -> Node {
    let mut min = [f32::MAX; 3];
    let mut max = [-f32::MAX; 3];
    for it in &items {
        for k in 0..3 {
            min[k] = min[k].min(it.min[k]);
            max[k] = max[k].max(it.max[k]);
        }
    }
    if items.len() <= thr {
        return Node { min, max, items, children: Vec::new() };
    }
    let n = items.len();
    let mut sum = [0f32; 3];
    for it in &items {
        for k in 0..3 {
            sum[k] += it.min[k];
            sum[k] += it.max[k];
        }
    }
    let s = (0.5f64 / n as f64) as f32;
    let avg = sum.map(|x| x * s);
    let mut cnt = [0usize; 3];
    for it in &items {
        let c = centre(it);
        for k in 0..3 {
            cnt[k] += (c[k] < avg[k]) as usize;
        }
    }
    let half = n as f64 / 2.0;
    let dd = cnt.map(|c| (half - c as f64).abs());
    let axis = if dd[0] <= dd[1] && dd[0] <= dd[2] { 0 } else if dd[1] <= dd[2] { 1 } else { 2 };
    let (mut l1, mut l2): (Vec<Item>, Vec<Item>) = items.into_iter().partition(|it| centre(it)[axis] > avg[axis]);
    if l1.is_empty() || l2.is_empty() {
        let mut l3: Vec<Item> = l1.into_iter().chain(l2).collect();
        let key = |it: &Item| (it.min, it.max);
        l3.sort_by(|a, b| key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal));
        l2 = l3.split_off(l3.len() / 2);
        l1 = l3;
    }
    let mut children = vec![build_node(l1, thr), build_node(l2, thr)];
    children.sort_by_key(|c| std::cmp::Reverse(c.total_items()));
    Node { min, max, items: Vec::new(), children }
}

fn gather<'a>(n: &'a Node, out: &mut Vec<&'a Node>) {
    out.push(n);
    for c in &n.children {
        gather(c, out);
    }
}

/// Quantum, centre, vertex shorts, BVH (threshold 4), polygon order, edge
/// neighbours and node records of one geometry.
pub fn derive(g: &Geometry) -> Result<Derived, String> {
    if g.tris.is_empty() {
        return Err("geometry without triangles".into());
    }
    let quantum: [f32; 3] = std::array::from_fn(|k| ((g.box_max[k] - g.box_min[k]) * 0.5) / 32767.0);
    let center = mid(g.box_min, g.box_max);
    let verts = g
        .verts
        .iter()
        .map(|v| {
            let s: [f32; 3] = std::array::from_fn(|k| (v[k] / quantum[k]).trunc());
            if s.iter().any(|x| x.abs() > 32767.0) {
                return Err(format!("vertex {v:?} outside the box"));
            }
            Ok(s.map(|x| x as i16))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let items: Vec<Item> = g
        .tris
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let p: Vec<[f32; 3]> = t.iter().map(|&k| std::array::from_fn(|a| g.verts[k as usize][a] + center[a])).collect();
            let min = std::array::from_fn(|a| p.iter().map(|v| v[a]).fold(f32::MAX, f32::min) - g.margin);
            let max = std::array::from_fn(|a| p.iter().map(|v| v[a]).fold(f32::MIN, f32::max) + g.margin);
            Item { min, max, poly: i }
        })
        .collect();
    let mut mn = [f32::MAX; 3];
    let mut mx = [-f32::MAX; 3];
    for it in &items {
        for k in 0..3 {
            mn[k] = mn[k].min(it.min[k]);
            mx[k] = mx[k].max(it.max[k]);
        }
    }
    let cen: [f32; 3] = std::array::from_fn(|k| (mn[k] + mx[k]) * 0.5);
    let bq: [f32; 3] = std::array::from_fn(|k| (mn[k] - cen[k]).abs().max((mx[k] - cen[k]).abs()) / 32767.0);
    let qi = bq.map(|q| 1.0 / q);
    let root = build_node(items, 4);
    let mut nodes = Vec::new();
    gather(&root, &mut nodes);
    // polygons in leaf order
    let mut first = vec![0usize; nodes.len()];
    let mut order = Vec::new();
    for (k, n) in nodes.iter().enumerate() {
        first[k] = order.len();
        order.extend(n.items.iter().map(|it| it.poly));
    }
    let q16 = |v: f32| v.clamp(-32767.0, 32767.0) as i16;
    let recs = nodes
        .iter()
        .enumerate()
        .map(|(k, n)| {
            let lo: [i16; 3] = std::array::from_fn(|a| q16(((n.min[a] - cen[a]) * qi[a]).floor()));
            let hi: [i16; 3] = std::array::from_fn(|a| q16(((n.max[a] - cen[a]) * qi[a]).ceil()));
            let tn = n.total_nodes();
            let (id, count) = if tn <= 1 { (first[k], n.total_items()) } else { (tn, 0) };
            [lo[0], lo[1], lo[2], hi[0], hi[1], hi[2], id as i16, count as i16]
        })
        .collect();
    // edge neighbours on the final order
    let p: Vec<[u32; 3]> = order.iter().map(|&i| g.tris[i]).collect();
    let mut ei = vec![[-1i16; 3]; p.len()];
    let mut edges: std::collections::HashMap<(u32, u32), ((usize, usize), Option<(usize, usize)>)> = std::collections::HashMap::new();
    for (i, t) in p.iter().enumerate() {
        for (k, (x, y)) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])].into_iter().enumerate() {
            let key = (x.min(y), x.max(y));
            match edges.get_mut(&key) {
                Some(e) => match e.1 {
                    Some(_) => ei[i][k] = e.0 .0 as i16,
                    None => e.1 = Some((i, k)),
                },
                None => {
                    edges.insert(key, ((i, k), None));
                }
            }
        }
    }
    for (_, (a, b)) in edges {
        if let Some(b) = b {
            ei[a.0][a.1] = b.0 as i16;
            ei[b.0][b.1] = a.0 as i16;
        }
    }
    let polys = order.iter().enumerate().map(|(k, &i)| (g.tris[i], ei[k], g.tri_mat[i])).collect();
    Ok(Derived { quantum, center, verts, polys, nodes: recs, bvh: [mn, mx, cen, qi, bq] })
}

fn v3w(w: &mut W, v: [f32; 3], last: f32) {
    w.f32(v[0]).f32(v[1]).f32(v[2]).f32(last);
}

/// Base bounds (112 bytes) of a composite / geometry: box, centres = box
/// middle, inertia (1,1,1).
fn base(w: &mut W, ty: u8, unk11: u8, blockmap: Option<BlockId>, lo: [f32; 3], hi: [f32; 3], margin: f32, mass: f32) {
    let c = mid(lo, hi);
    let r = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f32>().sqrt() * 0.5;
    w.u64(0).ptr(blockmap).u8(ty).u8(unk11).u16(0).f32(r).u64(0);
    v3w(w, hi, margin);
    // BoxMin's 4th lane holds Unknown_3Ch = 1 (a u32)
    v3w(w, lo, f32::from_bits(1));
    v3w(w, c, 0.0); // BoxCenter, Material1 0
    v3w(w, c, 0.0); // SphereCenter, Material2 0
    v3w(w, [1.0; 3], mass);
}

fn geometry_block(b: &mut Builder, g: &Geometry) -> Result<BlockId, String> {
    let d = derive(g)?;
    if g.materials.is_empty() || g.materials.len() > 255 {
        return Err(format!("{} materials (1..255)", g.materials.len()));
    }
    let id = b.reserve(Seg::Sys);
    let mut pw = W::new();
    for (t, e, _) in &d.polys {
        pw.u32(1).u16(t[0] as u16).u16(t[1] as u16).u16(t[2] as u16).u16(e[0] as u16).u16(e[1] as u16).u16(e[2] as u16);
    }
    let polys = b.add(Seg::Sys, pw);
    let mut vw = W::new();
    for v in &d.verts {
        vw.u16(v[0] as u16).u16(v[1] as u16).u16(v[2] as u16);
    }
    let verts = b.add(Seg::Sys, vw);
    let mut mw = W::new();
    for k in 0..g.materials.len().max(4) {
        let m = g.materials.get(k).copied().unwrap_or((0, 0));
        mw.u32(m.0).u32(m.1);
    }
    let mats = b.add(Seg::Sys, mw);
    let mi = b.add(Seg::Sys, W { b: d.polys.iter().map(|p| p.2).collect(), ptrs: Vec::new() });
    let mut nw = W::new();
    for n in &d.nodes {
        for x in n {
            nw.u16(*x as u16);
        }
    }
    let nodes = b.add(Seg::Sys, nw);
    let [mn, mx, cen, qi, bq] = d.bvh;
    let mut bw = W::new();
    v3w(&mut bw, mn, f32::from_bits(1));
    v3w(&mut bw, mx, 0.0);
    v3w(&mut bw, cen, 0.0);
    v3w(&mut bw, qi, f32::from_bits(0xFFC0_0000));
    v3w(&mut bw, bq, 0.0);
    bw.ptr(Some(nodes)).u32(d.nodes.len() as u32).u32(d.nodes.len() as u32).zeros(16);
    debug_assert_eq!(bw.len(), 112);
    let bvh = b.add(Seg::Sys, bw);
    let mut w = W::new();
    base(&mut w, 5, 64, None, g.box_min, g.box_max, g.margin, 1.0);
    let (nv, np) = (d.verts.len() as u32, d.polys.len() as u32);
    w.u64(0).u64(0).u16(0).u16(0).u32(nv).ptr(Some(polys));
    v3w(&mut w, d.quantum, 0.0);
    v3w(&mut w, d.center, 0.0);
    w.ptr(Some(verts)).u64(0).zeros(16).u32(nv).u32(np).zeros(24);
    w.ptr(Some(mats)).u64(0).zeros(24).ptr(Some(mi));
    w.u8(g.materials.len() as u8).u8(0).u16(0).u32(0).u64(0);
    w.ptr(Some(bvh)).u64(0).u16(0xFFFF).u16(0).u32(0).u64(0);
    debug_assert_eq!(w.len(), 336);
    b.set(id, w);
    Ok(id)
}

/// A .ybn: one Composite (identity child matrices, m44 = 1; no composite
/// BVH below 6 children) of GeometryBVH children.
pub fn build_ybn(children: &[Geometry]) -> Result<Vec<u8>, String> {
    if children.is_empty() {
        return Err("ybn: no children".into());
    }
    if children.len() >= 6 {
        return Err("ybn: 6 or more children need a composite BVH (not written)".into());
    }
    let mut b = Builder::new();
    let root = b.reserve(Seg::Sys);
    let bm = b.sys_bytes(vec![0u8; 16]);
    b.pages_info = Some(bm);
    let kids = children.iter().map(|g| geometry_block(&mut b, g)).collect::<Result<Vec<_>, _>>()?;
    let mut cw = W::new();
    for k in &kids {
        cw.ptr(Some(*k));
    }
    let cptr = b.add(Seg::Sys, cw);
    let mut mw = W::new();
    for _ in children {
        for r in 0..4 {
            for c in 0..4 {
                mw.f32(if r == c { 1.0 } else { 0.0 });
            }
        }
    }
    let mats = b.add(Seg::Sys, mw);
    let mut fw = W::new();
    for g in children {
        fw.u64(g.type_flags).u64(g.include_flags);
    }
    let flags = b.add(Seg::Sys, fw);
    let lo = std::array::from_fn(|k| children.iter().map(|g| g.box_min[k]).fold(f32::MAX, f32::min));
    let hi = std::array::from_fn(|k| children.iter().map(|g| g.box_max[k]).fold(f32::MIN, f32::max));
    let mut w = W::new();
    base(&mut w, 6, 0, Some(bm), lo, hi, 0.0, children.len() as f32);
    let n = children.len() as u16;
    w.ptr(Some(cptr)).ptr(Some(mats)).ptr(Some(mats)).ptr(Some(flags)).ptr(Some(flags)).u16(n).u16(n).u32(0).u64(0).u64(0);
    debug_assert_eq!(w.len(), 176);
    b.set(root, w);
    rsc8::build(b, YBN_VERSION)
}

/// Summary of a read-back .ybn.
#[derive(Debug, Clone)]
pub struct YbnInfo {
    pub vsize: usize,
    pub box_min: [f32; 3],
    pub box_max: [f32; 3],
    /// per child: (type, type flags, include flags, vertices, polygons, materials, BVH nodes)
    pub children: Vec<(u8, u64, u64, u32, u32, Vec<(u32, u32)>, u32)>,
}

/// Walk a .ybn: composite, every child pointer, geometry arrays in range,
/// polygon vertex indices < vertex count, material indices < material
/// count, BVH leaves covering every polygon once.
pub fn read(bytes: &[u8]) -> Result<YbnInfo, String> {
    let r = Rsc8::parse(bytes)?;
    if r.version != YBN_VERSION {
        return Err(format!("ybn version {} (want {YBN_VERSION})", r.version));
    }
    if r.u8(16) != 6 {
        return Err(format!("root bound type {} (want 6 Composite)", r.u8(16)));
    }
    let v3 = |o: usize| [r.f32(o), r.f32(o + 4), r.f32(o + 8)];
    let n = r.u16(154) as usize;
    if r.u16(152) as usize != n || n == 0 {
        return Err(format!("composite bounds {}/{n}", r.u16(152)));
    }
    let cp = r.sys(r.u64(112), 8 * n)?;
    r.sys(r.u64(120), 64 * n)?;
    let fl = r.sys(r.u64(136), 16 * n)?;
    let mut children = Vec::new();
    for k in 0..n {
        let co = r.sys(r.u64(cp + 8 * k), 112)?;
        let ty = r.u8(co + 16);
        let (tf, inc) = (r.u64(fl + 16 * k), r.u64(fl + 16 * k + 8));
        if ty != 5 {
            children.push((ty, tf, inc, 0, 0, Vec::new(), 0));
            continue;
        }
        r.sys(r.u64(cp + 8 * k), 336)?;
        let (nv, np) = (r.u32(co + 208), r.u32(co + 212));
        let nm = r.u8(co + 288) as usize;
        let po = r.sys(r.u64(co + 136), 16 * np as usize)?;
        r.sys(r.u64(co + 176), 6 * nv as usize)?;
        let mo = r.sys(r.u64(co + 240), 8 * nm.max(4))?;
        let mi = r.sys(r.u64(co + 280), np as usize)?;
        let mats: Vec<(u32, u32)> = (0..nm).map(|i| (r.u32(mo + 8 * i), r.u32(mo + 8 * i + 4))).collect();
        for p in 0..np as usize {
            let o = po + 16 * p;
            if r.u8(o + 3) & 7 != 0 {
                return Err(format!("child {k} polygon {p}: type {} (only triangles are written)", r.u8(o + 3) & 7));
            }
            if (0..3).any(|i| r.u16(o + 4 + 2 * i) as u32 >= nv) || r.u8(mi + p) as usize >= nm {
                return Err(format!("child {k} polygon {p}: vertex or material index out of range"));
            }
        }
        let bo = r.sys(r.u64(co + 304), 112)?;
        let nn = r.u32(bo + 88);
        let no = r.sys(r.u64(bo + 80), 16 * nn as usize)?;
        let mut covered = vec![0u8; np as usize];
        for i in 0..nn as usize {
            let o = no + 16 * i;
            let (id, cnt) = (r.u16(o + 12) as usize, r.u16(o + 14) as usize);
            if cnt > 0 {
                for p in id..id + cnt {
                    *covered.get_mut(p).ok_or(format!("child {k} node {i}: items {id}+{cnt} past {np}"))? += 1;
                }
            }
        }
        if covered.iter().any(|&c| c != 1) {
            return Err(format!("child {k}: BVH leaves do not cover every polygon exactly once"));
        }
        children.push((ty, tf, inc, nv, np, mats, nn));
    }
    Ok(YbnInfo { vsize: r.virt.len(), box_min: v3(48), box_max: v3(32), children })
}
