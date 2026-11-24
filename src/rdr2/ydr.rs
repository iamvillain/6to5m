//! RDR2 `.ydr` (drawable, RSC8 version 162) writer for static, unskinned
//! props, from the GTA V writer's in-memory `gta5::ydr::Mesh` / `Geom`.
//!
//! Layout worked out from vanilla props and checked against
//! CodeX-written files: 208-byte drawable, shader group, 64-byte shaders
//! with a (hash, packed info) parameter table and one flat parameter block
//! (buffer pointers, 4 copies of the cbuffer data, texture pointers, sampler
//! state ids), 80-byte texture stubs naming textures of the prop's .ytd,
//! one model per LOD level, 64-byte geometries with separate vertex / index
//! buffers (each with its own SRV), and structure-of-arrays vertex data
//! described by a 316-byte 52-slot vertex format.
//!
//! Shaders: `default` (diffuse only; POSITION Float3, NORMAL Dec3N, COLOR0
//! RGBA8, TEXCOORD0 Float2) and `standard` (diffuse + normal map + `_ma`
//! material map + blank `_mb`; adds TANGENT Dec3N with w = bitangent sign and TEXCOORD1),
//! plus `standard_decal`, `standard_alpha` and `standard_glass` (same maps
//! and layout as `standard`), one `ShaderDef` table entry each.
//! Parameter lists and values are those of named vanilla props.
//! Shader catalogue facts cross-checked with Sollumz_RDR (GPL-3.0).

use super::{rsc8, ytd};
use crate::gta5::{
    rsc::{BlockId, Builder, Seg, W},
    ydr::{uv_tangents, Geom, Mesh, FLAT_BUMP},
};
use std::collections::HashMap;

const VFT_SHADERGROUP: u64 = 0x1_4091_2C88;
const VFT_MODEL: u64 = 0x1_4091_2C58;
const VFT_GEOMETRY: u64 = 0x1_4091_2C48;
const VFT_VB: u64 = 0x1_4091_2400;
const VFT_IB: u64 = 0x1_4091_23E0;
/// Texture referenced by name (lives in another dictionary).
const VFT_TEXREF: u64 = 0x1_4091_0008;
const SRV_BUFFER: u64 = 0x104;
const SHADER_TAG: u32 = 0x6D65_7461; // "meta"

/// Names of the stand-ins `standard` shaders bind when a geometry has no
/// own `_ma` (`Geom::spec`), and always for `_mb`; the caller puts them into
/// the prop's .ytd (`ytd::blank_ma`, `ytd::blank_mb`, vanilla values).
pub const BLANK_MA: &str = "r6m_blank_ma";
pub const BLANK_MB: &str = "r6m_blank_mb";
/// Flat (128, 128, 255) normal map of `standard` geometries without one.
pub const FLAT_NORMAL: &str = "r6m_flat_n";

// vertex element slots and format codes (52-slot vertex format)
const SLOT_POSITION: usize = 0;
const SLOT_NORMAL: usize = 4;
const SLOT_TANGENT: usize = 8;
const SLOT_COLOR0: usize = 24;
const SLOT_TEXCOORD0: usize = 28;
const SLOT_TEXCOORD1: usize = 29;
const FMT_FLOAT3: u8 = 6;
const FMT_FLOAT2: u8 = 16;
const FMT_DEC3N: u8 = 24;
const FMT_COLOUR: u8 = 28;

/// Dec3N: 10:10:10:2, each axis offset-binary (`0x200 + n` for v >= 0,
/// `!n & 0x1FF` below, n = |v| * 511): +1 -> 1023, 0 -> 512, -1 -> 0. `w` is
/// two-bit signed (-1 stored as 3).
pub fn dec3n(v: [f32; 3], w: i8) -> u32 {
    let enc = |x: f32| -> u32 {
        let x = if x.is_finite() { x.clamp(-1.0, 1.0) } else { 0.0 };
        let n = ((x.abs() * 511.0) as u32).min(511);
        if x >= 0.0 { 0x200 + n } else { !n & 0x1FF }
    };
    enc(v[0]) | enc(v[1]) << 10 | enc(v[2]) << 20 | ((w as u32) & 3) << 30
}

/// Inverse of `dec3n`.
#[cfg(test)]
pub fn dec3n_decode(d: u32) -> ([f32; 3], i8) {
    let dec = |r: u32| -> f32 { if r & 0x200 != 0 { (r - 0x200) as f32 / 511.0 } else { -((!r & 0x1FF) as f32) / 511.0 } };
    let w = (d >> 30) as i8;
    ([dec(d & 0x3FF), dec(d >> 10 & 0x3FF), dec(d >> 20 & 0x3FF)], if w == 3 { -1 } else { w })
}

/// Texture slot meaning in a shader definition.
#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Null,
    Diffuse,
    Bump,
    Spec,
    Spec2,
}

