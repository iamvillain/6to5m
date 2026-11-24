//! `6ixpack fivem build`: GTA VI (6m) world data -> FiveM map resource, in
//! pure Rust. Port of the 6m-port Python pipeline (terrain_port.py,
//! buildings_port.py, water_port.py, obj2ydrxml.py, ytdpack) onto the GTA V
//! writers in this crate.
//!
//! Output: <out>/stream/*.{ydr,ybn,ytd,ytyp,ymap}, <out>/fxmanifest.lua,
//! <out>/water.xml; incremental state in <work>/index (fingerprint manifest,
//! source-texture records: textures are decoded in memory, fivem/texmem.rs).
//!
//! Fragments are written as static ydr (main drawable, bind pose, no
//! breakage/physics). Interior (MLO) entities are moved into world space as
//! plain entities (no MLO ytyp, rooms or portals). LOD chains come from
//! `sixng lod-links`: HD cells -> t6m_lod1 (LOD) -> t6m_lod2 (SLOD1) ...
//!
//! TODO (not done here):
//! - Normal/spec shaders,
//!   skinned models (skipped and reported), prop-batch LOD parents.
//! - Rotation: placements store the inverse entity rotation, which is what
//!   CEntityDef stores too, so it is copied as is (like buildings_port.py).
//!   Not verified in game; `--conjugate-rot` writes the conjugate instead.

mod batches;
mod buildings;
pub mod ped;
mod collision;
mod geom;
mod lsremove;
mod minimap;
mod nav;
mod regionlog;
pub mod split;
mod terrain;
mod vehicles;
mod weapons;
mod frags;
mod vlog;
pub mod progress;
mod texmem;
mod texslot;
mod quality;
pub mod verify;
mod wardrobe;

use memmap2::Mmap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    error::Error,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

pub const USAGE: &str = "\
usage: 6ixpack fivem build --game <dir> --out <resource dir> [options]
  --game <dir>         GTA VI install (levels.rpf, data/ttdb)
  --out <dir>          FiveM resource directory (stream/, fxmanifest.lua, water.xml),
                       or a [name] category (e.g. server-data/resources/[t6m]):
                       resources are written there directly, no build dir
  --bbox x0,y0,x1,y1   world region (metres); default: everything
  --offset dx,dy[,dz]  shift every output by this (metres; --bbox stays in 6m
                       coordinates). GTA VI reaches y 9474, past GTA V's ~8000
                       limit: e.g. --offset 0,-1500 for the north
  --name <substr>      only building archetypes whose name contains this
  --only <list>        terrain,buildings,collision,water,lsremove,paths,navmesh (default: all);
                       batches (instanced prop/grass batches, opt-in)
                       lsremove: empty stand-ins for vanilla _citye/_cityw/_hills
                       ymaps/ybns in the region, read from GTA V ($GTA5_DIR or Steam)
  --report <file>      JSON report: converted / skipped with reason / outputs
  --work <dir>         sixng work dir with index/levels.json (default $SIXNG_WORK or ./work)
  --placements <dir>   sixng placement JSON dir with index.jsonl, or its parent
                       (default <work>/placements/levels; generated when missing)
  Source textures: decoded in memory; records in <work>/index/fivem_textures_v*.json.
  --max-tex <px>       drop top mips so building textures are at most this size (e.g. 512, 1024)
  --interiors         also place interior (MLO) contents in world space; off by
                       default because the MLO shell itself is not converted,
                       so contents of multi-floor interiors float
  --no-lod            HD entities only, all ORPHANHD (no LOD/SLOD chain)
  --weapon-collision  keep weapon-only collision shapes (bullets); off by default (~20% of triangles)
  --conjugate-rot      write the conjugate of the stored placement quaternion
  --force              rebuild outputs even when up to date
  --split <metres>     also write <out>/[<name>]/: one resource per square region
                       (rounded to 512 m cells) plus <name>_common (ytyps, LOD/SLOD
                       ymaps, shared txds, terrain, water); see ensure.cfg there
  --split-max <MB>     quarter any region resource bigger than this (default 1024)
  --verify             re-parse every new output (resource layout checks); off by default
  --bake               whole-map pass (no --bbox) that only fills the output cache
                       <work>/fivem_out_cache; --out defaults to <work>/fivem_bake.
                       Later regional builds then hardlink from the cache
also: 6ixpack fivem vehicle <name> --game <dir> --out <dir>   (one 6m vehicle -> FiveM vehicle resource; --help)
      6ixpack fivem weapon <name|all> --game <dir> --out <dir>  (6m weapons -> FiveM weapon resource; --help)";

/// Per-stage output versions: bump only the one whose writer changed, so
/// other outputs stay cached (each goes into its own outputs' fingerprints).
pub const TEX_V: &str = "tex7";
pub const YDR_V: &str = "ydr22";
/// Fragment props as dynamic .yft (fivem/frags.rs).
pub const FRAG_V: &str = "frag5";
pub const YBN_V: &str = "ybn5";
pub const TERRAIN_V: &str = "terrain2";
/// Meta files (ymap/ytyp) and water.xml are rebuilt every run and compared
/// byte for byte, so they need no version. MANIFEST_V is the manifest layout.
const MANIFEST_V: &str = "fivem-m2";

pub struct Opts {
    pub game: PathBuf,
    pub out: PathBuf,
    pub bbox: Option<[f32; 4]>,
    pub name: Option<String>,
    pub only: Vec<String>,
    pub report: Option<PathBuf>,
    pub work: PathBuf,
    pub placements: Option<PathBuf>,
    pub conjugate: bool,
    pub interiors: bool,
    pub no_lod: bool,
    pub weapon_collision: bool,
    pub force: bool,
    pub max_tex: Option<u16>,
    pub split: Option<f32>,
    pub split_max: u64,
    pub bake: bool,
    pub verify: bool,
    pub offset: [f32; 3],
}

