//! Red Dead Redemption 2 / RedM output (RSC8): static props and textures.
//! `6ixpack redm ...`: loose .ytd / .ydr from DDS and OBJ, and one GTA VI
//! (6m) prop to a RedM resource.
//!
//! Format references: layouts (RSC8 container, ytd v2, ydr v162, ytyp v3)
//! worked out from vanilla RDR2 files and checked against files written
//! by CodeX (used only as a parsing oracle); shader catalogue facts from
//! Sollumz_RDR (GPL-3.0). No CodeX code is used.

pub mod interior;
pub mod manifest;
mod manifest_psch;
pub mod meta;
pub mod mlo;
pub mod meta_defs;
pub mod read;
pub mod ybn;
pub mod ymap;
pub mod rsc8;
pub mod ydr;
pub mod ytd;
pub mod ytyp;

use crate::gta5::{self, fivem};
use std::{fs, path::{Path, PathBuf}};

pub const USAGE: &str = "\
usage: 6ixpack redm <command> <args...>
  convert ytd <dds-dir> <out.ytd>                     DDS files -> RDR2 texture dictionary
  convert ydr <in.obj> <out.ydr> [--ytd <dds-dir>]    OBJ -> RDR2 drawable (`default` shader, `standard`
          [--lod <dist>]                              for materials with map_Bump; --ytd embeds those DDS)
  convert ytyp <in.json> <out.ytyp>                   archetype list -> RDR2 archetype file (see below)
  prop <name> --game <GTA VI dir> --out <resource>    one 6m drawable/fragment -> RedM resource:
          [--work <dir>] [--max-tex <px>] [--default-only]
          [--tint-row <n>] [--as <model>]             stream/<name>.ydr + .ytd + .ytyp, fxmanifest.lua
  prop <name> --tints --game <dir> [--work <dir>]     list the tint palette rows (writes nothing)
  prop --list <pattern> [--work <dir>]                6m drawable/fragment names containing <pattern>
  interior --list [pattern] --work <dir> [--game ..]  6m interiors (MLOs): rooms, portals, entities, collision
  interior <name> --game <dir> --work <dir>           one 6m interior -> RedM resource (in progress: its
          --out <resource> [--max-tex <px>]           entity archetypes as props, shared ytds, one ytyp,
          [--default-only]                            interior.json debug dump; `redm interior` for more)
  check <file.ydr|file.ytd|file.ytyp>                 re-read a file and print its structure

  --work: sixng work dir with index/levels.json (default $SIXNG_WORK or ./work;
          `6ixpack sixng index levels --game <dir> --work <dir>` makes it)
  --default-only: diffuse-only `default` shader everywhere (no `standard` with normal maps)
  --tint-row <n>: bake row n of the model's tint palette (rows of 256 colours, column = vertex
          colour blue; the game picks the row per placement). Default 0. `--tints` lists the rows
          and the colours of the columns the model uses; exact palette colours are baked.
  --as <model>: name of everything written (files, drawable, archetype, txd; a-z 0-9 _, < 64
          chars), so colour variants sit side by side: --tint-row 5 --as <name>_blue
  Texture names are taken as is from DDS stems / OBJ map_Kd stems (lower case).
  convert ytyp JSON: {\"name\": \"<ytyp name, default: file stem>\", \"archetypes\": [{
      \"name\": \"<model = .ydr stem>\", \"bb_min\": [x,y,z], \"bb_max\": [x,y,z],
      optional: \"texture_dictionary\" (default name), \"lod_dist\" (100), \"hd_texture_dist\" (5),
      \"bs_centre\" (box centre), \"bs_radius\" (half box diagonal), \"physics_dictionary\" (none)}]}
  Archetypes are static props (flags 0x30C20000, ASSET_TYPE_DRAWABLE, DUMMY_OBJECT); a prop
  resource lists the .ytyp as DLC_ITYP_REQUEST, so CreateObject(joaat('<name>'), ...) can spawn it.";

pub fn run(args: &[std::ffi::OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let rest = args.get(1..).unwrap_or(&[]);
    let r = match (args.first().map(String::as_str), rest.first().map(String::as_str)) {
        (Some("convert"), Some("ytd")) => convert_ytd(&rest[1..]),
        (Some("convert"), Some("ydr")) => convert_ydr(&rest[1..]),
        (Some("convert"), Some("ytyp")) => convert_ytyp(&rest[1..]),
        (Some("prop"), _) => prop(rest),
        (Some("interior"), _) => interior::run(rest),
        (Some("check"), Some(f)) => check(Path::new(f)),
        _ => {
            println!("{USAGE}");
            return Ok(());
        }
    };
    r.map_err(Into::into)
}

/// DDS files of a directory as textures (name = file stem, numeric
/// `NNNN_` prefix dropped as in `convert ytd`).
fn dds_dir(dir: &str) -> Result<Vec<ytd::Texture>, String> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("{dir}: {e}"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dds")))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("no .dds files in {dir}"));
    }
    files
        .iter()
        .map(|p| {
            let stem = p.file_stem().unwrap().to_string_lossy();
            let bytes = fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
            ytd::Texture::from_dds(&gta5::ytd::tex_name(&stem).to_ascii_lowercase(), &bytes)
        })
        .collect()
}

fn write(p: &Path, b: &[u8]) -> Result<(), String> {
    if let Some(d) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    fs::write(p, b).map_err(|e| format!("{}: {e}", p.display()))
}

fn convert_ytd(args: &[String]) -> Result<(), String> {
    let [dir, out] = args else { return Err("usage: redm convert ytd <dds-dir> <out.ytd>".into()) };
    let texs = dds_dir(dir)?;
    let b = ytd::build_ytd(&texs)?;
    let d = read::ytd(&b).map_err(|e| format!("self-check: {e}"))?;
    write(Path::new(out), &b)?;
    println!("{out}: {} textures, {} bytes", d.textures.len(), b.len());
    print_dict(&d);
    Ok(())
}

fn convert_ydr(args: &[String]) -> Result<(), String> {
    let (mut pos, mut embed, mut lod) = (Vec::new(), None, 9998.0f32);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--ytd" => embed = Some(it.next().ok_or("--ytd needs a DDS directory")?.clone()),
            "--lod" => lod = it.next().and_then(|s| s.parse().ok()).ok_or("--lod needs a number")?,
            _ => pos.push(a.clone()),
        }
    }
    let [src, dst] = &pos[..] else { return Err("usage: redm convert ydr <in.obj> <out.ydr> [--ytd <dds-dir>] [--lod <dist>]".into()) };
    let (src, dst) = (Path::new(src), Path::new(dst));
    let text = fs::read_to_string(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let mut mtl = std::collections::HashMap::new();
    let mut bumps = std::collections::HashMap::new();
    if let Some(lib) = text.lines().find_map(|l| l.trim().strip_prefix("mtllib ")) {
        if let Ok(t) = fs::read_to_string(src.parent().unwrap_or(Path::new(".")).join(lib.trim())) {
            mtl = gta5::ydr::parse_mtl(&t);
            bumps = mtl_bumps(&t, &mtl);
        }
    }
    let mut mesh = gta5::ydr::obj_to_mesh(&text, &mtl);
    // obj_to_mesh names textures `r6m_<stem>` for the fivem pipeline; RDR2
    // files here use the plain map_Kd stem (as the DDS stems of --ytd). A
    // map_Bump normal map switches the geometry to `standard`.
    for g in &mut mesh.geoms {
        if let Some(s) = g.texture.strip_prefix("r6m_").filter(|_| g.texture != "r6m_missing") {
            g.texture = s.to_string();
        }
        if let Some(n) = bumps.get(&g.texture) {
            g.normal = n.clone();
        }
    }
    mesh.name = dst.file_name().map(|s| s.to_string_lossy().split('.').next().unwrap_or("").to_string()).unwrap_or_default();
    let mut texs = embed.as_deref().map(dds_dir).transpose()?;
    // embedded dictionary: add the `standard` stand-ins (vanilla blank_ma /
    // blank_mb, as `redm prop`) unless the DDS directory has them
    if let Some(t) = texs.as_mut() {
        for n in ydr::texture_names(std::slice::from_ref(&mesh), &ydr::Opts::default()) {
            if !t.iter().any(|x| x.tex.name == n) {
                match n.as_str() {
                    ydr::BLANK_MA => t.push(ytd::blank_ma(&n)),
                    ydr::BLANK_MB => t.push(ytd::blank_mb(&n)),
                    _ => {}
                }
            }
        }
    }
    let b = ydr::build_ydr(&[mesh], [lod; 4], texs.as_deref(), &ydr::Opts::default())?;
    let info = read::ydr(&b).map_err(|e| format!("self-check: {e}"))?;
    write(dst, &b)?;
    println!("{}: {} bytes", dst.display(), b.len());
    print_ydr(&info);
    Ok(())
}