/// One cbuffer parameter: (name hash, byte offset in its buffer, default).
type Param = (u32, u16, &'static [f32]);

/// A shader definition: everything its parameter block and parameter table
/// hold. Each buffer is (size, parameter runs concatenated in table order);
/// `set` overrides defaults of parameters shared with other definitions.
pub struct ShaderDef {
    pub name: &'static str,
    pub hash: u32,
    buffers: &'static [(u16, &'static [&'static [Param]])],
    textures: &'static [(u32, Slot)],
    /// (sampler name hash, global sampler state id)
    samplers: &'static [(u32, u8)],
    set: &'static [(u32, &'static [f32])],
}

impl ShaderDef {
    /// Every cbuffer parameter as (hash, buffer, offset, value), table order.
    pub fn cbuffers(&self) -> impl Iterator<Item = (u32, u8, u16, &'static [f32])> + '_ {
        self.buffers.iter().enumerate().flat_map(move |(b, (_, runs))| {
            runs.iter().flat_map(|r| r.iter()).map(move |&(h, off, v)| (h, b as u8, off, self.set.iter().find(|s| s.0 == h).map_or(v, |s| s.1)))
        })
    }

    pub fn buffer_sizes(&self) -> Vec<u16> {
        self.buffers.iter().map(|b| b.0).collect()
    }

    #[cfg(test)]
    pub fn texture_hashes(&self) -> Vec<u32> {
        self.textures.iter().map(|t| t.0).collect()
    }

    pub fn samplers(&self) -> &'static [(u32, u8)] {
        self.samplers
    }

    /// Shader parameter block size: buffer pointers (4 x nb), 4 data
    /// copies, texture pointers (4 x nt), one byte per sampler.
    pub fn block_size(&self) -> usize {
        let total: usize = self.buffers.iter().map(|b| b.0 as usize).sum();
        32 * self.buffers.len() + 4 * total + 32 * self.textures.len() + self.samplers.len()
    }
}

// cbuffer runs shared by the definitions (vanilla offsets and defaults)
/// `default` buffer 0.
const P_DEFAULT_B0: &[Param] = &[
    (0xF086_53A5, 0, &[1.0, 1.0, 1.0, 0.0]), // tintoverride
    (0x32DD_9FF5, 16, &[1.0]),               // wetnessmultiplier
    (0x4620_A35D, 20, &[0.0]),               // usetessellation
    (0xA693_4C16, 24, &[0.0]),               // marktaaghostingbit
];
/// Three unnamed parameters: `default` buffer 1, `standard_decal` buffer 0.
const P_UNNAMED: &[Param] = &[(0x9FBB_A584, 0, &[0.0, 0.0]), (0x7CF9_469A, 8, &[0.0, 0.0]), (0xFEB9_BA87, 16, &[0.0])];
/// The `standard` family's parameters up to hardalphablend (offset 156).
const P_STD_HEAD: &[Param] = &[
    (0xFDD7_96CF, 0, &[0.0, 0.0]),              // tintpaletteselector
    (0xB51E_2E8F, 16, &[0.0, 0.0, 24.0, 24.0]), // detailsettings
    (0xCC2F_ED8E, 32, &[-1.0]),                 // detailindexoverride
    (0xB5FE_3ACB, 36, &[0.0]),                  // fresnel
    (0xFF11_711D, 48, &[1.0, 0.0, 0.0]),        // specmapintmask
    (0xF086_53A5, 64, &[1.0, 1.0, 1.0, 0.0]),   // tintoverride
    (0x93F1_06FF, 80, &[0.0]),                  // pbrglassiness
    (0x48A8_92C5, 84, &[0.0]),                  // pbrcavityrange
    (0x9FE5_F2B2, 88, &[0.0]),                  // pbrtweak_metallic
    (0xD665_9424, 92, &[0.0]),                  // pbrtweak_sheen
    (0x3E9E_055F, 96, &[0.0]),                  // pbrtweak_roughness
    (0xE785_1D21, 100, &[0.0]),                 // pbrtweak_ao
    (0x3712_0671, 104, &[0.0]),                 // pbrtweak_translucency
    (0x3942_822D, 108, &[0.0]),                 // pbrtweak_fresnel
    (0x1B5A_9556, 112, &[0.0]),                 // pbrtweak_detail
    (0x1C1A_CF8C, 116, &[-1.0]),                // pbrtweak_detailindex_
    (0x48C7_E72B, 120, &[0.0]),                 // pbrdetail_metallic
    (0x1A33_BFED, 124, &[0.0]),                 // pbrdetail_sheen
    (0xA0C5_5C99, 128, &[0.0]),                 // pbrdetail_roughness
    (0xC4B7_D819, 132, &[0.0]),                 // pbrdetail_ao
    (0x9861_B27C, 136, &[0.0]),                 // pbrdetail_translucency
    (0xF671_2B81, 140, &[1.0]),                 // bumpiness
    (0x32DD_9FF5, 144, &[1.0]),                 // wetnessmultiplier
    (0x6603_5C96, 148, &[0.5]),                 // porosity
    (0x4620_A35D, 152, &[0.0]),                 // usetessellation
    (0xE943_7406, 156, &[0.0]),                 // hardalphablend
];
/// `standard` / `standard_decal` after hardalphablend (puddle parameters).
const P_STD_TAIL: &[Param] = &[
    (0xA693_4C16, 160, &[0.0]), // marktaaghostingbit
    (0x5CE9_B178, 164, &[0.0]), // usepaintedwetness
    (0x042A_8B79, 168, &[0.0]), // haspuddleswitch
    (0xD87B_4F49, 172, &[0.0]), // puddleheightbias
    (0xFB38_2D27, 176, &[2.0]), // puddlemaskscale
    (0x3819_99F3, 180, &[0.0]), // translucencyastintmask
];
/// `standard_alpha` after hardalphablend.
const P_ALPHA_TAIL: &[Param] = &[
    (0xA693_4C16, 160, &[0.0]), // marktaaghostingbit
    (0x3819_99F3, 164, &[0.0]), // translucencyastintmask
];
/// `standard_glass` after hardalphablend.
const P_GLASS_TAIL: &[Param] = &[
    (0xEE85_0962, 160, &[0.2, 0.7]), // bloommod
    (0xA693_4C16, 168, &[0.0]),      // marktaaghostingbit
    (0x3819_99F3, 172, &[0.0]),      // translucencyastintmask
];