impl Opts {
    /// `v` moved by --offset (world-space geometry: collision, terrain ybn).
    pub fn shift(&self, v: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|k| v[k] + self.offset[k])
    }

    /// Cache-key part for world-space outputs ("" without --offset, so
    /// existing caches stay valid).
    pub fn offset_fp(&self) -> String {
        if self.offset == [0.0; 3] { String::new() } else { format!("|off{:?}", self.offset) }
    }

    pub fn in_bbox(&self, lo: [f32; 3], hi: [f32; 3]) -> bool {
        self.bbox.is_none_or(|b| hi[0] >= b[0] && lo[0] <= b[2] && hi[1] >= b[1] && lo[1] <= b[3])
    }
}

#[derive(Serialize, Deserialize, Default)]
struct Manifest {
    tool: String,
    files: BTreeMap<String, MEntry>,
}

#[derive(Serialize, Deserialize, Clone)]
struct MEntry {
    fp: String,
    #[serde(default)]
    info: Value,
    /// verify summary, so up-to-date files are not re-read
    #[serde(default)]
    summary: String,
}

#[derive(Default, Serialize)]
pub struct Report {
    pub converted: Vec<Value>,
    pub skipped: Vec<Value>,
    pub skipped_counts: BTreeMap<String, usize>,
    pub warnings: Vec<Value>,
    pub outputs: Vec<Value>,
    pub timings: Vec<Value>,
    /// per extension: [from stream (kept), from output cache, converted]
    pub cache: BTreeMap<String, [usize; 3]>,
    pub diagnostics: Value,
}

/// Map archives placed besides levels.rpf (6m story and minigame packs).
pub const PACK_ARCHIVES: &[&str] = &["pack/story000/story000.rpf", "pack/shared_minigame000/shared_minigame000.rpf"];
/// Bit where a virtual offset's archive slot starts (archives < 16 TB).
pub const PACK_SHIFT: u32 = 44;

/// One extra placement archive: id, file, memory map and sixng index.
pub struct Pack {
    pub id: String,
    pub path: PathBuf,
    pub map: Mmap,
    pub index: sixng_core::scan::ArchiveIndex,
}

/// Shared build state.
pub struct Ctx {
    pub o: Opts,
    pub stream: PathBuf,
    /// the `[name]` category dir when --out is one (resources written in place)
    pub root: Option<PathBuf>,
    pub levels: Mmap,
    /// Other placement archives (PACK_ARCHIVES that are present and
    /// indexed). Model offsets into them are "virtual": the 1-based slot in
    /// the bits from PACK_SHIFT up, so one usize names archive and offset.
    pub packs: Vec<Pack>,
    /// Newest input mtime: outputs older than this are rebuilt.
    pub input_time: SystemTime,
    manifest: Mutex<Manifest>,
    pub report: Mutex<Report>,
    pub layout: Mutex<split::Layout>,
    /// stream files emitted this run (for pruning stale outputs)
    emitted: Mutex<std::collections::HashSet<String>>,
    /// fingerprint manifest (<work>/index/fivem_manifest.json): the
    /// deployed stream files are the only copy of the converted bytes
    manifest_path: PathBuf,
    /// levels.rpf size and mtime, part of every cache key
    pub input_stamp: String,
}

impl Ctx {
    /// Archive bytes and real offset of a virtual model offset.
    pub fn src_map(&self, voff: usize) -> (&[u8], usize) {
        match voff >> PACK_SHIFT {
            0 => (&self.levels[..], voff),
            s => (&self.packs[s - 1].map[..], voff & ((1usize << PACK_SHIFT) - 1)),
        }
    }

    /// sixng model id ("<archive>@<hex>") of a virtual offset.
    pub fn src_id(&self, voff: usize) -> String {
        let off = voff & ((1usize << PACK_SHIFT) - 1);
        match voff >> PACK_SHIFT {
            0 => format!("levels.rpf@{off:x}"),
            s => format!("{}@{off:x}", self.packs[s - 1].id),
        }
    }

    /// Virtual offset of a sixng model id ("<archive>@<hex>[#member]");
    /// None for an archive that is not loaded.
    pub fn voff_of(&self, model_id: &str) -> Option<usize> {
        let (a, h) = model_id.rsplit_once('@')?;
        let off = usize::from_str_radix(h.split('#').next()?, 16).ok()?;
        let slot = if a == "levels.rpf" { 0 } else { self.packs.iter().position(|p| p.id == a)? + 1 };
        Some(off | (slot << PACK_SHIFT))
    }

    /// Archive file of an index entry (entry ids are "<archive>@<hex>").
    pub fn entry_path(&self, e: &sixng_core::scan::Entry) -> PathBuf {
        let a = e.id.rsplit_once('@').map_or("levels.rpf", |x| x.0);
        self.packs.iter().find(|p| p.id == a).map_or_else(|| self.o.game.join("levels.rpf"), |p| p.path.clone())
    }

    fn cache_key(&self, fp: &str) -> String {
        sha(&format!("{fp}|{}", self.input_stamp))
    }

    /// Manifest info for `file` when stream/ already holds it from the same
    /// fingerprint and inputs. None = convert it (there is no second copy of
    /// converted bytes: the deployed stream file is the cache).
    pub fn up_to_date(&self, file: &str, fp: &str) -> Option<Value> {
        if self.o.force {
            return None;
        }
        let key = self.cache_key(fp);
        if let Some(e) = self.manifest.lock().unwrap().files.get(file).cloned() {
            if e.fp == key && self.stream.join(file).is_file() {
                self.count(file, 0);
                return Some(e.info);
            }
        }
        None
    }