/// Diffuse texture name (as `convert ydr` names it: map_Kd stem, lower
/// case, "0x" removed) -> map_Bump / bump stem of the same material.
fn mtl_bumps(text: &str, kd: &std::collections::HashMap<String, String>) -> std::collections::HashMap<String, String> {
    let stem = |f: &str| Path::new(f).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut out = std::collections::HashMap::new();
    let mut cur: Option<&str> = None;
    for l in text.lines() {
        let q: Vec<&str> = l.split_whitespace().collect();
        match q.as_slice() {
            ["newmtl", m, ..] => cur = Some(m),
            [k, .., f] if k.eq_ignore_ascii_case("map_bump") || *k == "bump" => {
                if let Some(d) = cur.and_then(|m| kd.get(m)) {
                    out.insert(d.to_lowercase().replace("0x", ""), stem(f));
                }
            }
            _ => {}
        }
    }
    out
}

fn check(p: &Path) -> Result<(), String> {
    let b = fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "ytd" => print_dict(&read::ytd(&b)?),
        "ydr" => print_ydr(&read::ydr(&b)?),
        "ytyp" => print_ytyp(&read::ytyp(&b)?),
        "ymap" => print_ymap(&read::ymap(&b)?),
        "ybn" => print_ybn(&ybn::read(&b)?),
        "ymf" => print_manifest(&b, &manifest::read(&b)?)?,
        _ => return Err("check wants a .ytd, .ydr, .ytyp, .ymap, .ybn or _manifest.ymf".into()),
    }
    println!("{}: ok", p.display());
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonArch {
    name: String,
    bb_min: [f32; 3],
    bb_max: [f32; 3],
    texture_dictionary: Option<String>,
    lod_dist: Option<f32>,
    hd_texture_dist: Option<f32>,
    bs_centre: Option<[f32; 3]>,
    bs_radius: Option<f32>,
    physics_dictionary: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonYtyp {
    name: Option<String>,
    archetypes: Vec<JsonArch>,
}

fn convert_ytyp(args: &[String]) -> Result<(), String> {
    let [src, dst] = args else { return Err("usage: redm convert ytyp <in.json> <out.ytyp>".into()) };
    let text = fs::read_to_string(src).map_err(|e| format!("{src}: {e}"))?;
    let j: JsonYtyp = serde_json::from_str(&text).map_err(|e| format!("{src}: {e}"))?;
    let dst = Path::new(dst);
    let stem = dst.file_name().map(|s| s.to_string_lossy().split('.').next().unwrap_or("").to_string()).unwrap_or_default();
    let archs: Vec<ytyp::Archetype> = j
        .archetypes
        .into_iter()
        .map(|a| {
            let name = a.name.to_ascii_lowercase();
            let mut x = ytyp::Archetype::from_box(&name, a.lod_dist.unwrap_or(100.0), a.bb_min, a.bb_max);
            x.texture_dictionary = a.texture_dictionary.map(|t| t.to_ascii_lowercase()).unwrap_or(name);
            x.hd_texture_dist = a.hd_texture_dist.unwrap_or(x.hd_texture_dist);
            x.bs_centre = a.bs_centre.unwrap_or(x.bs_centre);
            x.bs_radius = a.bs_radius.unwrap_or(x.bs_radius);
            x.physics_dictionary = a.physics_dictionary.map(|p| p.to_ascii_lowercase()).filter(|p| !p.is_empty());
            x
        })
        .collect();
    let b = ytyp::build_ytyp(j.name.as_deref().unwrap_or(&stem), &archs)?;
    let info = read::ytyp(&b).map_err(|e| format!("self-check: {e}"))?;
    write(dst, &b)?;
    println!("{}: {} bytes", dst.display(), b.len());
    print_ytyp(&info);
    Ok(())
}

fn print_ytyp(i: &read::YtypInfo) {
    println!("  ytyp name 0x{:08X} (virtual {} B), root block {}, {} classes, {} enums, {} data blocks, {} meta pointers", i.name, i.vsize, i.root_block, i.classes.len(), i.enums.len(), i.blocks.len(), i.meta_ptrs);
    for (k, &(ty, len, off)) in i.blocks.iter().enumerate() {
        println!("  data block {}: type 0x{ty:08X}, {len} B at 0x{off:X}", k + 1);
    }
    for a in &i.archetypes {
        println!(
            "  archetype 0x{:08X} txd 0x{:08X} asset 0x{:08X} (type {}) physics 0x{:08X} flags 0x{:08X} entity {} lod {} hd {} bb ({:.3}, {:.3}, {:.3})..({:.3}, {:.3}, {:.3}) sphere ({:.3}, {:.3}, {:.3}) r {:.3}",
            a.name, a.texture_dictionary, a.asset_name, a.asset_type, a.physics_dictionary, a.flags, a.entity_type, a.lod_dist, a.hd_texture_dist,
            a.bb_min[0], a.bb_min[1], a.bb_min[2], a.bb_max[0], a.bb_max[1], a.bb_max[2], a.bs_centre[0], a.bs_centre[1], a.bs_centre[2], a.bs_radius
        );
    }
    i.mlos.iter().for_each(print_mlo);
}

fn print_mlo(m: &read::MloInfo) {
    println!("  mlo 0x{:08X} physicsDictionary 0x{:08X}: {} entities, {} rooms, {} portals, {} entity sets", m.name, m.physics_dictionary, m.entities, m.rooms.len(), m.portals.len(), m.sets.len());
    for (k, r) in m.rooms.iter().enumerate() {
        println!(
            "    room {k} {:?}: bb ({:.2}, {:.2}, {:.2})..({:.2}, {:.2}, {:.2}) flags {} portals {} entities {}",
            r.0, r.1[0], r.1[1], r.1[2], r.2[0], r.2[1], r.2[2], r.3, r.4, r.5.len()
        );
    }
    for (k, p) in m.portals.iter().enumerate() {
        println!("    portal {k}: room {} -> {} flags {} corner0 ({:.2}, {:.2}, {:.2})", p.0, p.1, p.2, p.3[0][0], p.3[0][1], p.3[0][2]);
    }
    for s in &m.sets {
        println!("    entity set 0x{:08X}: {} entities, locations {:?}", s.0, s.1, s.2);
    }
}

fn print_ymap(i: &read::YmapInfo) {
    println!(
        "  ymap 0x{:08X} (virtual {} B): flags {} contentFlags {}, {} classes, {} data blocks, {} meta pointers, physicsDictionaries {:08X?}",
        i.name, i.vsize, i.flags, i.content_flags, i.classes.len(), i.blocks, i.meta_ptrs, i.physics_dictionaries
    );
    println!("  extents ({:.1}, {:.1}, {:.1})..({:.1}, {:.1}, {:.1}), streaming ({:.1}, {:.1}, {:.1})..({:.1}, {:.1}, {:.1})", i.extents.0[0], i.extents.0[1], i.extents.0[2], i.extents.1[0], i.extents.1[1], i.extents.1[2], i.streaming.0[0], i.streaming.0[1], i.streaming.0[2], i.streaming.1[0], i.streaming.1[1], i.streaming.1[2]);
    for e in &i.entities {
        let kind = if e.num_exit_portals.is_some() { "CMloInstanceDef" } else { "CEntityDef" };
        println!(
            "  {kind} 0x{:08X} at ({:.3}, {:.3}, {:.3}) rotation ({:.6}, {:.6}, {:.6}, {:.6}) flags {} lodDist {}{}",
            e.archetype, e.position[0], e.position[1], e.position[2], e.rotation[0], e.rotation[1], e.rotation[2], e.rotation[3], e.flags, e.lod_dist,
            e.num_exit_portals.map_or(String::new(), |n| format!(" numExitPortals {n}"))
        );
    }
}

fn print_manifest(b: &[u8], m: &manifest::Manifest) -> Result<(), String> {
    let secs: Vec<String> = manifest::sections(b)?.iter().map(|s| format!("{} {} B", String::from_utf8_lossy(&s.0), s.1.len())).collect();
    println!("  manifest: {}{}", secs.join(", "), m.map_data_groups.as_ref().map_or(String::new(), |g| format!(", {} MapDataGroups", g.0)));
    let dep = |kind: &str, d: &manifest::Dep| println!("  {kind} 0x{:08X} flags {}{} -> {:08X?}", d.name, d.flags, if d.flags & manifest::INTERIOR_DATA != 0 { " (INTERIOR_DATA)" } else { "" }, d.deps);
    m.imaps.iter().for_each(|d| dep("imap", d));
    m.ityps.iter().for_each(|d| dep("ityp", d));
    for (n, bs) in &m.interiors {
        println!("  interior 0x{n:08X} bounds {bs:08X?}");
    }
    Ok(())
}

fn print_ybn(i: &ybn::YbnInfo) {
    println!("  ybn (virtual {} B): box ({:.3}, {:.3}, {:.3})..({:.3}, {:.3}, {:.3}), {} children", i.vsize, i.box_min[0], i.box_min[1], i.box_min[2], i.box_max[0], i.box_max[1], i.box_max[2], i.children.len());
    for (k, c) in i.children.iter().enumerate() {
        println!("  child {k}: type {} flags 0x{:X}/0x{:X}, {} vertices, {} polygons, {} BVH nodes, materials {:08X?}", c.0, c.1, c.2, c.3, c.4, c.6, c.5.iter().map(|m| m.0).collect::<Vec<_>>());
    }
}

fn print_dict(d: &read::DictInfo) {
    for t in &d.textures {
        println!("  texture {} {}x{} mips {} format 0x{:02X} blocks {} x {} B", t.name, t.width, t.height, t.mips, t.format, t.block_count, t.block_stride);
    }
}

fn print_ydr(i: &read::YdrInfo) {
    println!("  drawable {} (virtual {} B, physical {} B), radius {:.3}", i.name, i.vsize, i.psize, i.radius);
    for (k, s) in i.shaders.iter().enumerate() {
        let tx: Vec<&str> = s.textures.iter().flatten().map(String::as_str).collect();
        println!("  shader {k}: 0x{:08X} bucket {} params {} B, textures {:?}", s.hash, s.bucket, s.data_size, tx);
    }
    for g in &i.geoms {
        println!("  lod {} geometry: shader {}, {} vertices (stride {}), {} indices", g.lod, g.shader, g.vertices, g.stride, g.indices);
    }
    if let Some(d) = &i.dict {
        print_dict(d);
    }
}

/// Resource manifest: stream/ (.ydr, .ytd) is streamed automatically; the
/// .ytyp must also be listed as a DLC_ITYP_REQUEST data file, or a script
/// spawn by model name does not find the archetype.
fn fxmanifest(name: &str) -> String {
    format!(
        "fx_version 'cerulean'
game 'rdr3'
rdr3_warning 'I acknowledge that this is a prerelease build of RedM, and I am aware my resources *will* become incompatible once RedM ships.'

-- stream/{name}.ydr and {name}.ytd are streamed automatically; the archetype
-- file makes the model spawnable: CreateObject(joaat('{name}'), ...)
files {{ 'stream/{name}.ytyp' }}
data_file 'DLC_ITYP_REQUEST' 'stream/{name}.ytyp'
"
    )
}

/// Archetype lodDist (the distance the whole model stops drawing) from the
/// drawable that was written. If its last present level has a finite
/// switch distance, the drawable ends there anyway: use it. If the last
/// level is "never switch" (9998), keep it visible over a band: twice the
/// farthest finite switch, at least the clutter range (30 x radius, 40..100
/// m, as the fivem prop path); 100 m (CodeX default) without any switch.
fn archetype_lod(info: &read::YdrInfo) -> f32 {
    let finite = |d: f32| d.is_finite() && d > 0.0 && d < 9998.0;
    let Some(last) = info.geoms.iter().map(|g| g.lod).max() else { return 100.0 };
    if finite(info.lod_dist[last]) {
        return info.lod_dist[last];
    }
    let far = info.geoms.iter().map(|g| info.lod_dist[g.lod]).filter(|&d| finite(d)).fold(0.0f32, f32::max);
    if far == 0.0 {
        return 100.0;
    }
    let r = ytyp::half_diagonal(info.bbox.0, info.bbox.1);
    (2.0 * far).max((30.0 * r).clamp(40.0, 100.0))
}

/// Parsed `redm prop` arguments.
#[derive(Debug, Default)]
struct PropArgs {
    name: Option<String>,
    game: Option<PathBuf>,
    out: Option<PathBuf>,
    work: Option<PathBuf>,
    list: Option<String>,
    max_tex: Option<u16>,
    default_only: bool,
    /// palette row to bake (0 = default scheme)
    tint_row: usize,
    /// print the palette rows and exit
    tints: bool,
    /// output model name (file stems, drawable, archetype, txd)
    as_name: Option<String>,
}

fn parse_prop_args(args: &[String]) -> Result<PropArgs, String> {
    let mut p = PropArgs::default();
    let mut it = args.iter();
    let val = |it: &mut std::slice::Iter<String>, k: &str| it.next().cloned().ok_or(format!("{k} needs a value"));
    while let Some(a) = it.next() {
        match a.as_str() {
            "--game" => p.game = Some(PathBuf::from(val(&mut it, a)?)),
            "--out" => p.out = Some(PathBuf::from(val(&mut it, a)?)),
            "--work" => p.work = Some(val(&mut it, a)?.into()),
            "--list" => p.list = Some(val(&mut it, a)?.to_ascii_lowercase()),
            "--max-tex" => p.max_tex = Some(val(&mut it, a)?.parse::<u16>().map_err(|e| format!("--max-tex: {e}"))?),
            "--default-only" => p.default_only = true,
            "--tint-row" => p.tint_row = val(&mut it, a)?.parse::<usize>().map_err(|e| format!("--tint-row: {e}"))?,
            "--tints" => p.tints = true,
            "--as" => p.as_name = Some(model_name(&val(&mut it, a)?)?),
            x if !x.starts_with("--") && p.name.is_none() => p.name = Some(x.to_ascii_lowercase()),
            _ => return Err(format!("unknown argument {a}\n{USAGE}")),
        }
    }
    if p.tints && (p.tint_row > 0 || p.as_name.is_some()) {
        return Err("--tints only lists the palette rows; drop --tint-row / --as".into());
    }
    Ok(p)
}

/// A RedM model name for `--as`: lower case, 1..63 chars of [a-z0-9_]
/// (the characters `safe_name` keeps, so file stem, drawable name and
/// joaat(name) all agree).
fn model_name(s: &str) -> Result<String, String> {
    let n = s.to_ascii_lowercase();
    if !is_model_name(&n) {
        return Err(format!("--as {s}: model names are 1-63 characters of a-z, 0-9 and _"));
    }
    Ok(n)
}

/// 1..63 chars of [a-z0-9_] (lower case already).
fn is_model_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 63 && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `--tints` report: palette size, rows, and per row the colours of the
/// columns the model's LOD0 triangles use.
fn print_tints(name: &str, uses: &[fivem::PaletteUse]) {
    if uses.is_empty() {
        println!("{name}: no tint palette embedded (only row 0, untinted)");
        return;
    }
    for (k, u) in uses.iter().enumerate() {
        let rows = u.rows();
        let left = u.palette.len().saturating_sub(fivem::palette_index(rows, 0));
        let cols: Vec<String> = u.columns.iter().map(usize::to_string).collect();
        println!("{name}: palette {k}: {} entries, {rows} rows ({left} left over), LOD0 uses columns {}", u.palette.len(), cols.join(", "));
        for r in 0..rows {
            let c: Vec<String> = u.columns.iter().map(|&c| format!("{c}={}", u.colour(r, c).map_or("untinted".into(), |[r, g, b]| format!("{r:02x}{g:02x}{b:02x}")))).collect();
            println!("  row {r}: {}", c.join("  "));
        }
    }
}

/// `_ma` maps no larger than the diffuse they go with (largest diffuse of
/// the geometries binding that `_ma`; tinted copies share one).
fn clamp_ma_to_diffuse(levels: &[gta5::ydr::Mesh], texs: &mut [ytd::Texture]) {
    let dim = |n: &str| texs.iter().find(|t| t.tex.name == n).map(|t| t.tex.width.max(t.tex.height));
    let mut cap: Vec<(String, u16)> = Vec::new();
    for g in levels.iter().flat_map(|l| &l.geoms).filter(|g| !g.spec.is_empty()) {
        let (s, Some(d)) = (g.spec.to_ascii_lowercase(), dim(&g.texture.to_ascii_lowercase())) else { continue };
        match cap.iter_mut().find(|c| c.0 == s) {
            Some(c) => c.1 = c.1.max(d),
            None => cap.push((s, d)),
        }
    }
    for t in texs.iter_mut() {
        if let Some(c) = cap.iter().find(|c| c.0 == t.tex.name) {
            t.tex.limit_size(c.1);
        }
    }
}

/// The textures a converted 6m model's RDR2 drawable binds, as
/// `ytd::Texture`s: decoded ones (named as the geometries name them, lower
/// case; `max_tex` caps their size, `_ma` maps no larger than their
/// diffuse), then vanilla stand-ins for `standard`'s blank `_ma` / `_mb`
/// and a grey placeholder for anything that did not resolve.
fn prop_textures(src: &[(String, std::sync::Arc<Vec<u8>>)], levels: &[gta5::ydr::Mesh], opts: &ydr::Opts, max_tex: Option<u16>) -> Result<Vec<ytd::Texture>, String> {
    // textures the RDR2 drawable binds (FLAT_SPEC, GTA V's flat spec map, is not one)
    let wanted = ydr::texture_names(levels, opts);
    let mut texs = Vec::new();
    for (n, dds) in src {
        let n = n.to_ascii_lowercase();
        if !wanted.contains(&n) {
            continue;
        }
        let mut t = ytd::Texture::from_dds(&n, dds)?;
        if let Some(m) = max_tex {
            t.tex.limit_size(m);
        }
        texs.push(t);
    }
    clamp_ma_to_diffuse(levels, &mut texs);
    for n in &wanted {
        if texs.iter().any(|t| &t.tex.name == n) {
            continue;
        }
        texs.push(match n.as_str() {
            ydr::BLANK_MA => ytd::blank_ma(n),
            ydr::BLANK_MB => ytd::blank_mb(n),
            // r6m_missing and anything that did not decode
            _ => ytd::grey(n),
        });
    }
    Ok(texs)
}

/// Every texture a shader of `info` names must be in `dict` (`dict_name`
/// for the message).
fn check_shader_textures(name: &str, info: &read::YdrInfo, dict: &read::DictInfo, dict_name: &str) -> Result<(), String> {
    for s in &info.shaders {
        if let Some(t) = s.textures.iter().flatten().find(|t| !dict.textures.iter().any(|d| &d.name == *t)) {
            return Err(format!("{name}: shader texture {t} not in {dict_name}"));
        }
    }
    Ok(())
}

/// RedM requests joaat(<name>): the archetype name and assetName must be
/// the .ydr file stem, and the drawable carries "<name>.#dr".
fn check_drawable_name(name: &str, info: &read::YdrInfo) -> Result<(), String> {
    if info.name != format!("{name}.#dr") {
        return Err(format!("{name}.ydr: drawable named {}, expected {name}.#dr", info.name));
    }
    Ok(())
}

fn prop(args: &[String]) -> Result<(), String> {
    let p = parse_prop_args(args)?;
    let opts = ydr::Opts { standard: !p.default_only };
    let work = p.work.clone().unwrap_or_else(|| std::env::var_os("SIXNG_WORK").map(PathBuf::from).unwrap_or_else(|| "work".into()));
    if let Some(pat) = p.list {
        let ip = work.join("index").join("levels.json");
        let index = sixng_core::scan::load_index(&ip).map_err(|e| format!("{}: {e:#}", ip.display()))?;
        let mut names: Vec<(String, String)> = index
            .entries
            .iter()
            .filter(|e| matches!(e.rtype.as_deref(), Some("dr" | "ft")))
            .filter_map(|e| Some((e.name.clone()?, e.rtype.clone()?)))
            .filter(|(n, _)| n.to_ascii_lowercase().contains(&pat))
            .collect();
        names.sort();
        names.dedup();
        for (n, t) in &names {
            println!("{n}\t{t}");
        }
        eprintln!("{} names", names.len());
        return Ok(());
    }
    if p.tints {
        let (Some(src_name), Some(game)) = (p.name, p.game) else { return Err(USAGE.into()) };
        print_tints(&src_name, &fivem::prop_tints(&game, &work, &src_name)?);
        return Ok(());
    }
    let (Some(src_name), Some(game), Some(out)) = (p.name, p.game, p.out) else { return Err(USAGE.into()) };
    // the 6m model is looked up by `src_name`; everything written is `name`
    let name = p.as_name.unwrap_or_else(|| src_name.clone());
    let t0 = std::time::Instant::now();
    // a single prop keeps exact palette colours (`fivem build` rounds them
    // to 32 steps to limit tinted copies over a whole map)
    let src = fivem::prop_source(&game, &work, &src_name, fivem::TintPick { row: p.tint_row, exact: true })?;
    let mut levels = src.levels;
    for l in &mut levels {
        l.name = name.clone();
    }
    let texs = prop_textures(&src.textures, &levels, &opts, p.max_tex)?;
    let ydr_b = ydr::build_ydr(&levels, src.lod_dist, None, &opts).map_err(|e| format!("{name}.ydr: {e}"))?;
    let ytd_b = ytd::build_ytd(&texs).map_err(|e| format!("{name}.ytd: {e}"))?;
    let info = read::ydr(&ydr_b).map_err(|e| format!("{name}.ydr self-check: {e}"))?;
    let dict = read::ytd(&ytd_b).map_err(|e| format!("{name}.ytd self-check: {e}"))?;
    check_shader_textures(&name, &info, &dict, &format!("{name}.ytd"))?;
    check_drawable_name(&name, &info)?;
    let arch = ytyp::Archetype::from_box(&name, archetype_lod(&info), info.bbox.0, info.bbox.1);
    let ytyp_b = ytyp::build_ytyp(&name, &[arch]).map_err(|e| format!("{name}.ytyp: {e}"))?;
    let types = read::ytyp(&ytyp_b).map_err(|e| format!("{name}.ytyp self-check: {e}"))?;
    let h = gta5::rsc::joaat(&name);
    if types.archetypes.iter().any(|a| a.name != h || a.asset_name != h || a.texture_dictionary != h) {
        return Err(format!("{name}.ytyp: archetype hashes do not match joaat({name}) 0x{h:08X}"));
    }
    let stream = out.join("stream");
    write(&stream.join(format!("{name}.ydr")), &ydr_b)?;
    write(&stream.join(format!("{name}.ytd")), &ytd_b)?;
    write(&stream.join(format!("{name}.ytyp")), &ytyp_b)?;
    write(&out.join("fxmanifest.lua"), fxmanifest(&name).as_bytes())?;
    print_ydr(&info);
    print_dict(&dict);
    print_ytyp(&types);
    if !src.missing.is_empty() {
        eprintln!("redm: {name}: textures missing (grey placeholder): {}", src.missing.join("; "));
    }
    let tris: u32 = info.geoms.iter().filter(|g| g.lod == 0).map(|g| g.indices / 3).sum();
    let from = if src_name == name { String::new() } else { format!(" from {src_name}") };
    let row = if p.tint_row > 0 { format!(", tint row {}", p.tint_row) } else { String::new() };
    println!("redm: {name}{from} ({}{row}) -> {}: {} LOD levels, {tris} triangles, {} shaders, {} textures, {:.1}s", src.rtype, out.display(), levels.iter().filter(|l| !l.geoms.is_empty()).count(), info.shaders.len(), dict.textures.len(), t0.elapsed().as_secs_f32());
    Ok(())
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_mlo;