/// Texture slots of `standard`, `standard_alpha` and `standard_glass`.
const T_STD: &[(u32, Slot)] = &[
    (0x2C8D_7182, Slot::Null), // tintpalettetex
    (0xE9F3_2960, Slot::Null),
    (0xC8E8_C282, Slot::Diffuse), // diffusetex
    (0x0CD2_7763, Slot::Null),    // detailtexarrayn
    (0x1E77_BF4C, Slot::Bump),    // bumptex
    (0xC6F4_6E79, Slot::Spec),    // speculartex (_ma)
    (0x739E_3D0D, Slot::Spec2),   // speculartex2 (_mb)
];

// sampler name hashes
const S_POINTCLAMP: u32 = 0x77CA_165F;
const S_TRILINEARCLAMP: u32 = 0x184D_4D47;
const S_DIFFUSE: u32 = 0xF1FE_2B71;
const S_TRILINEARWRAP: u32 = 0xE446_90BB;
const S_MIPLINEARCLAMP: u32 = 0x7904_5347;
const S_ANISO16WRAP: u32 = 0x5505_25C7;
const S_PROJDECAL: u32 = 0xD997_7F30;
const S_MIPLINEARWRAP: u32 = 0x738C_A901;
const S_BUMP: u32 = 0x46B7_C64F;
const S_SPEC: u32 = 0x6087_99C6;
const S_TRILINEARBORDERBLACK: u32 = 0xD837_3AEC;
const S_SPEC2: u32 = 0xE50A_5060;
const S_RAIN: u32 = 0x8BC2_A8EB;
const S_SHADOW: u32 = 0x24C5_AB07;

/// `default` (joaat 0xE4DF46D5), as vanilla p_cratemultiplayerammo01x.
const DEFAULT: ShaderDef = ShaderDef {
    name: "default",
    hash: 0xE4DF_46D5,
    buffers: &[(32, &[P_DEFAULT_B0]), (32, &[P_UNNAMED])],
    textures: &[(0xE9F3_2960, Slot::Null), (0xC8E8_C282, Slot::Diffuse), (0x56D8_55C5, Slot::Null)],
    samplers: &[(S_DIFFUSE, 1), (S_POINTCLAMP, 0), (S_TRILINEARCLAMP, 7), (S_TRILINEARWRAP, 13), (S_MIPLINEARCLAMP, 8), (S_MIPLINEARWRAP, 10), (S_TRILINEARBORDERBLACK, 11), (S_SHADOW, 9)],
    set: &[],
};

/// `standard` (joaat 0xBCF03FC1), as the static LodMed model of vanilla
/// p_canteen01x. Cutout geometries use it too, in bucket 3 (vanilla
/// p_sign02x, p_helpwantedsign01x).
const STANDARD: ShaderDef = ShaderDef {
    name: "standard",
    hash: 0xBCF0_3FC1,
    buffers: &[(192, &[P_STD_HEAD, P_STD_TAIL])],
    textures: T_STD,
    samplers: &[
        (S_POINTCLAMP, 0),
        (S_TRILINEARCLAMP, 7),
        (S_DIFFUSE, 1),
        (S_TRILINEARWRAP, 13),
        (S_MIPLINEARCLAMP, 8),
        (S_ANISO16WRAP, 2),
        (S_PROJDECAL, 12),
        (S_MIPLINEARWRAP, 10),
        (S_BUMP, 3),
        (S_SPEC, 4),
        (S_TRILINEARBORDERBLACK, 11),
        (S_SPEC2, 5),
        (S_RAIN, 6),
        (S_SHADOW, 9),
    ],
    set: &[],
};