    fn count(&self, file: &str, k: usize) {
        let ext = file.rsplit_once('.').map_or("", |x| x.1).to_string();
        self.report.lock().unwrap().cache.entry(ext).or_default()[k] += 1;
    }

    /// Write (or keep) a stream file. New bytes are verified and written to
    /// stream/ unless it already holds the same bytes (left untouched: mtime
    /// and FXServer's hash stay valid).
    /// `info` goes to the manifest for later runs. Returns the verify summary.
    pub fn emit(&self, file: &str, fp: &str, info: Value, bytes: Option<Vec<u8>>) -> Result<String, String> {
        let path = self.stream.join(file);
        let meta = fp == "meta";
        let key = if meta { "meta".to_string() } else { self.cache_key(fp) };
        let (status, summary, len) = match bytes {
            Some(b) => {
                if file.len() > 63 + 5 {
                    return Err(format!("{file}: name too long"));
                }
                let summary = if self.o.verify { verify::verify(file, &b)? } else { format!("{} bytes", b.len()) };
                let same = fs::metadata(&path).is_ok_and(|m| m.len() == b.len() as u64) && fs::read(&path).is_ok_and(|x| x == b);
                if !meta {
                    self.count(file, 2);
                }
                if !same {
                    write_atomic(&path, &b).map_err(|e| format!("{}: {e}", path.display()))?;
                }
                (if same { "unchanged" } else { "written" }, summary, b.len() as u64)
            }
            None => {
                let known = self.manifest.lock().unwrap().files.get(file).filter(|e| e.fp == key && !e.summary.is_empty()).map(|e| e.summary.clone());
                let len = fs::metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?.len();
                let summary = match known {
                    Some(s) => s,
                    None if self.o.verify => verify::verify(file, &fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?)?,
                    None => format!("{len} bytes"),
                };
                ("up-to-date", summary, len)
            }
        };
        self.emitted.lock().unwrap().insert(file.to_string());
        self.manifest.lock().unwrap().files.insert(file.to_string(), MEntry { fp: key, info, summary: summary.clone() });
        self.report.lock().unwrap().outputs.push(json!({"file": file, "status": status, "bytes": len, "verify": summary}));
        Ok(summary)
    }

    pub fn skip(&self, name: &str, reason: &str, extra: Value) {
        let mut r = self.report.lock().unwrap();
        *r.skipped_counts.entry(reason.to_string()).or_default() += 1;
        let mut v = json!({"name": name, "reason": reason});
        if let (Some(m), Value::Object(x)) = (v.as_object_mut(), extra) {
            m.extend(x);
        }
        r.skipped.push(v);
    }

    fn save_manifest(&self) -> Result<(), String> {
        let m = self.manifest.lock().unwrap();
        let p = &self.manifest_path;
        fs::write(p, serde_json::to_vec(&*m).unwrap()).map_err(|e| format!("{}: {e}", p.display()))
    }
}

/// Build state for converting single assets outside `fivem build` (e.g.
/// `redm prop`): levels.rpf mapped, no pack archives, empty manifest and
/// report; nothing is emitted through it.
fn single_ctx(game: &Path, work: &Path) -> Result<Ctx, String> {
    let s = |p: &Path| p.to_string_lossy().into_owned();
    let o = parse(&["--game".into(), s(game), "--out".into(), s(&work.join("single")), "--work".into(), s(work)])?;
    let lp = game.join("levels.rpf");
    let lf = fs::File::open(&lp).map_err(|e| format!("{}: {e}", lp.display()))?;
    let input_time = lf.metadata().and_then(|m| m.modified()).map_err(|e| e.to_string())?;
    // SAFETY: read-only map; nothing writes the archive while we run.
    let levels = unsafe { Mmap::map(&lf) }.map_err(|e| e.to_string())?;
    let stream = o.out.join("stream");
    let manifest_path = work.join("index").join("single_manifest.json");
    Ok(Ctx { o, stream, root: None, levels, packs: Vec::new(), input_time, manifest: Default::default(), report: Default::default(), layout: Default::default(), emitted: Default::default(), manifest_path, input_stamp: String::new() })
}

pub(crate) use buildings::{palette_index, prop_source, prop_tints, PaletteUse, TintPick};
#[cfg(test)]
pub(crate) use buildings::{check_tint_row, palette_rows};

/// `r6m_<name>` when it fits in 63 chars, else `r6m_<joaat hex>`.
pub fn safe_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c.to_ascii_lowercase() } else { '_' }).collect();
    let n = format!("r6m_{clean}");
    if n.len() < 64 { n } else { format!("r6m_{:08x}", super::rsc::joaat(name)) }
}

