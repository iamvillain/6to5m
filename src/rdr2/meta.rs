//! Generic RDR2 RSC8 "meta" writer (`.ytyp`, `.ymap`), driven by the class
//! and enum definition tables in `meta_defs.rs`.
//!
//! Container (RSC8 version 3, virtual segment only): a 0x60-byte root
//! (magic `0DRP`, platform 0x79), the class table (32 B per class, with its
//! member entries), the enum table (24 B per enum, with its values), the
//! DataBlocks list (16 B per block), 12 zero bytes of UnkData1, and the data
//! blocks. Inside data blocks, pointers are meta pointers
//! `(offset << 20) | block id` (1-based block index).
//!
//! Data-block rules (format facts, checked against CodeX-written files):
//! - an object goes into the first block of its type with `len + n <
//!   0xFFFFF`, else a new block of that type;
//! - block types below 0x20 (pointer arrays 7, strings 0x10, u32 0x15) are
//!   packed, every other type (struct classes, 0x33 float4, 0x4A hashes) is
//!   8-aligned;
//! - blocks are numbered in order of first use; callers write depth first
//!   (an object's arrays after the object, a pointer array after its
//!   elements, a struct array reserved before its elements' arrays);
//! - the class table lists classes in the order they are first used (with
//!   the inline struct classes of a class right after it), the enum table
//!   the enums in the order an enum field is met while adding classes.

use super::meta_defs;
use super::rsc8;
use crate::gta5::rsc::{joaat, Builder, Seg, W};

/// Class definition: name hash, key, unk8, size, entries
/// `(name hash, offset, type, unk9, ref index, type name hash)`.
pub struct ClassDef {
    pub name: u32,
    pub key: u32,
    pub unk8: u32,
    pub size: u32,
    pub entries: &'static [(u32, i32, u8, u8, u16, u32)],
}

/// Enum definition: name hash, key, (value, name hash).
pub struct EnumDef {
    pub name: u32,
    pub key: u32,
    pub values: &'static [(i64, u32)],
}

pub const META_MAGIC: u32 = 0x5052_4430; // "0DRP"
pub const META_PLATFORM: u16 = 0x79;
pub const META_VERSION: u8 = 3;
/// Data block types of the plain arrays.
pub const T_PTRS: u32 = 0x07;
pub const T_CHARS: u32 = 0x10;
pub const T_U32: u32 = 0x15;
pub const T_F4: u32 = 0x33;
pub const T_HASH: u32 = 0x4A;
/// Member type codes.
pub const M_STRUCT: u8 = 0x05;
pub const M_ENUM32: u8 = 0x62;
/// Array element descriptors are named 0x100.
pub const ELEM: u32 = 0x100;
const LIMIT: usize = 0xF_FFFF;

/// Meta pointer to `offset` inside data block `id` (1-based).
pub fn meta_ptr(id: u32, offset: usize) -> u64 {
    (offset as u64) << 20 | id as u64
}

/// Hash field: joaat of the lower-cased name, 0 for none.
pub fn name_hash(s: &str) -> u32 {
    if s.is_empty() { 0 } else { joaat(s) }
}

pub fn class_by_hash(h: u32) -> Option<&'static ClassDef> {
    meta_defs::CLASSES.iter().copied().find(|c| c.name == h)
}

fn enum_by_hash(h: u32) -> Option<&'static EnumDef> {
    meta_defs::ENUMS.iter().copied().find(|e| e.name == h)
}

/// An array / CharPtr header value: (meta pointer, count).
pub type Arr = (u64, u16);

/// One object's bytes, written field by field through its class table
/// (fields are found by name hash; a wrong name or size is a bug and panics).
pub struct Obj {
    pub class: &'static ClassDef,
    pub b: Vec<u8>,
}

impl Obj {
    pub fn new(class: &'static ClassDef) -> Obj {
        Obj { class, b: vec![0; class.size as usize] }
    }

    fn at(&mut self, field: u32, types: &[u8]) -> usize {
        let e = self.class.entries.iter().find(|e| e.0 == field && e.0 != ELEM).unwrap_or_else(|| panic!("class 0x{:08X} has no field 0x{field:08X}", self.class.name));
        assert!(types.contains(&e.2), "class 0x{:08X} field 0x{field:08X} has type 0x{:02X}, written as {types:02X?}", self.class.name, e.2);
        e.1 as usize
    }

    fn put(&mut self, o: usize, v: &[u8]) -> &mut Obj {
        self.b[o..o + v.len()].copy_from_slice(v);
        self
    }