/// `standard_decal` (joaat 0xE0B4C573), bucket 2, as vanilla
/// p_helpwantedsign01x.yft (both LODs): the three unnamed parameters of
/// `default`'s second buffer, then `standard`'s buffer; one more texture slot.
const STANDARD_DECAL: ShaderDef = ShaderDef {
    name: "standard_decal",
    hash: 0xE0B4_C573,
    buffers: &[(32, &[P_UNNAMED]), (192, &[P_STD_HEAD, P_STD_TAIL])],
    textures: &[
        (0x2C8D_7182, Slot::Null),
        (0xE9F3_2960, Slot::Null),
        (0xC8E8_C282, Slot::Diffuse),
        (0x0CD2_7763, Slot::Null),
        (0x1E77_BF4C, Slot::Bump),
        (0xC6F4_6E79, Slot::Spec),
        (0x739E_3D0D, Slot::Spec2),
        (0x56D8_55C5, Slot::Null),
    ],
    samplers: &[
        (S_TRILINEARCLAMP, 7),
        (S_DIFFUSE, 1),
        (S_POINTCLAMP, 0),
        (S_MIPLINEARCLAMP, 8),
        (S_ANISO16WRAP, 2),
        (S_PROJDECAL, 12),
        (S_MIPLINEARWRAP, 10),
        (S_BUMP, 3),
        (S_SPEC, 4),
        (S_TRILINEARBORDERBLACK, 11),
        (S_SPEC2, 5),
        (S_RAIN, 6),
        (S_SHADOW, 9),
    ],
    set: &[],
};

/// Samplers of `standard_alpha` / `standard_glass` (vanilla order).
const S_ALPHA: &[(u32, u8)] = &[
    (S_POINTCLAMP, 0),
    (S_DIFFUSE, 1),
    (S_TRILINEARCLAMP, 7),
    (S_MIPLINEARCLAMP, 8),
    (S_ANISO16WRAP, 2),
    (S_MIPLINEARWRAP, 10),
    (S_BUMP, 3),
    (S_SPEC, 4),
    (S_TRILINEARBORDERBLACK, 11),
    (S_SPEC2, 5),
    (S_RAIN, 6),
    (S_SHADOW, 9),
];

/// `standard_alpha` (joaat 0xE3BA3129), bucket 1: layout of vanilla
/// p_strmillwindow01ax, defaults of the catalogue and vanilla
/// rc_windows_01 (glassiness 0, cavity 0, bumpiness 1).
const STANDARD_ALPHA: ShaderDef = ShaderDef {
    name: "standard_alpha",
    hash: 0xE3BA_3129,
    buffers: &[(176, &[P_STD_HEAD, P_ALPHA_TAIL])],
    textures: T_STD,
    samplers: S_ALPHA,
    set: &[],
};

/// `standard_glass` (joaat 0x5C884B59), bucket 1, as vanilla p_lamp01x
/// (glassiness 1, bloommod 0.2 / 0.7).
const STANDARD_GLASS: ShaderDef = ShaderDef {
    name: "standard_glass",
    hash: 0x5C88_4B59,
    buffers: &[(176, &[P_STD_HEAD, P_GLASS_TAIL])],
    textures: T_STD,
    samplers: S_ALPHA,
    set: &[(0x93F1_06FF, &[1.0])],
};

/// Shader a geometry gets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shader {
    Default,
    Standard,
    Decal,
    Alpha,
    Glass,
}

impl Shader {
    pub fn def(self) -> &'static ShaderDef {
        match self {
            Shader::Default => &DEFAULT,
            Shader::Standard => &STANDARD,
            Shader::Decal => &STANDARD_DECAL,
            Shader::Alpha => &STANDARD_ALPHA,
            Shader::Glass => &STANDARD_GLASS,
        }
    }

    /// The `standard` family: normal, `_ma` and `_mb` maps, PNXCTT layout
    /// (TANGENT and TEXCOORD1 on top of `default`'s PNCT).
    pub fn pbr(self) -> bool {
        self != Shader::Default
    }
}