pub fn sha(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(s.as_bytes());
    d.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts {
        game: PathBuf::new(),
        out: PathBuf::new(),
        bbox: None,
        name: None,
        only: vec!["terrain".into(), "buildings".into(), "collision".into(), "water".into(), "lsremove".into(), "paths".into(), "navmesh".into()],
        report: None,
        work: std::env::var_os("SIXNG_WORK").map(PathBuf::from).unwrap_or_else(|| "work".into()),
        placements: None,
        conjugate: false,
        interiors: false,
        no_lod: false,
        weapon_collision: false,
        force: false,
        max_tex: None,
        split: None,
        split_max: 1024,
        bake: false,
        verify: false,
        offset: [0.0; 3],
    };
    let mut it = args.iter();
    let val = |it: &mut std::slice::Iter<String>, k: &str| it.next().cloned().ok_or(format!("{k} needs a value"));
    while let Some(a) = it.next() {
        match a.as_str() {
            "--game" => o.game = val(&mut it, a)?.into(),
            "--out" => o.out = val(&mut it, a)?.into(),
            "--work" => o.work = val(&mut it, a)?.into(),
            "--placements" => o.placements = Some(val(&mut it, a)?.into()),
            "--report" => o.report = Some(val(&mut it, a)?.into()),
            "--name" => o.name = Some(val(&mut it, a)?.to_lowercase()),
            "--only" => o.only = val(&mut it, a)?.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect(),
            "--bbox" => {
                let v: Vec<f32> = val(&mut it, a)?.split(',').map(|s| s.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|e| format!("--bbox: {e}"))?;
                let [a, b, c, d] = v[..] else { return Err("--bbox needs minx,miny,maxx,maxy".into()) };
                o.bbox = Some([a.min(c), b.min(d), a.max(c), b.max(d)]);
            }
            "--offset" => {
                let v: Vec<f32> = val(&mut it, a)?.split(',').map(|s| s.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|e| format!("--offset: {e}"))?;
                o.offset = match v[..] {
                    [x, y] => [x, y, 0.0],
                    [x, y, z] => [x, y, z],
                    _ => return Err("--offset needs dx,dy[,dz]".into()),
                };
            }
            "--conjugate-rot" => o.conjugate = true,
            "--interiors" => o.interiors = true,
            "--no-lod" => o.no_lod = true,
            "--weapon-collision" => o.weapon_collision = true,
            "--force" => o.force = true,
            "--max-tex" => o.max_tex = Some(val(&mut it, a)?.parse().map_err(|e| format!("--max-tex: {e}"))?),
            "--split" => o.split = Some(val(&mut it, a)?.parse().map_err(|e| format!("--split: {e}"))?),
            "--bake" => o.bake = true,
            "--verify" => o.verify = true,
            "--split-max" => o.split_max = val(&mut it, a)?.parse().map_err(|e| format!("--split-max: {e}"))?,
            _ => return Err(format!("unknown argument {a}\n{USAGE}")),
        }
    }
    if o.bake {
        if o.bbox.is_some() || o.name.is_some() {
            return Err("--bake is a whole-map pass: drop --bbox / --name".into());
        }
        if o.out.as_os_str().is_empty() {
            o.out = o.work.join("fivem_bake");
        }
        o.split = None;
    }
    if o.game.as_os_str().is_empty() || o.out.as_os_str().is_empty() {
        return Err(USAGE.into());
    }
    if let Some(p) = o.placements.take() {
        o.placements = Some(resolve_placements(&p)?);
    }
    if let Some(k) = o.only.iter().find(|k| !["terrain", "buildings", "collision", "water", "lsremove", "paths", "navmesh", "batches"].contains(&k.as_str())) {
        return Err(format!("--only: unknown part {k}"));
    }
    Ok(o)
}

/// A placement dir must hold index.jsonl; accept its parent when exactly one
/// subfolder (or `levels`) has one.
fn resolve_placements(p: &std::path::Path) -> Result<PathBuf, String> {
    if p.join("index.jsonl").is_file() {
        return Ok(p.to_path_buf());
    }
    if p.join("levels").join("index.jsonl").is_file() {
        return Ok(p.join("levels"));
    }
    let subs: Vec<PathBuf> = fs::read_dir(p)
        .map_err(|e| format!("--placements {}: {e}", p.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|d| d.join("index.jsonl").is_file())
        .collect();
    match subs.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(format!("--placements {}: no index.jsonl here or in a subfolder", p.display())),
        many => Err(format!("--placements {}: several subfolders have index.jsonl ({}); pick one", p.display(), many.iter().filter_map(|d| d.file_name()).map(|n| n.to_string_lossy()).collect::<Vec<_>>().join(", "))),
    }
}