    pub fn u8(&mut self, f: u32, v: u8) -> &mut Obj {
        let o = self.at(f, &[0x11, 0x01]);
        self.put(o, &[v])
    }
    pub fn u32(&mut self, f: u32, v: u32) -> &mut Obj {
        let o = self.at(f, &[0x15, 0x4A, 0x62, 0x14, 0x32]);
        self.put(o, &v.to_le_bytes())
    }
    pub fn i32(&mut self, f: u32, v: i32) -> &mut Obj {
        self.u32(f, v as u32)
    }
    pub fn u64(&mut self, f: u32, v: u64) -> &mut Obj {
        let o = self.at(f, &[0x17, 0x08]);
        self.put(o, &v.to_le_bytes())
    }
    pub fn f32(&mut self, f: u32, v: f32) -> &mut Obj {
        let o = self.at(f, &[0x21]);
        self.put(o, &v.to_le_bytes())
    }
    /// Hash of a lower-cased name (0 for "").
    pub fn hash(&mut self, f: u32, name: &str) -> &mut Obj {
        self.u32(f, name_hash(name))
    }
    /// Float3 with its 4th lane.
    pub fn f3(&mut self, f: u32, v: [f32; 3], w: f32) -> &mut Obj {
        let o = self.at(f, &[0x33]);
        let b: Vec<u8> = [v[0], v[1], v[2], w].iter().flat_map(|x| x.to_le_bytes()).collect();
        self.put(o, &b)
    }
    pub fn f4(&mut self, f: u32, v: [f32; 4]) -> &mut Obj {
        let o = self.at(f, &[0x34]);
        let b: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        self.put(o, &b)
    }
    /// Array or CharPtr header `{u64 ptr, u16 count, u16 capacity, u32 0}`.
    pub fn arr(&mut self, f: u32, a: Arr) -> &mut Obj {
        let o = self.at(f, &[0x52, 0x44]);
        let mut b = a.0.to_le_bytes().to_vec();
        b.extend_from_slice(&a.1.to_le_bytes());
        b.extend_from_slice(&a.1.to_le_bytes());
        self.put(o, &b)
    }
}

/// Data blocks plus the classes and enums used so far.
#[derive(Default)]
pub struct Meta {
    pub blocks: Vec<(u32, Vec<u8>)>,
    classes: Vec<&'static ClassDef>,
    enums: Vec<&'static EnumDef>,
}

impl Meta {
    /// Add a class to the class table (with its inline struct classes and
    /// the enums its fields use), if not there yet.
    pub fn use_class(&mut self, c: &'static ClassDef) {
        if self.classes.iter().any(|x| x.name == c.name) {
            return;
        }
        self.classes.push(c);
        for e in c.entries.iter().filter(|e| e.0 != ELEM) {
            match e.2 {
                M_ENUM32 => {
                    if let Some(en) = enum_by_hash(e.5).filter(|en| !self.enums.iter().any(|x| x.name == en.name)) {
                        self.enums.push(en);
                    }
                }
                M_STRUCT => {
                    if let Some(s) = class_by_hash(e.5) {
                        self.use_class(s);
                    }
                }
                _ => {}
            }
        }
    }

    /// Room for `n` bytes in a block of type `ty`: (block id, offset).
    fn alloc(&mut self, ty: u32, n: usize) -> (u32, usize) {
        let align = |l: usize| if ty < 0x20 { l } else { l.div_ceil(8) * 8 };
        let i = match self.blocks.iter().position(|b| b.0 == ty && align(b.1.len()) + n < LIMIT) {
            Some(i) => i,
            None => {
                self.blocks.push((ty, Vec::new()));
                self.blocks.len() - 1
            }
        };
        let off = align(self.blocks[i].1.len());
        self.blocks[i].1.resize(off + n, 0);
        (i as u32 + 1, off)
    }

    /// Reserve `n` objects of class `c` (contiguous); returns the meta
    /// pointer of the first. Fill them with `set`.
    pub fn reserve(&mut self, c: &'static ClassDef, n: usize) -> u64 {
        self.use_class(c);
        let (id, off) = self.alloc(c.name, n * c.size as usize);
        meta_ptr(id, off)
    }

    /// Overwrite reserved bytes at meta pointer `p`.
    pub fn set(&mut self, p: u64, b: &[u8]) {
        let (i, off) = ((p & 0xFFFF) as usize - 1, (p >> 20) as usize);
        self.blocks[i].1[off..off + b.len()].copy_from_slice(b);
    }