/// Writer options.
#[derive(Clone, Copy)]
pub struct Opts {
    /// Use `standard` for opaque geometries with a normal map (else every
    /// geometry is `default`).
    pub standard: bool,
    /// With `standard`: opaque geometries WITHOUT a usable normal map are
    /// `standard` too, binding the flat normal map `FLAT_NORMAL` (and the
    /// blank, matte `_ma` when they have none): `default` has no roughness
    /// map, so the engine's default gloss made matte 6m surfaces shiny.
    pub flat_normal: bool,
    /// With `standard`: the other kinds get their `standard`-family shader
    /// too (`shader_for`), with a normal map (flat stand-in when absent) and
    /// `_ma` (blank when absent). Off: only opaque geometries can be
    /// `standard`, the rest stay `default`.
    pub kinds: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { standard: true, flat_normal: false, kinds: false }
    }
}

/// One shader of the drawable: definition, bucket and texture names.
#[derive(Clone, PartialEq)]
struct Mat {
    shader: Shader,
    bucket: u8,
    diffuse: String,
    bump: String,
    /// `_ma` (speculartex) of a `standard`-family material
    spec: String,
}

/// GTA V geometry kind (ydr::Geom::kind) -> RDR2 draw bucket of a
/// `default` shader: opaque 0, alpha 1 (alpha, decal, glass), cutout 3.
fn bucket_of(kind: u8) -> u8 {
    match kind {
        1 | 2 | 5 => 1,
        3 => 3,
        _ => 0,
    }
}

/// Shader and draw bucket of a geometry. With `Opts::kinds` (the prop
/// path), as vanilla RDR2 props: opaque `standard` bucket 0, cutout
/// `standard` bucket 3, decal `standard_decal` bucket 2, alpha
/// `standard_alpha` bucket 1, glass `standard_glass` bucket 1.
pub fn shader_for(g: &Geom, o: &Opts) -> (Shader, u8) {
    let has_normal = !g.normal.is_empty() && g.normal != FLAT_BUMP;
    let fallback = (Shader::Default, bucket_of(g.kind));
    if !o.standard {
        return fallback;
    }
    match g.kind {
        0 if has_normal || o.flat_normal => (Shader::Standard, 0),
        _ if !o.kinds => fallback,
        3 => (Shader::Standard, 3),
        2 => (Shader::Decal, 2),
        1 => (Shader::Alpha, 1),
        5 => (Shader::Glass, 1),
        // TODO emissive (kind 4): vanilla `emissive` (joaat 0x8F245469); stays `default` for now
        _ => fallback,
    }
}

fn mat_of(g: &Geom, o: &Opts) -> Mat {
    let has_normal = !g.normal.is_empty() && g.normal != FLAT_BUMP;
    let (shader, bucket) = shader_for(g, o);
    let pbr = shader.pbr();
    let spec = match pbr {
        false => String::new(),
        true if g.spec.is_empty() => BLANK_MA.to_string(),
        true => g.spec.to_ascii_lowercase(),
    };
    let bump = match (pbr, has_normal) {
        (false, _) => String::new(),
        (true, true) => g.normal.to_ascii_lowercase(),
        (true, false) => FLAT_NORMAL.to_string(),
    };
    Mat { shader, bucket, diffuse: g.texture.to_ascii_lowercase(), bump, spec }
}

/// Every diffuse (albedo) texture name the drawable binds.
pub fn diffuse_names(levels: &[Mesh], o: &Opts) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for g in levels.iter().flat_map(|l| &l.geoms) {
        let d = mat_of(g, o).diffuse;
        if !d.is_empty() && !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// Every texture name the drawable references (diffuse, normal and `_ma`
/// maps, and the `standard` stand-ins), in first-use order.
pub fn texture_names(levels: &[Mesh], o: &Opts) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |n: &str| {
        if !n.is_empty() && !out.iter().any(|x| x == n) {
            out.push(n.to_string());
        }
    };
    for g in levels.iter().flat_map(|l| &l.geoms) {
        let m = mat_of(g, o);
        add(&m.diffuse);
        if m.shader.pbr() {
            add(&m.bump);
            add(&m.spec);
            add(BLANK_MB);
        }
    }
    out
}

/// 80-byte texture reference block naming `name`.
fn add_texref(b: &mut Builder, name: &str) -> BlockId {
    let nm = b.string(name);
    let mut w = W::new();
    w.u64(VFT_TEXREF).u32(0).u32(0).u32(0x1000_0000).u32(0);
    w.u16(0).u16(0).u16(1).u8(1).u8(0);
    w.u8(0xFF).u8(0).u8(1).u8(0).u8(0).u8(0).u16(1);
    w.ptr(Some(nm)).u64(0).u64(0).zeros(16);
    debug_assert_eq!(w.len(), 80);
    b.add(Seg::Sys, w)
}