pub fn run(args: &[OsString]) -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    match args.first().map(String::as_str) {
        Some("build") => {}
        Some("ped") => return ped::run(&args[1..]),
        Some("vehicle") => return vehicles::run(&args[1..]),
        Some("gta5") => return lsremove::gta5_cli(&args[1..]),
        Some("weapon") => return weapons::run(&args[1..]),
        _ => {
            println!("{USAGE}");
            return Ok(());
        }
    }
    let mut o = parse(&args[1..])?;
    crate::gta5::meta::set_world_offset(o.offset);
    // derived work data (placements, collision, LOD / interior tables,
    // batches, side tables) lives in memory: nothing extracted on disk
    sixng_core::vfs::set_memory(&o.work);
    let t0 = std::time::Instant::now();
    // `--out <dir>/[name]` (a FiveM resource category, e.g.
    // server-data/resources/[t6m]): resources are written straight into it,
    // no build dir. The main resource (`name`, or `name_common` with
    // --split) takes the place of <out>; the deployed files are the cache.
    let root = split::category(&o.out).map(|_| o.out.clone());
    if let Some(r) = &root {
        let base = split::category(r).unwrap();
        o.out = r.join(if o.split.is_some() { format!("{base}_common") } else { base });
    }
    let stream = o.out.join("stream");
    fs::create_dir_all(&stream)?;
    if let Some(r) = &root {
        split::gather(r, &stream)?;
    }
    let lp = o.game.join("levels.rpf");
    let lf = fs::File::open(&lp).map_err(|e| format!("{}: {e}", lp.display()))?;
    let input_time = lf.metadata()?.modified()?;
    // SAFETY: read-only map; nothing writes the archive while we run.
    let levels = unsafe { Mmap::map(&lf) }?;
    // fingerprint manifest lives with the index; the old <out> copy is read
    // once so existing stream files stay up to date
    let manifest_path = o.work.join("index").join("fivem_manifest.json");
    let mut manifest: Manifest = [manifest_path.clone(), o.out.join(".fivem-manifest.json")].iter().find_map(|p| fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok())).unwrap_or_default();
    if manifest.tool != MANIFEST_V {
        // older manifest layout; outputs come back from the cache
        manifest = Manifest { tool: MANIFEST_V.into(), ..Default::default() };
    }
    let layout = split::load(&o.out);
    fs::create_dir_all(o.work.join("index"))?;
    let input_stamp = format!("{}:{}", lf.metadata()?.len(), input_time.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
    // story / minigame packs: placed only when indexed (`sixng index <id>`)
    let packs: Vec<Pack> = if o.only.iter().any(|x| x == "buildings") {
        PACK_ARCHIVES
            .iter()
            .filter_map(|id| {
                let path = o.game.join(id);
                if !path.is_file() {
                    return None;
                }
                let ip = sixng_core::scan::index_path(&o.work.join("index"), id);
                let Ok(index) = sixng_core::scan::load_index(&ip) else {
                    eprintln!("{id}: not indexed ({}), its placements are skipped", ip.display());
                    return None;
                };
                // SAFETY: read-only map; nothing writes the archive while we run.
                let map = fs::File::open(&path).ok().and_then(|f| unsafe { Mmap::map(&f) }.ok())?;
                Some(Pack { id: id.to_string(), path, map, index })
            })
            .collect()
    } else {
        Vec::new()
    };
    let ctx = Ctx { o, stream, root, levels, packs, input_time, manifest: Mutex::new(manifest), report: Mutex::new(Report::default()), layout: Mutex::new(layout), emitted: Default::default(), manifest_path, input_stamp };
    let part = |p: &str| ctx.o.only.iter().any(|x| x == p);

    let result = (|| -> Result<(), Box<dyn Error>> {
        let needs_index = part("terrain") || part("buildings") || part("collision") || part("paths");
        let _p = Phase::new(&ctx, "index load");
        let index = if needs_index {
            let ip = ctx.o.work.join("index").join("levels.json");
            eprintln!("loading {}", ip.display());
            Some(sixng_core::scan::load_index(&ip).map_err(|e| format!("{}: {e:#} (run `6ixpack sixng index levels.rpf --work ...` first)", ip.display()))?)
        } else {
            None
        };
        drop(_p);
        // buildings first: it records the flat ground tiles (asphalt, pads)
        // that terrain must sink below (<work>/fivem_ground.json)
        if part("buildings") {
            let _p = Phase::new(&ctx, "buildings (total)");
            buildings::build(&ctx, index.as_ref().unwrap())?;
            let done: std::collections::HashSet<String> = ctx.report.lock().unwrap().converted.iter().filter_map(|v| v["name"].as_str().map(str::to_string)).collect();
            batches::grass(&ctx, index.as_ref().unwrap(), &done)?;
            ctx.save_manifest()?;
            split::save(&ctx)?;
            minimap::zones(&ctx)?;
            check_gtxd(&ctx);
        }
        if part("terrain") {
            let _p = Phase::new(&ctx, "terrain");
            terrain::build(&ctx, index.as_ref().unwrap())?;
            ctx.save_manifest()?;
        }
        if part("collision") {
            let _p = Phase::new(&ctx, "collision");
            collision::build(&ctx, index.as_ref().unwrap())?;
            ctx.save_manifest()?;
        }
        if part("water") {
            let _p = Phase::new(&ctx, "water");
            water(&ctx)?;
        }
        if part("lsremove") {
            let _p = Phase::new(&ctx, "lsremove");
            lsremove::build(&ctx)?;
            ctx.save_manifest()?;
        }
        if part("paths") {
            let _p = Phase::new(&ctx, "paths");
            nav::build_paths(&ctx, index.as_ref().unwrap())?;
            ctx.save_manifest()?;
        }
        if part("navmesh") {
            let _p = Phase::new(&ctx, "navmesh");
            nav::build_navmesh(&ctx)?;
            ctx.save_manifest()?;
        }
        if ["terrain", "buildings", "collision", "water", "lsremove", "paths", "navmesh"].iter().all(|p| part(p)) && ctx.o.name.is_none() {
            prune(&ctx)?;
        }
        fxmanifest(&ctx)?;
        if ctx.o.split.is_none() {
            let r = ctx.report.lock().unwrap();
            let ch = r.outputs.iter().any(|v| v["status"] == "written" || (v["status"] == "pruned" && v["files"].as_u64().unwrap_or(0) > 0));
            drop(r);
            let name = ctx.o.out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            live_report(&ctx, &if ch { vec![name] } else { vec![] });
        }
        if let Some(m) = ctx.o.split {
            let _p = Phase::new(&ctx, "split");
            split::split(&ctx, m, ctx.o.split_max)?;
        }
        Ok(())
    })();
    ctx.save_manifest()?;
    let over = diagnostics(&ctx, t0.elapsed().as_secs_f64());
    quality::finish(&ctx.o.out);
    regionlog::finish(&ctx.o.out, &ctx.stream);
    let r = ctx.report.lock().unwrap();
    let written = r.outputs.iter().filter(|v| v["status"] == "written").count();
    println!(
        "fivem: {} outputs ({} written, {} up to date), {} converted, {} skipped {:?}, {:.1}s -> {}",
        r.outputs.len(),
        written,
        r.outputs.len() - written,
        r.converted.len(),
        r.skipped.len(),
        r.skipped_counts,
        t0.elapsed().as_secs_f32(),
        ctx.o.out.display()
    );
    vlog::flush(&ctx.o.out);
    let moved = texslot::MOVED.load(std::sync::atomic::Ordering::Relaxed);
    if moved > 0 {
        println!("fivem: {moved} texture lookups moved off a 32-bit hash slot owned by another texture");
    }
    if let Some(p) = &ctx.o.report {
        fs::write(p, serde_json::to_vec_pretty(&*r)?).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    result?;
    // FiveM evicts streamed files over 16 MB in memory (blurry fallbacks):
    // a build that writes one is broken, not merely warned about.
    if !over.is_empty() {
        return Err(format!("{} streamed files over 16 MB (RSC7 page flags): {}", over.len(), over.iter().take(10).cloned().collect::<Vec<_>>().join(", ")).into());
    }
    Ok(())
}

/// (virtual, physical) bytes of an RSC7 resource from its header page flags
/// (CodeWalker RpfResourceFileEntry.GetSizeFromFlags).
fn rsc7_mem(p: &Path) -> Option<(u64, u64)> {
    use std::io::Read;
    let mut h = [0u8; 16];
    fs::File::open(p).ok()?.read_exact(&mut h).ok()?;
    if &h[..4] != b"RSC7" {
        return None;
    }
    let size = |f: u32| -> u64 {
        let f = f as u64;
        let n = ((f >> 27) & 1) + (((f >> 26) & 1) << 1) + (((f >> 25) & 1) << 2) + (((f >> 24) & 1) << 3) + (((f >> 17) & 0x7F) << 4) + (((f >> 11) & 0x3F) << 5) + (((f >> 7) & 0xF) << 6) + (((f >> 5) & 3) << 7) + (((f >> 4) & 1) << 8);
        (0x200u64 << (f & 0xF)) * n
    };
    let u = |o: usize| u32::from_le_bytes(h[o..o + 4].try_into().unwrap());
    Some((size(u(8)), size(u(12))))
}

/// End-of-run diagnostics: stage timings, cache hits, largest files and
/// files over 16 MB (returned: they fail the build; FiveM evicts them), skip
/// reasons with sample names, entity load. Printed, stored in the report
/// (`diagnostics`) and written to <out>/fivem-build-<unix time>.log.
fn diagnostics(ctx: &Ctx, total: f64) -> Vec<String> {
    let mut t = String::new();
    let r = ctx.report.lock().unwrap();
    t += &format!("fivem build: {total:.1}s total
stages:
");
    for v in &r.timings {
        t += &format!("  {:<22} {:>7}s wall {:>8}s cpu {:>5} cores
", v["phase"].as_str().unwrap_or(""), v["wall"], v["cpu"], v["cores"]);
    }
    t += "cache (kept in stream / from output cache / converted):
";
    for (ext, c) in &r.cache {
        t += &format!("  {ext:<5} {:>6} {:>6} {:>6}
", c[0], c[1], c[2]);
    }
    let mut files: Vec<(String, u64)> = fs::read_dir(&ctx.stream).into_iter().flatten().flatten().filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), e.metadata().ok()?.len()))).collect();
    files.sort_by(|a, b| b.1.cmp(&a.1));
    // FiveM limits a streamed file by its in-memory (virtual + physical)
    // size from the RSC7 page flags, not by the compressed file size
    let mem = |n: &str| rsc7_mem(&ctx.stream.join(n));
    let over: Vec<(String, u64)> = files.iter().filter_map(|f| mem(&f.0).filter(|m| m.1 > 16 << 20 || m.0 > 16 << 20).map(|m| (f.0.clone(), m.0.max(m.1)))).collect();
    let mb = |b: u64| b as f64 / 1048576.0;
    t += &format!("stream: {} files, {:.0} MB; largest:
", files.len(), mb(files.iter().map(|f| f.1).sum()));
    for (n, b) in files.iter().take(20) {
        t += &format!("  {:>8.1} MB {n}
", mb(*b));
    }
    if !over.is_empty() {
        t += &format!("WARNING: {} files over 16 MB virtual or physical (RSC7 page flags; FiveM refuses or evicts them): {}
", over.len(), over.iter().take(10).map(|f| format!("{} ({:.0} MB)", f.0, mb(f.1))).collect::<Vec<_>>().join(", "));
    }
    let mut samples: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for v in &r.skipped {
        let e = samples.entry(v["reason"].as_str().unwrap_or("?")).or_default();
        if e.len() < 5 {
            e.push(v["name"].as_str().unwrap_or("?"));
        }
    }
    t += "skipped / placeholders:
";
    for (reason, n) in &r.skipped_counts {
        t += &format!("  {n:>6} {reason}: {}
", samples.get(reason.as_str()).map(|s| s.join(", ")).unwrap_or_default());
    }
    if let Some(l) = r.outputs.iter().find(|v| v["file"] == "(entity load)") {
        t += &format!("entities: {}, at most {} streamed at once near {}
", l["total"], l["max_at_once"], l["at"]);
        if let Some(m) = l["per_ymap"].as_object() {
            let mut v: Vec<(&String, u64)> = m.iter().map(|(k, n)| (k, n.as_u64().unwrap_or(0))).collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            t += &format!("  largest ymaps: {}
", v.iter().take(10).map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", "));
        }
    }
    let diag = json!({"largest": files.iter().take(20).map(|f| json!({"file": f.0, "bytes": f.1})).collect::<Vec<_>>(), "over_16mb": over.iter().map(|f| f.0.clone()).collect::<Vec<_>>(),
        "skip_samples": samples});
    drop(r);
    eprint!("{t}");
    let secs = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let _ = fs::write(ctx.o.out.join(format!("fivem-build-{secs}.log")), &t);
    ctx.report.lock().unwrap().diagnostics = diag;
    over.into_iter().map(|f| f.0).collect()
}

/// gtxd.meta parent chain check after the buildings stage (see
/// verify::gtxd_chain): printed and stored in the report; problems are
/// warnings, since partial (--name) builds legitimately miss txds.
fn check_gtxd(ctx: &Ctx) {
    let texs: Vec<(String, Vec<String>)> = ctx.manifest.lock().unwrap().files.iter()
        .filter(|(f, _)| f.ends_with(".ydr"))
        .filter_map(|(f, e)| Some((f.trim_end_matches(".ydr").to_string(), e.info["texs"].as_array()?.iter().filter_map(|t| t.as_str().map(String::from)).collect())))
        .collect();
    let txd = ctx.layout.lock().unwrap().txd.clone();
    let (s, bad) = verify::gtxd_chain(&ctx.o.out, &ctx.stream, &txd, &texs);
    eprintln!("{s}");
    for b in &bad {
        eprintln!("gtxd WARNING: {b}");
    }
    let mut r = ctx.report.lock().unwrap();
    r.warnings.extend(bad.iter().map(|b| json!({"gtxd": b})));
    r.outputs.push(json!({"file": "gtxd.meta", "status": "checked", "summary": s, "problems": bad}));
}

/// Water cell size (water_port.py).
pub const WATER_CELL: f32 = 256.0;

/// After a full build (all parts, no --name): delete stream files and
/// manifest entries this run did not emit, so old outputs don't pile up.
fn prune(ctx: &Ctx) -> Result<(), Box<dyn Error>> {
    let keep = ctx.emitted.lock().unwrap();
    let (mut n, mut bytes) = (0usize, 0u64);
    for e in fs::read_dir(&ctx.stream)?.flatten() {
        let f = e.file_name().to_string_lossy().into_owned();
        if e.file_type().is_ok_and(|t| t.is_file()) && !keep.contains(&f) {
            bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
            fs::remove_file(e.path())?;
            n += 1;
        }
    }
    ctx.manifest.lock().unwrap().files.retain(|f, _| keep.contains(f));
    if n > 0 {
        eprintln!("pruned {n} stale stream files ({:.0} MB)", bytes as f64 / 1048576.0);
    }
    ctx.report.lock().unwrap().outputs.push(json!({"file": "stream/", "status": "pruned", "files": n, "bytes": bytes}));
    Ok(())
}

/// water.xml (water_port.py): 256 m ocean cells at z 0 over the 6m terrain
/// extent plus a 4 km margin (or --bbox plus one cell), wet only where the
/// lowest terrain vertex in the cell is below z + 0.5 or there is no terrain
/// (<work>/fivem_water_low.json from the terrain stage); runs in a row merge.
fn water(ctx: &Ctx) -> Result<(), Box<dyn Error>> {
    let s = WATER_CELL as i64;
    let z = 0.0f32;
    let (x0, y0, x1, y1, m) = match ctx.o.bbox {
        Some(b) => (b[0].floor() as i64, b[1].floor() as i64, b[2].ceil() as i64, b[3].ceil() as i64, s),
        None => (-9607, -7957, 2442, 9474, 4000),
    };
    let lp = ctx.o.work.join("fivem_water_low.json");
    let low: std::collections::HashMap<(i64, i64), f32> = match sixng_core::vfs::read_to_string(&lp).ok().and_then(|t| serde_json::from_str::<BTreeMap<String, f32>>(&t).ok()) {
        Some(m) => m.into_iter().filter_map(|(k, v)| { let (a, b) = k.split_once(',')?; Some(((a.parse().ok()?, b.parse().ok()?), v)) }).collect(),
        None => {
            ctx.skip("water.xml", "no terrain heights (run the terrain part); ocean everywhere", json!({}));
            Default::default()
        }
    };
    let wet = |i: i64, j: i64| low.get(&(i, j)).is_none_or(|h| *h < z + 0.5);
    let (i0, j0) = ((x0 - m).div_euclid(s), (y0 - m).div_euclid(s));
    let (i1, j1) = ((x1 + m).div_euclid(s), (y1 + m).div_euclid(s));
    let mut q = String::new();
    let mut n = 0;
    for j in j0..=j1 {
        let mut i = i0;
        while i <= i1 {
            if !wet(i, j) {
                i += 1;
                continue;
            }
            let a = i;
            while i <= i1 && wet(i, j) {
                i += 1;
            }
            q += &format!(
                "  <Item><minX value=\"{}\" /><maxX value=\"{}\" /><minY value=\"{}\" /><maxY value=\"{}\" /><Type value=\"0\" /><IsInvisible value=\"false\" /><HasLimitedDepth value=\"false\" /><z value=\"{:.1}\" /><a1 value=\"26\" /><a2 value=\"26\" /><a3 value=\"26\" /><a4 value=\"26\" /><NoStencil value=\"false\" /></Item>
",
                (a * s) as f32 + ctx.o.offset[0],
                (i * s) as f32 + ctx.o.offset[0],
                (j * s) as f32 + ctx.o.offset[1],
                ((j + 1) * s) as f32 + ctx.o.offset[1],
                z + ctx.o.offset[2]
            );
            n += 1;
        }
    }
    let xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<WaterData>
 <WaterQuads>
{q} </WaterQuads>
 <CalmingQuads />
 <WaveQuads />
</WaterData>
");
    let p = ctx.o.out.join("water.xml");
    let same = fs::read_to_string(&p).is_ok_and(|t| t == xml);
    if !same {
        fs::write(&p, &xml)?;
    }
    eprintln!("water: {n} quads");
    ctx.report.lock().unwrap().outputs.push(json!({"file": "water.xml", "status": if same {"up-to-date"} else {"written"}, "quads": n}));
    Ok(())
}

fn fxmanifest(ctx: &Ctx) -> Result<(), Box<dyn Error>> {
    let mut ytyps: Vec<String> = fs::read_dir(&ctx.stream)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".ytyp"))
        .collect();
    ytyps.sort();
    let mut t = String::from("fx_version 'cerulean'\ngame 'gta5'\nthis_is_a_map 'yes'\n");
    for y in &ytyps {
        t += &format!("data_file 'DLC_ITYP_REQUEST' 'stream/{y}'\n");
    }
    let files: Vec<&str> = ["water.xml", "gtxd.meta"].into_iter().filter(|f| ctx.o.out.join(f).exists()).collect();
    if ctx.o.out.join("t6m_zones.lua").exists() {
        t += "client_script 't6m_zones.lua'
";
    }
    if !files.is_empty() {
        t += &format!("files {{ {} }}\n", files.iter().map(|f| format!("'{f}'")).collect::<Vec<_>>().join(", "));
    }
    if files.contains(&"water.xml") {
        t += "data_file 'WATER_FILE' 'water.xml'\n";
    }
    if files.contains(&"gtxd.meta") {
        t += "data_file 'GTXD_PARENTING_DATA' 'gtxd.meta'\n";
    }
    fs::write(ctx.o.out.join("fxmanifest.lua"), t)?;
    Ok(())
}

/// Live reload: print (and write to <out>/.fivem-changed.txt) the resources
/// whose files changed this run and the FXServer console line that reloads
/// just those. With --out inside the server's resources folder that is all a
/// rebuild needs: unchanged resources keep their files and hashes.
pub fn live_report(ctx: &Ctx, changed: &[String]) {
    let line = if changed.is_empty() { String::new() } else { format!("refresh; {}", changed.iter().map(|r| format!("ensure {r}")).collect::<Vec<_>>().join("; ")) };
    if changed.is_empty() {
        eprintln!("live: nothing changed");
    } else {
        eprintln!("live: {} resource(s) changed; FXServer console: {line}", changed.len());
    }
    let _ = fs::write(ctx.o.out.join(".fivem-changed.txt"), format!("{}
{line}
", changed.join("
")));
    ctx.report.lock().unwrap().outputs.push(json!({"file": ".fivem-changed.txt", "status": "live", "changed": changed, "command": line}));
}

/// Process CPU time (user + kernel) in seconds.
#[cfg(windows)]
pub fn cpu_secs() -> f64 {
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Ft(u32, u32);
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetProcessTimes(h: isize, c: *mut Ft, e: *mut Ft, k: *mut Ft, u: *mut Ft) -> i32;
    }
    let (mut c, mut e, mut k, mut u) = (Ft::default(), Ft::default(), Ft::default(), Ft::default());
    // SAFETY: plain Win32 call with valid out pointers.
    if unsafe { GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) } == 0 {
        return 0.0;
    }
    let t = |f: Ft| ((f.1 as u64) << 32 | f.0 as u64) as f64 / 1e7;
    t(k) + t(u)
}
#[cfg(not(windows))]
pub fn cpu_secs() -> f64 {
    0.0
}

/// Stage timer: prints wall time, CPU time and average cores in use on drop,
/// and records them in the report (`timings`).
pub struct Phase<'a> {
    ctx: &'a Ctx,
    name: String,
    t0: std::time::Instant,
    c0: f64,
}

impl<'a> Phase<'a> {
    pub fn new(ctx: &'a Ctx, name: &str) -> Self {
        // phase start / end as progress events (stage = phase name)
        progress::report(name.trim(), 0, 1, &std::sync::Mutex::new(None));
        Phase { ctx, name: name.to_string(), t0: std::time::Instant::now(), c0: cpu_secs() }
    }
}

impl Drop for Phase<'_> {
    fn drop(&mut self) {
        progress::report(self.name.trim(), 1, 1, &std::sync::Mutex::new(None));
        let wall = self.t0.elapsed().as_secs_f64();
        let cpu = cpu_secs() - self.c0;
        let cores = if wall > 0.0 { cpu / wall } else { 0.0 };
        eprintln!("time {:<22} {wall:7.1}s wall {cpu:8.1}s cpu {cores:5.1} cores", self.name);
        self.ctx.report.lock().unwrap().timings.push(json!({"phase": self.name, "wall": (wall * 10.0).round() / 10.0, "cpu": (cpu * 10.0).round() / 10.0, "cores": (cores * 10.0).round() / 10.0}));
    }
}

/// Put `src` at `dst` as a hard link (copy when linking fails, e.g. across
/// volumes), replacing what was there.
pub fn link_into(src: &Path, dst: &Path) -> std::io::Result<()> {
    let tmp = dst.with_extension(format!("lnk{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    if fs::hard_link(src, &tmp).is_err() {
        fs::copy(src, &tmp)?;
    }
    fs::rename(&tmp, dst)
}

/// Write a file under the out dir's texture cache atomically.
pub fn write_atomic(p: &Path, b: &[u8]) -> std::io::Result<()> {
    let tmp = p.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, b)?;
    fs::rename(&tmp, p)
}