    /// One object: reserved and written.
    pub fn object(&mut self, o: &Obj) -> u64 {
        let p = self.reserve(o.class, 1);
        self.set(p, &o.b);
        p
    }

    /// Raw elements of an array of block type `ty`; empty -> (0, 0).
    pub fn array(&mut self, ty: u32, bytes: &[u8], count: usize) -> Arr {
        if count == 0 {
            return (0, 0);
        }
        let (id, off) = self.alloc(ty, bytes.len());
        self.blocks[id as usize - 1].1[off..off + bytes.len()].copy_from_slice(bytes);
        (meta_ptr(id, off), count as u16)
    }

    /// Pointer array (StructPtr elements), appended after its elements.
    pub fn ptrs(&mut self, ptrs: &[u64]) -> Arr {
        let b: Vec<u8> = ptrs.iter().flat_map(|p| p.to_le_bytes()).collect();
        self.array(T_PTRS, &b, ptrs.len())
    }

    pub fn u32s(&mut self, v: &[u32]) -> Arr {
        let b: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        self.array(T_U32, &b, v.len())
    }

    pub fn hashes(&mut self, v: &[u32]) -> Arr {
        let b: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        self.array(T_HASH, &b, v.len())
    }

    /// Float4 elements (16 bytes each: also float3 arrays such as corners).
    pub fn f4s(&mut self, v: &[[f32; 4]]) -> Arr {
        let b: Vec<u8> = v.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        self.array(T_F4, &b, v.len())
    }

    /// CharPtr string (no NUL, count = capacity = length, as CodeX writes).
    pub fn string(&mut self, s: &str) -> Arr {
        self.array(T_CHARS, s.as_bytes(), s.len())
    }

    /// The RSC8 file. The root object must be in data block `root_block`.
    pub fn build(self, root_block: u32) -> Result<Vec<u8>, String> {
        if self.blocks.len() > u16::MAX as usize || self.classes.len() > u16::MAX as usize {
            return Err("meta: too many blocks or classes".into());
        }
        let mut b = Builder::new();
        // creation order (stable size sort in rsc8::build): root, member
        // entries, enum values, BlockMap, class table, enum table, data
        // blocks, block list, UnkData1
        let root = b.reserve(Seg::Sys);
        let entries: Vec<_> = self
            .classes
            .iter()
            .map(|c| {
                let mut w = W::new();
                for &(name, off, ty, u9, rf, tn) in c.entries {
                    w.u32(name).i32(off).u8(ty).u8(u9).u16(rf).u32(tn);
                }
                b.add(Seg::Sys, w)
            })
            .collect();
        let values: Vec<_> = self
            .enums
            .iter()
            .map(|e| {
                let mut w = W::new();
                for &(v, h) in e.values {
                    w.u64(v as u64).u32(h).u32(0);
                }
                b.add(Seg::Sys, w)
            })
            .collect();
        let bm = b.sys_bytes(vec![0u8; 48]);
        b.pages_info = Some(bm);
        let mut classes = W::new();
        for (c, e) in self.classes.iter().zip(&entries) {
            classes.u32(c.name).u32(c.key).u32(c.unk8).u32(0).ptr(Some(*e)).u32(c.size).u16(0).u16(c.entries.len() as u16);
        }
        let classes = b.add(Seg::Sys, classes);
        let enums = if self.enums.is_empty() {
            None
        } else {
            let mut w = W::new();
            for (e, v) in self.enums.iter().zip(&values) {
                w.u32(e.name).u32(e.key).ptr(Some(*v)).u32(e.values.len() as u32).u32(0);
            }
            Some(b.add(Seg::Sys, w))
        };
        let mut list = W::new();
        let n = self.blocks.len();
        for (ty, data) in self.blocks {
            let len = data.len() as u32;
            list.u32(ty).u32(len).ptr(Some(b.add(Seg::Sys, W { b: data, ptrs: Vec::new() })));
        }
        let list_id = b.add(Seg::Sys, list);
        let unk = b.sys_bytes(vec![0u8; 12]);
        let mut r = W::new();
        r.u64(0).ptr(Some(bm)).u32(META_MAGIC).u16(META_PLATFORM).u8(0).u8(0).u64(0);
        r.u32(root_block).u32(0);
        r.ptr(Some(classes)).ptr(enums).ptr(Some(list_id)).ptr(Some(unk));
        r.u16(self.classes.len() as u16).u16(self.enums.len() as u16).u16(n as u16).u16(0).u64(0).u64(0);
        debug_assert_eq!(r.len(), 0x60);
        b.set(root, r);
        rsc8::build(b, META_VERSION)
    }
}