/// One shader block (+ its parameter data / block and texture references).
fn add_shader(b: &mut Builder, m: &Mat, embedded: &HashMap<String, BlockId>) -> BlockId {
    let def = m.shader.def();
    let nb = def.buffers.len();
    let total: usize = def.buffers.iter().map(|s| s.0 as usize).sum();
    let nt = def.textures.len();
    let ns = def.samplers.len();
    let tex_off = 32 * nb + 4 * total;
    let size = def.block_size();
    // parameter block: buffer pointers (4 copies x nb), 4 data copies,
    // texture pointers (4 x nt, only the first nt used), sampler ids
    let pb = b.reserve(Seg::Sys);
    let mut starts = vec![0usize; nb];
    for j in 1..nb {
        starts[j] = starts[j - 1] + def.buffers[j - 1].0 as usize;
    }
    let mut w = W::new();
    for c in 0..4 {
        for j in 0..nb {
            w.ptr_at(Some(pb), (32 * nb + c * total + starts[j]) as u64);
        }
    }
    let mut data = vec![0u8; total];
    for (_, buf, off, vals) in def.cbuffers() {
        let at = starts[buf as usize] + off as usize;
        for (k, v) in vals.iter().enumerate() {
            data[at + 4 * k..at + 4 * k + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    for _ in 0..4 {
        w.bytes(&data);
    }
    for &(_, slot) in def.textures {
        let name = match slot {
            Slot::Null => None,
            Slot::Diffuse => Some(m.diffuse.as_str()),
            Slot::Bump => Some(m.bump.as_str()),
            Slot::Spec => Some(m.spec.as_str()),
            Slot::Spec2 => Some(BLANK_MB),
        };
        let t = name.filter(|n| !n.is_empty()).map(|n| embedded.get(n).copied().unwrap_or_else(|| add_texref(b, n)));
        w.ptr(t);
    }
    w.zeros(3 * 8 * nt);
    for &(_, s) in def.samplers {
        w.u8(s);
    }
    debug_assert_eq!(w.len(), size);
    b.set(pb, w);

    let mut pd = W::new();
    pd.u8(nb as u8).u8(nt as u8).u8(0).u8(ns as u8).u32((nt + ns + def.cbuffers().count()) as u32);
    for (i, &(h, _)) in def.textures.iter().enumerate() {
        pd.u32(h).u32((i as u32) << 2);
    }
    for (i, &(h, _)) in def.samplers.iter().enumerate() {
        pd.u32(h).u32(2 | (i as u32) << 2);
    }
    for (h, buf, off, vals) in def.cbuffers() {
        pd.u32(h).u32(3 | (buf as u32) << 2 | (off as u32) << 8 | ((4 * vals.len()) as u32) << 20);
    }
    let pdb = b.add(Seg::Sys, pd);

    let mut s = W::new();
    s.u32(def.hash).u32(SHADER_TAG);
    s.ptr(Some(pb)).ptr_at(Some(pb), tex_off as u64).u64(0).ptr(Some(pdb));
    s.u64(0).u64(0);
    s.u8(0).u8(m.bucket).u16(size as u16).u8(1 << m.bucket).u8(0xFF).u16(0);
    debug_assert_eq!(s.len(), 64);
    b.add(Seg::Sys, s)
}

fn aabb(p: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for v in p {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    (lo, hi)
}

fn buffer_srv(b: &mut Builder) -> BlockId {
    let mut w = W::new();
    ytd::write_srv(&mut w, SRV_BUFFER);
    b.add(Seg::Sys, w)
}

/// Geometry block with its vertex and index buffers.
fn add_geometry(b: &mut Builder, g: &Geom, pbr: bool) -> Result<BlockId, String> {
    let nv = g.positions.len();
    if nv == 0 || g.indices.is_empty() {
        return Err("empty geometry".into());
    }
    if nv > 0xFFFF {
        return Err(format!("geometry has {nv} vertices (max 65535)"));
    }
    if g.normals.len() != nv || g.colours.len() != nv || g.uvs.len() != nv {
        return Err("geometry attribute arrays differ in length".into());
    }
    if let Some(&i) = g.indices.iter().find(|&&i| i as usize >= nv) {
        return Err(format!("index {i} out of range ({nv} vertices)"));
    }
    // (slot, size, format) in ascending slot order: the SOA arrays follow it
    let mut elems: Vec<(usize, u8, u8)> = vec![(SLOT_POSITION, 12, FMT_FLOAT3), (SLOT_NORMAL, 4, FMT_DEC3N)];
    if pbr {
        elems.push((SLOT_TANGENT, 4, FMT_DEC3N));
    }
    elems.push((SLOT_COLOR0, 4, FMT_COLOUR));
    elems.push((SLOT_TEXCOORD0, 8, FMT_FLOAT2));
    if pbr {
        elems.push((SLOT_TEXCOORD1, 8, FMT_FLOAT2));
    }
    let stride: usize = elems.iter().map(|e| e.1 as usize).sum();
    let tangents = if pbr {
        let idx: Vec<u32> = g.indices.iter().map(|&i| i as u32).collect();
        uv_tangents(&g.positions, &g.normals, &g.uvs, &idx)
    } else {
        Vec::new()
    };
    let mut vd = Vec::with_capacity(nv * stride);
    let mut offsets = [0u32; 52];
    let mut sizes = [0u8; 52];
    let mut types = [0u8; 52];
    for &(slot, size, fmt) in &elems {
        offsets[slot] = vd.len() as u32;
        sizes[slot] = size;
        types[slot] = fmt;
        for i in 0..nv {
            match slot {
                SLOT_POSITION => g.positions[i].iter().for_each(|f| vd.extend_from_slice(&f.to_le_bytes())),
                SLOT_NORMAL => vd.extend_from_slice(&dec3n(g.normals[i], 0).to_le_bytes()),
                // w = sign of cross(N, T) . dP/dv (vanilla p_canteen01x: -1 throughout, measured)
                SLOT_TANGENT => {
                    let t = tangents[i];
                    vd.extend_from_slice(&dec3n([t[0], t[1], t[2]], if t[3] < 0.0 { -1 } else { 1 }).to_le_bytes())
                }
                SLOT_COLOR0 => vd.extend_from_slice(&g.colours[i].to_le_bytes()),
                SLOT_TEXCOORD0 => g.uvs[i].iter().for_each(|f| vd.extend_from_slice(&f.to_le_bytes())),
                // vanilla static `standard` meshes carry (1, 0) here
                _ => [1.0f32, 0.0].iter().for_each(|f| vd.extend_from_slice(&f.to_le_bytes())),
            }
        }
    }
    let vdata = b.sys_bytes(vd);
    let mut f = W::new();
    offsets.iter().for_each(|&o| {
        f.u32(o);
    });
    f.bytes(&sizes).bytes(&types);
    f.u32(1 | (stride as u32) << 2 | (nv as u32) << 10);
    debug_assert_eq!(f.len(), 316);
    let fmt = b.add(Seg::Sys, f);
    let vsrv = buffer_srv(b);
    let mut v = W::new();
    v.u64(VFT_VB).u32(nv as u32).u16(stride as u16).u16(0).u32(0x0020_9000).u32(0);
    v.ptr(Some(vdata)).u64(0).u64(0).ptr(Some(vsrv)).ptr(Some(fmt));
    let vb = b.add(Seg::Sys, v);

    let idata = b.sys_bytes(g.indices.iter().flat_map(|i| i.to_le_bytes()).collect());
    let isrv = buffer_srv(b);
    let mut x = W::new();
    x.u64(VFT_IB).u32(g.indices.len() as u32).u16(2).u16(0).u32(0x0020_A000).u32(0);
    x.ptr(Some(idata)).u64(0).u64(0).ptr(Some(isrv)).u64(0);
    let ib = b.add(Seg::Sys, x);

    let mut w = W::new();
    w.u64(VFT_GEOMETRY).ptr(Some(vb)).ptr(Some(ib)).u64(0).u64(0).u32(0).u16(0).u16(0).u32(0).u32(0).u64(0);
    debug_assert_eq!(w.len(), 64);
    Ok(b.add(Seg::Sys, w))
}

/// Pointer array block (u64 per target).
fn ptr_array(b: &mut Builder, ids: &[BlockId]) -> BlockId {
    let mut w = W::new();
    ids.iter().for_each(|&i| {
        w.ptr(Some(i));
    });
    b.add(Seg::Sys, w)
}

/// One model holding every geometry of a LOD level; `shader_of[i]` is the
/// shader index of geometry i. Returns the 16-byte LOD block.
fn add_lod(b: &mut Builder, geoms: &[&Geom], shader_of: &[u16], pbr: &[bool]) -> Result<BlockId, String> {
    let n = geoms.len();
    let gids: Vec<BlockId> = geoms.iter().zip(shader_of).map(|(g, &s)| add_geometry(b, g, pbr[s as usize])).collect::<Result<_, _>>()?;
    let garr = ptr_array(b, &gids);
    let boxes: Vec<_> = geoms.iter().map(|g| aabb(&g.positions)).collect();
    let mut bw = W::new();
    let mut put = |lo: [f32; 3], hi: [f32; 3]| {
        bw.v3(lo).f32(0.0).v3(hi).f32(0.0);
    };
    if n > 1 {
        let all: Vec<[f32; 3]> = boxes.iter().flat_map(|bx| [bx.0, bx.1]).collect();
        let (lo, hi) = aabb(&all);
        put(lo, hi);
    }
    boxes.iter().for_each(|&(lo, hi)| put(lo, hi));
    let bounds = b.add(Seg::Sys, bw);
    let mut sm = W::new();
    shader_of.iter().for_each(|&s| {
        sm.u16(s);
    });
    let maps = b.add(Seg::Sys, sm);
    let mut m = W::new();
    m.u64(VFT_MODEL).ptr(Some(garr)).u16(n as u16).u16(n as u16).u32(0);
    m.ptr(Some(bounds)).ptr(Some(maps)).u64(0);
    m.u8(0).u8(0).u8(0).u8(0).u8(0xFF).u8(0).u16(n as u16).u64(0);
    debug_assert_eq!(m.len(), 64);
    let model = b.add(Seg::Sys, m);
    let marr = ptr_array(b, &[model]);
    let mut l = W::new();
    l.ptr(Some(marr)).u16(1).u16(1).u32(0);
    Ok(b.add(Seg::Sys, l))
}

/// Drawable with up to four LOD levels (High, Med, Low, VLow); `levels[0]`
/// names it and sets the bounds; empty later levels are skipped. `embed`
/// textures go into a dictionary inside the drawable (shader group +8) and
/// the shaders point at them; otherwise shaders name textures of another
/// dictionary (the prop's .ytd).
pub fn build_ydr(levels: &[Mesh], dists: [f32; 4], embed: Option<&[ytd::Texture]>, o: &Opts) -> Result<Vec<u8>, String> {
    let mesh = levels.first().ok_or("no LOD levels")?;
    if mesh.geoms.is_empty() {
        return Err("mesh has no geometries".into());
    }
    let mut b = Builder::new();
    let root = b.reserve(Seg::Sys);
    let bm = b.sys_bytes(vec![0u8; 48]);
    b.pages_info = Some(bm);
    let name = b.string(&format!("{}.#dr", mesh.name.to_ascii_lowercase()));

    let (dict, embedded) = match embed {
        Some(t) if !t.is_empty() => {
            let d = b.reserve(Seg::Sys);
            let e = ytd::write_dict(&mut b, d, t, None)?;
            (Some(d), e.into_iter().collect::<HashMap<_, _>>())
        }
        _ => (None, HashMap::new()),
    };

    // shaders, deduplicated by material over all levels
    let mut mats: Vec<Mat> = Vec::new();
    let mut per_level: Vec<Vec<u16>> = Vec::new();
    for l in levels.iter().take(4) {
        per_level.push(
            l.geoms
                .iter()
                .map(|g| {
                    let m = mat_of(g, o);
                    let i = mats.iter().position(|x| *x == m).unwrap_or_else(|| {
                        mats.push(m);
                        mats.len() - 1
                    });
                    i as u16
                })
                .collect(),
        );
    }
    let shaders: Vec<BlockId> = mats.iter().map(|m| add_shader(&mut b, m, &embedded)).collect();
    let sarr = ptr_array(&mut b, &shaders);
    let mut sg = W::new();
    sg.u64(VFT_SHADERGROUP).ptr(dict).ptr(Some(sarr)).u16(shaders.len() as u16).u16(shaders.len() as u16).u32(0).zeros(32);
    let sgid = b.add(Seg::Sys, sg);
    let pbr: Vec<bool> = mats.iter().map(|m| m.shader.pbr()).collect();

    let mut lods: [Option<BlockId>; 4] = [None; 4];
    let mut masks = [0u32; 4];
    for (k, l) in levels.iter().enumerate().take(4) {
        if l.geoms.is_empty() {
            continue;
        }
        let gs: Vec<&Geom> = l.geoms.iter().collect();
        lods[k] = Some(add_lod(&mut b, &gs, &per_level[k], &pbr)?);
        let flags = per_level[k].iter().fold(0u32, |f, &s| f | 1 << mats[s as usize].bucket);
        masks[k] = 0xFF << 8 | flags;
    }

    let pts: Vec<[f32; 3]> = mesh.geoms.iter().flat_map(|g| g.positions.iter().copied()).collect();
    let (lo, hi) = aabb(&pts);
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
    let r = pts.iter().map(|p| (0..3).map(|k| (p[k] - c[k]).powi(2)).sum::<f32>()).fold(0.0f32, f32::max).sqrt();
    let mut w = W::new();
    w.u64(0).ptr(Some(bm)).ptr(Some(sgid)).u64(0);
    w.v3(c).f32(r).v3(lo).u32(0).v3(hi).u32(0);
    lods.iter().for_each(|&l| {
        w.ptr(l);
    });
    dists.iter().for_each(|&d| {
        w.f32(d);
    });
    masks.iter().for_each(|&m| {
        w.u32(m);
    });
    w.u64(0).u32(0).u16(0).u16(0).u64(0).ptr(Some(name)).u64(0).u64(0).u64(0).u64(0);
    debug_assert_eq!(w.len(), 0xD0);
    b.set(root, w);
    rsc8::build(b, rsc8::YDR_VERSION)
}
