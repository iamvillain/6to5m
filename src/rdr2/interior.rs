//! `6ixpack redm interior`: one GTA VI (6m) interior (MLO) -> RedM resource.
//!
//! The interior's data (`fivem::interior_source`: placements, rooms,
//! portals, entities, entity sets, linked collision); every distinct entity
//! archetype converted with the `redm prop` path into
//! `stream/<archetype>.ydr`, textures deduplicated into shared
//! `stream/<int>_txd<N>.ytd`; `stream/<int>.ytyp` with the prop archetypes
//! and the MLO archetype (`mlo::assemble`); `stream/<int>_milo_.ymap`
//! placing it; the collision pair `stream/<int>.ybn` + `stream/hi@<int>.ybn`;
//! `stream/_manifest.ymf` (PSO: the milo ymap and the ytyp as interior data,
//! the collision pair); `fxmanifest.lua`; and a debug dump `interior.json`.

use super::{archetype_lod, check_drawable_name, check_shader_textures, is_model_name, manifest, mlo, prop_textures, read, write, ybn, ydr, ymap, ytd, ytyp};
use crate::gta5::{self, fivem};
use rayon::prelude::*;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

pub const USAGE: &str = "\
usage: 6ixpack redm interior --list [pattern] --work <dir> [--game <GTA VI dir>]
       6ixpack redm interior <name> --game <GTA VI dir> --work <dir> --out <resource>
          [--at x,y,z[,heading]] [--no-manifest] [--max-tex <px>] [--default-only]
  --list: every 6m interior of <work>/placements/levels/interiors.jsonl whose name contains
          [pattern]: rooms, portals (decoded only with --game), entities (+ entity-set
          entities), resolvable entities / all, distinct entity archetypes (resolvable / all),
          linked collision, world position(s)
  <name>: int_<hash> as --list prints it (or the 8-digit hash, or the 6m name)
  Output (stream/): <archetype>.ydr per converted entity archetype (the `redm prop` path: all
  LOD levels, standard/default shaders, _ma maps, tint row 0), shared <int>_txd<N>.ytd (each
  under 14 MiB of pixel data), <int>.ytyp (the prop archetypes + the CMloArchetypeDef: rooms,
  portals, the entities whose archetype converted, entity sets), <int>_milo_.ymap (the
  CMloInstanceDef), <int>.ybn + hi@<int>.ybn (collision, interior space; 6m surface ids ->
  RDR2 materials through `sixng surfaces` categories, else DEFAULT); fxmanifest.lua
  (this_is_a_map, as the working custom interiors); interior.json: a DEBUG dump (rooms,
  portals, entities, placement, collision, per-archetype results), not read by RedM.
  --at x,y,z[,heading]: place the interior there (heading in degrees, default 0) instead of
          its 6m world position. GTA VI coordinates are not RDR2 ones: at the 6m position the
          interior may land anywhere on (or off) the RDR2 map.
  stream/_manifest.ymf (PSO CPackFileMetaData, as the working custom interiors have it):
          <int>_milo_ -> <int> (INTERIOR_DATA), <int> (INTERIOR_DATA), interior <int> ->
          [hi@<int>, <int>]. --no-manifest leaves it out.
  Needs in <work>: `sixng index levels`, `sixng collision levels --out <work>`, then
  `sixng interiors levels --out <work>` and `sixng placements levels --id <interior file>
  --out <work>` (the command names the missing one); `sixng surfaces levels --navmeshes none
  --out <work>` for collision materials.";
/// Pixel data per texture dictionary. CitizenFX streams a resource file
/// whole and FiveM refuses / evicts streamed files over 16 MiB of
/// physical memory (the same streaming code is assumed for RedM; not
/// verified there). 14 MiB of BlockCount x BlockStride leaves room for the
/// 16-byte alignment of every texture and the system segment.
pub const TXD_CAP: u64 = 14 << 20;

#[derive(Debug, Default, PartialEq)]
struct Args {
    name: Option<String>,
    list: Option<String>,
    game: Option<PathBuf>,
    work: Option<PathBuf>,
    out: Option<PathBuf>,
    max_tex: Option<u16>,
    default_only: bool,
    /// x, y, z, heading (degrees)
    at: Option<[f32; 4]>,
    no_manifest: bool,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut i = 0;
    let val = |i: &mut usize, k: &str| -> Result<String, String> {
        *i += 1;
        args.get(*i).cloned().ok_or(format!("{k} needs a value"))
    };
    while i < args.len() {
        let k = args[i].as_str();
        match k {
            // the pattern is optional: `--list` alone lists everything
            "--list" => {
                let next = args.get(i + 1).filter(|v| !v.starts_with("--"));
                a.list = Some(next.map(|v| v.to_ascii_lowercase()).unwrap_or_default());
                if next.is_some() {
                    i += 1;
                }
            }
            "--game" => a.game = Some(val(&mut i, k)?.into()),
            "--work" => a.work = Some(val(&mut i, k)?.into()),
            "--out" => a.out = Some(val(&mut i, k)?.into()),
            "--max-tex" => a.max_tex = Some(val(&mut i, k)?.parse().map_err(|e| format!("--max-tex: {e}"))?),
            "--default-only" => a.default_only = true,
            "--no-manifest" => a.no_manifest = true,
            "--at" => a.at = Some(parse_at(&val(&mut i, k)?)?),
            x if !x.starts_with("--") && a.name.is_none() => a.name = Some(x.to_ascii_lowercase()),
            _ => return Err(format!("unknown argument {k}\n{USAGE}")),
        }
        i += 1;
    }
    if a.list.is_some() && a.name.is_some() {
        return Err("--list takes a pattern, not an interior name".into());
    }
    Ok(a)
}

pub fn run(args: &[String]) -> Result<(), String> {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    let a = parse(args)?;
    let work = a.work.clone().unwrap_or_else(|| std::env::var_os("SIXNG_WORK").map(PathBuf::from).unwrap_or_else(|| "work".into()));
    if let Some(pat) = &a.list {
        return list(&work, a.game.as_deref(), pat);
    }
    let (Some(name), Some(game), Some(out)) = (a.name.clone(), a.game.clone(), a.out.clone()) else { return Err(USAGE.into()) };
    convert(&name, &game, &work, &out, &a)
}

fn list(work: &Path, game: Option<&Path>, pat: &str) -> Result<(), String> {
    let v = fivem::interior_list(work, game, pat)?;
    println!("name\trooms\tportals\tentities\tresolvable entities/all\tarchetypes (resolvable/all)\tcollision\tposition");
    for s in &v {
        let portals = match s.portals {
            None => "-".to_string(),
            Some((n, true)) => n.to_string(),
            Some((_, false)) => "undecoded".to_string(),
        };
        let ents = if s.set_entities > 0 { format!("{}+{}", s.entities, s.set_entities) } else { s.entities.to_string() };
        let col = s.collision.as_ref().map_or("none".to_string(), |c| c.1.clone());
        let pos: Vec<String> = s.placements.iter().map(|p| format!("{:.0},{:.0},{:.0}", p[0], p[1], p[2])).collect();
        let arch = format!("{}/{}", s.resolvable, s.archetypes);
        let er = format!("{}/{}", s.entities_resolvable, s.entities + s.set_entities);
        let line = format!("{}\t{}\t{portals}\t{ents}\t{er}\t{arch}\t{col}\t{}", fivem::interior_name(s.hash), s.rooms, pos.join(" "));
        match &s.problem {
            Some(p) => println!("{line}\t({p})"),
            None => println!("{line}"),
        }
    }
    eprintln!("{} interiors{}", v.len(), if game.is_none() { " (portals: pass --game to decode)" } else { "" });
    Ok(())
}

/// Textures of several models in one pool: identical textures (same size,
/// format, colour space and pixel data) are stored once, whatever their
/// names; a name already taken by different pixels gets a `_<n>` suffix.
#[derive(Default)]
pub struct TexPool {
    pub texs: Vec<ytd::Texture>,
    by_name: HashMap<String, usize>,
    by_key: HashMap<String, usize>,
}

fn content_key(t: &ytd::Texture) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(format!("{}x{}|{}|{:?}|{}|", t.tex.width, t.tex.height, t.tex.levels, t.tex.format, t.srgb));
    h.update(&t.tex.data);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

impl TexPool {
    /// Add one model's textures; returns (pool indices of the model's
    /// textures, renames: the model's name -> pool name, where they differ).
    pub fn add(&mut self, texs: Vec<ytd::Texture>) -> (Vec<usize>, HashMap<String, String>) {
        let (mut ids, mut ren) = (Vec::new(), HashMap::new());
        for mut t in texs {
            let key = content_key(&t);
            let name = t.tex.name.clone();
            let i = match self.by_key.get(&key) {
                Some(&i) => i,
                None => {
                    let mut n = name.clone();
                    let mut k = 1;
                    while self.by_name.contains_key(&n) {
                        n = format!("{name}_{k}");
                        k += 1;
                    }
                    t.tex.name = n.clone();
                    self.texs.push(t);
                    let i = self.texs.len() - 1;
                    self.by_name.insert(n, i);
                    self.by_key.insert(key, i);
                    i
                }
            };
            if self.texs[i].tex.name != name {
                ren.insert(name, self.texs[i].tex.name.clone());
            }
            if !ids.contains(&i) {
                ids.push(i);
            }
        }
        (ids, ren)
    }
}

/// Point a model's geometries at renamed pool textures.
fn rename_textures(levels: &mut [gta5::ydr::Mesh], ren: &HashMap<String, String>) {
    if ren.is_empty() {
        return;
    }
    for g in levels.iter_mut().flat_map(|l| l.geoms.iter_mut()) {
        for s in [&mut g.texture, &mut g.normal, &mut g.spec] {
            if let Some(n) = ren.get(&s.to_ascii_lowercase()) {
                *s = n.clone();
            }
        }
    }
}

/// One texture dictionary of a pack: pool indices and pixel bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Txd {
    pub textures: Vec<usize>,
    pub bytes: u64,
}

/// Put every model's textures (pool indices) in one dictionary each, at
/// most `cap` pixel bytes per dictionary (a model bigger than `cap` alone
/// gets its own). Biggest models first; each goes to the dictionary that
/// already holds most of its bytes and still fits, else the first that
/// fits, else a new one. A texture is stored twice only when two models
/// using it end up in different dictionaries. Returns the dictionaries and
/// each model's dictionary.
pub fn pack(models: &[Vec<usize>], sizes: &[u64], cap: u64) -> (Vec<Txd>, Vec<usize>) {
    let total = |m: &[usize]| m.iter().map(|&t| sizes[t]).sum::<u64>();
    let mut order: Vec<usize> = (0..models.len()).collect();
    order.sort_by(|&a, &b| total(&models[b]).cmp(&total(&models[a])).then(a.cmp(&b)));
    let mut txds: Vec<Txd> = Vec::new();
    let mut of = vec![0usize; models.len()];
    for m in order {
        let want = &models[m];
        let best = txds
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                let (shared, extra): (Vec<usize>, Vec<usize>) = want.iter().partition(|t| d.textures.contains(t));
                (d.bytes + total(&extra) <= cap).then(|| (total(&shared), i))
            })
            .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        let i = match best {
            Some((_, i)) => i,
            None => {
                txds.push(Txd { textures: Vec::new(), bytes: 0 });
                txds.len() - 1
            }
        };
        for &t in want {
            if !txds[i].textures.contains(&t) {
                txds[i].textures.push(t);
                txds[i].bytes += sizes[t];
            }
        }
        of[m] = i;
    }
    (txds, of)
}

/// Output model name of an entity archetype: its 6m name when it is a
/// valid RedM model name (as `redm prop` names it), else `r6m_<hash>` (no
/// recovered name, sixng's `arch_<hash>` stand-in, or invalid characters).
/// ydr stem, drawable name and ytyp archetype all use it.
pub fn output_name(hash: u32, name: Option<&str>) -> String {
    match name.map(str::to_ascii_lowercase) {
        Some(n) if is_model_name(&n) && !n.starts_with("arch_") => n,
        _ => format!("r6m_{hash:08x}"),
    }
}

/// Make one model's textures fit `cap`: drop the top mip of its largest
/// texture until the set fits (or nothing has a mip left). Shared pool
/// textures shrink for every model using them. Returns the mips dropped.
pub fn fit_model(pool: &mut TexPool, ids: &[usize], cap: u64) -> Result<usize, String> {
    let mut dropped = 0;
    loop {
        let sizes: Vec<u64> = ids.iter().map(|&i| ytd::stored_size(&pool.texs[i])).collect::<Result<_, _>>()?;
        if sizes.iter().sum::<u64>() <= cap {
            return Ok(dropped);
        }
        let Some((k, _)) = ids.iter().zip(&sizes).enumerate().filter(|(_, (i, _))| pool.texs[**i].tex.levels > 1).max_by_key(|(_, (_, s))| **s).map(|(k, x)| (k, *x.1)) else {
            return Ok(dropped);
        };
        let t = &mut pool.texs[ids[k]].tex;
        let edge = t.width.max(t.height);
        if !t.limit_size(edge / 2) {
            return Ok(dropped);
        }
        dropped += 1;
    }
}

/// Why an archetype was not converted, or its numbers.
enum Outcome {
    Skipped(String),
    Converted { tris: u32, lod_tris: Vec<u32>, textures: usize, ydr: usize, txd: usize },
}

struct Model {
    name: String,
    levels: Vec<gta5::ydr::Mesh>,
    lod_dist: [f32; 4],
    texs: Vec<ytd::Texture>,
    missing: Vec<String>,
}

fn convert(want: &str, game: &Path, work: &Path, out: &Path, a: &Args) -> Result<(), String> {
    let t0 = std::time::Instant::now();
    let src = fivem::interior_source(game, work, want)?;
    let int = src.name.clone();
    let opts = ydr::Opts { standard: !a.default_only };
    let batch = fivem::PropBatch::open(game, work)?;
    let archives = batch.archives();
    // distinct entity archetypes (by hash: the joaat of the 6m name)
    let mut arch: BTreeMap<u32, (Option<String>, Option<String>, usize)> = BTreeMap::new();
    for e in &src.entities {
        arch.entry(e.archetype).or_insert((e.name.clone(), e.model_id.clone(), 0)).2 += 1;
    }
    let mut outcome: BTreeMap<String, Outcome> = BTreeMap::new();
    // (output name, name to pick the model by, model id)
    let mut todo: Vec<(String, String, String)> = Vec::new();
    for (h, (n, mid, _)) in &arch {
        let out_name = output_name(*h, n.as_deref());
        match fivem::interior_resolvable(mid.as_deref(), &archives) {
            Err(e) => drop(outcome.insert(out_name, Outcome::Skipped(format!("missing from index: {e}")))),
            Ok(m) => todo.push((out_name.clone(), n.clone().unwrap_or(out_name), m)),
        }
    }
    eprintln!("redm: {int}: {} entities, {} distinct archetypes, converting {}", src.entities.len(), arch.len(), todo.len());
    // decode in parallel (shared texture pool), then pool the textures in name order
    let done: Vec<(String, Result<Model, String>)> = todo
        .par_iter()
        .map(|(n, pick, mid)| {
            let r = (|| {
                let s = batch.source_model(pick, mid, fivem::TintPick { row: 0, exact: true })?;
                if s.skinned {
                    return Err("skinned (bones): not a static prop".to_string());
                }
                let mut levels = s.levels;
                for l in &mut levels {
                    l.name = n.clone();
                }
                let texs = prop_textures(&s.textures, &levels, &opts, a.max_tex)?;
                Ok(Model { name: n.clone(), levels, lod_dist: s.lod_dist, texs, missing: s.missing })
            })();
            (n.to_string(), r)
        })
        .collect();
    let mut pool = TexPool::default();
    let mut built: Vec<(String, Vec<u8>, read::YdrInfo, Vec<usize>, Vec<String>)> = Vec::new();
    // LOD0 vertex samples per converted model (rotation check)
    let mut samples: HashMap<String, Vec<[f32; 3]>> = HashMap::new();
    for (n, r) in done {
        let mut m = match r {
            Ok(m) => m,
            Err(e) => {
                outcome.insert(n, Outcome::Skipped(e.lines().next().unwrap_or("").to_string()));
                continue;
            }
        };
        if m.levels.iter().all(|l| l.geoms.is_empty()) {
            outcome.insert(n, Outcome::Skipped("no triangles".into()));
            continue;
        }
        let all: Vec<[f32; 3]> = m.levels[0].geoms.iter().flat_map(|g| g.positions.iter().copied()).collect();
        samples.insert(n.clone(), all.iter().step_by(all.len().div_ceil(200).max(1)).copied().collect());
        let (ids, ren) = pool.add(std::mem::take(&mut m.texs));
        rename_textures(&mut m.levels, &ren);
        let res = ydr::build_ydr(&m.levels, m.lod_dist, None, &opts).map_err(|e| format!("{n}.ydr: {e}")).and_then(|b| {
            let info = read::ydr(&b).map_err(|e| format!("{n}.ydr self-check: {e}"))?;
            check_drawable_name(&m.name, &info)?;
            Ok((b, info))
        });
        match res {
            Ok((b, info)) => built.push((n, b, info, ids, m.missing)),
            Err(e) => drop(outcome.insert(n, Outcome::Skipped(e))),
        }
    }
    if built.is_empty() {
        write_debug(out, &src, &outcome, &[])?;
        return Err(format!("{int}: no archetype converted (see {})", out.join("interior.json").display()));
    }
    // shared texture dictionaries; a model whose own textures exceed the
    // cap has its largest textures halved until they fit (one dictionary
    // per archetype)
    for (n, _, _, ids, _) in &built {
        let k = fit_model(&mut pool, ids, TXD_CAP)?;
        if k > 0 {
            eprintln!("redm: {n}: {k} top mips dropped so its textures fit one {} MiB dictionary", TXD_CAP >> 20);
        }
    }
    let sizes: Vec<u64> = pool.texs.iter().map(ytd::stored_size).collect::<Result<_, _>>()?;
    let (txds, of) = pack(&built.iter().map(|b| b.3.clone()).collect::<Vec<_>>(), &sizes, TXD_CAP);
    let txd_name = |i: usize| format!("{int}_txd{i}");
    let stream = out.join("stream");
    let mut files: Vec<(String, usize)> = Vec::new();
    let mut dicts = Vec::new();
    for (i, d) in txds.iter().enumerate() {
        let mut ix = d.textures.clone();
        ix.sort_by(|a, b| pool.texs[*a].tex.name.cmp(&pool.texs[*b].tex.name));
        let texs: Vec<ytd::Texture> = ix.iter().map(|&t| clone_tex(&pool.texs[t])).collect();
        let n = txd_name(i);
        let b = ytd::build_ytd(&texs).map_err(|e| format!("{n}.ytd: {e}"))?;
        let dict = read::ytd(&b).map_err(|e| format!("{n}.ytd self-check: {e}"))?;
        if d.bytes > TXD_CAP {
            eprintln!("redm: {n}.ytd: {:.1} MiB of pixel data, over the {} MiB cap (one model's textures)", d.bytes as f64 / 1048576.0, TXD_CAP >> 20);
        }
        write(&stream.join(format!("{n}.ytd")), &b)?;
        files.push((format!("{n}.ytd"), b.len()));
        dicts.push(dict);
    }
    let mut archs = Vec::new();
    for (k, (n, b, info, _, _)) in built.iter().enumerate() {
        let tn = txd_name(of[k]);
        check_shader_textures(n, info, &dicts[of[k]], &format!("{tn}.ytd"))?;
        let mut x = ytyp::Archetype::from_box(n, archetype_lod(info), info.bbox.0, info.bbox.1);
        x.texture_dictionary = tn;
        archs.push(x);
        write(&stream.join(format!("{n}.ydr")), b)?;
        files.push((format!("{n}.ydr"), b.len()));
    }
    // the interior: MLO archetype (converted entities only), placement, collision
    let converted: HashMap<u32, String> = arch.iter().map(|(h, (n, _, _))| (*h, output_name(*h, n.as_deref()))).filter(|(_, n)| built.iter().any(|b| &b.0 == n)).collect();
    let input = mlo_input(&src);
    let (mlo_def, st) = mlo::assemble(&input, &|h| converted.get(&h).cloned(), 0x4000_0000_0000_0000 | (src.hash as u64) << 16);
    let ytyp_b = ytyp::build_ytyp_mlo(&int, &archs, Some(&mlo_def)).map_err(|e| format!("{int}.ytyp: {e}"))?;
    let types = read::ytyp(&ytyp_b).map_err(|e| format!("{int}.ytyp self-check: {e}"))?;
    for (a, x) in types.archetypes.iter().zip(&archs) {
        let h = gta5::rsc::joaat(&x.name);
        if a.name != h || a.asset_name != h || a.texture_dictionary != gta5::rsc::joaat(&x.texture_dictionary) {
            return Err(format!("{int}.ytyp: archetype {} hashes do not match", x.name));
        }
    }
    let rm = &types.mlos[0];
    if rm.entities != mlo_def.entities.len() || rm.rooms.len() != mlo_def.rooms.len() || rm.portals.len() != mlo_def.portals.len() {
        return Err(format!("{int}.ytyp self-check: MLO read back with {} entities / {} rooms / {} portals", rm.entities, rm.rooms.len(), rm.portals.len()));
    }
    write(&stream.join(format!("{int}.ytyp")), &ytyp_b)?;
    files.push((format!("{int}.ytyp"), ytyp_b.len()));
    let (pos, rot, placed_at) = match a.at {
        Some([x, y, z, h]) => ([x, y, z], mlo::yaw(h), "--at".to_string()),
        None => {
            let p = src.placements.first().ok_or(format!("{int}: no placement"))?;
            (p.position, p.rotation, format!("6m placement in {} (instance {})", p.map, p.instance))
        }
    };
    let inst = ymap::MloInstance { archetype: int.clone(), position: pos, rotation: rot, lod_dist: mlo_def.lod_dist, num_exit_portals: mlo_def.exit_portals(), id: 0x4000_0000_0000_0000 | src.hash as u64 };
    let (streaming, ents_ext) = mlo::extents(&mlo_def, pos, rot, mlo_def.lod_dist);
    let milo = format!("{int}_milo_");
    let ymap_b = ymap::build_milo(&milo, &inst, streaming, ents_ext).map_err(|e| format!("{milo}.ymap: {e}"))?;
    let yi = read::ymap(&ymap_b).map_err(|e| format!("{milo}.ymap self-check: {e}"))?;
    if yi.entities.len() != 1 || yi.entities[0].position != pos || yi.entities[0].rotation != rot {
        return Err(format!("{milo}.ymap self-check: placement read back differently"));
    }
    write(&stream.join(format!("{milo}.ymap")), &ymap_b)?;
    files.push((format!("{milo}.ymap"), ymap_b.len()));
    // collision pair (interior space)
    let surfaces = surface_names(work);
    let surface = |id: u16| surfaces.get(&id).copied().unwrap_or("DEFAULT");
    let mut col_report = json!(null);
    if let Some(c) = &src.collision {
        let mut rep = Vec::new();
        for (file, tris, tf, inc) in [(int.clone(), &c.mover, ybn::TYPE_MOVER_WEAPON, ybn::INCLUDE_MOVER_WEAPON), (format!("hi@{int}"), &c.all, ybn::TYPE_WEAPON, ybn::INCLUDE_WEAPON)] {
            let (kids, census) = mlo::collision(&tris.vertices, &tris.triangles, &tris.materials, &mlo_def.rooms, &surface, tf, inc)?;
            if kids.is_empty() {
                return Err(format!("{file}.ybn: no triangles"));
            }
            let b = ybn::build_ybn(&kids).map_err(|e| format!("{file}.ybn: {e}"))?;
            let yb = ybn::read(&b).map_err(|e| format!("{file}.ybn self-check: {e}"))?;
            let polys: u32 = yb.children.iter().map(|c| c.4).sum();
            if polys as usize != tris.triangles.len() {
                return Err(format!("{file}.ybn self-check: {polys} polygons, {} written", tris.triangles.len()));
            }
            write(&stream.join(format!("{file}.ybn")), &b)?;
            files.push((format!("{file}.ybn"), b.len()));
            rep.push(json!({"file": format!("{file}.ybn"), "children": kids.len(), "vertices": yb.children.iter().map(|c| c.3).sum::<u32>(), "triangles": polys, "bytes": b.len(),
                "materials": census.iter().map(|(w, n)| json!({"material": format!("0x{w:08X}"), "room": w >> 24 & 31, "triangles": n})).collect::<Vec<_>>()}));
        }
        let mut kinds = [0usize; 4];
        for p in &c.primitives {
            use sixng_core::bounds::NativePrim as P;
            kinds[match p { P::Box { .. } => 0, P::Sphere { .. } => 1, P::Capsule { .. } => 2, P::Cylinder { .. } => 3 }] += 1;
        }
        col_report = json!({"files": rep, "primitives_tessellated": {"box": kinds[0], "sphere": kinds[1], "capsule": kinds[2], "cylinder": kinds[3]}, "surface_categories": !surfaces.is_empty()});
    } else {
        eprintln!("redm: {int}: no linked collision: no .ybn pair written (the interior has no floor)");
    }
    let rot_check = rotation_check(&src, &converted, &samples);
    let manifest_note = if a.no_manifest {
        let _ = std::fs::remove_file(stream.join("_manifest.ymf"));
        json!("not written (--no-manifest)")
    } else {
        let m = manifest::Manifest::interior(&milo, &int, &int, &[]);
        let b = manifest::build(&m)?;
        if manifest::read(&b).map_err(|e| format!("_manifest.ymf self-check: {e}"))? != m {
            return Err("_manifest.ymf self-check: read back differently".into());
        }
        write(&stream.join("_manifest.ymf"), &b)?;
        files.push(("_manifest.ymf".into(), b.len()));
        json!({"imaps": [{"name": milo, "flags": "INTERIOR_DATA", "ityps": [&int]}], "ityps": [{"name": &int, "flags": "INTERIOR_DATA", "deps": []}], "interiors": [{"name": &int, "bounds": [format!("hi@{int}"), int.clone()]}], "bytes": b.len()})
    };
    write(&out.join("fxmanifest.lua"), fxmanifest(&int).as_bytes())?;
    println!(
        "redm: {int} MLO: {} entities ({} dropped: archetype not converted; {} without a room -> limbo{}), {} rooms, {} portals ({} exits, {} corner orders reversed), {} entity sets ({} left empty); placed at ({:.3}, {:.3}, {:.3}) rotation ({:.6}, {:.6}, {:.6}, {:.6}) from {placed_at}",
        mlo_def.entities.len(), st.dropped, st.to_limbo, if st.all_limbo { "; portals undecoded: all in limbo" } else { "" }, mlo_def.rooms.len(), mlo_def.portals.len(), mlo_def.exit_portals(), st.flipped, mlo_def.sets.len(), st.empty_sets,
        pos[0], pos[1], pos[2], rot[0], rot[1], rot[2], rot[3]
    );
    println!("redm: placement caveat: GTA VI coordinates are not RDR2 coordinates; at the 6m position the interior may sit anywhere on (or off) the RDR2 map. Use --at x,y,z[,heading] to place it.");
    if let Some(r) = rot_check.as_ref() {
        println!("redm: rotation check: {}", r["summary"].as_str().unwrap_or(""));
    }
    for (k, (n, b, info, ids, missing)) in built.iter().enumerate() {
        let lod_tris: Vec<u32> = (0..4).map(|l| info.geoms.iter().filter(|g| g.lod == l).map(|g| g.indices / 3).sum()).collect();
        if !missing.is_empty() {
            eprintln!("redm: {n}: textures missing (grey placeholder): {}", missing.join("; "));
        }
        outcome.insert(n.clone(), Outcome::Converted { tris: lod_tris[0], lod_tris, textures: ids.len(), ydr: b.len(), txd: of[k] });
    }
    let reports = write_debug(out, &src, &outcome, &txds.iter().enumerate().map(|(i, d)| (txd_name(i), d.textures.len(), d.bytes)).collect::<Vec<_>>())?;
    // phase 2 results beside the gathered data
    {
        let p = out.join("interior.json");
        let mut doc: Value = serde_json::from_slice(&std::fs::read(&p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        doc["mlo"] = json!({"entities": mlo_def.entities.len(), "dropped_entities": st.dropped, "to_limbo": st.to_limbo, "all_limbo": st.all_limbo, "rooms": mlo_def.rooms.iter().map(|r| json!({"name": r.name, "entities": r.attached.len()})).collect::<Vec<_>>(),
            "portals": mlo_def.portals.len(), "exit_portals": mlo_def.exit_portals(), "corner_orders_reversed": st.flipped, "entity_sets": mlo_def.sets.len(), "placement": {"position": pos, "rotation": rot, "from": placed_at},
            "collision": col_report, "rotation_check": rot_check, "manifest": manifest_note});
        write(&p, &serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?)?;
    }
    // report
    for r in &reports {
        println!("{r}");
    }
    let (conv, skip) = outcome.values().fold((0, 0), |c, o| if matches!(o, Outcome::Converted { .. }) { (c.0 + 1, c.1) } else { (c.0, c.1 + 1) });
    let refs: usize = built.iter().map(|b| b.3.len()).sum();
    let stored: usize = txds.iter().map(|d| d.textures.len()).sum();
    let bytes = |ext: &str| files.iter().filter(|f| f.0.ends_with(ext)).map(|f| f.1).sum::<usize>();
    let mib = |b: usize| b as f64 / 1048576.0;
    println!(
        "redm: {int} -> {}: {conv} archetypes converted, {skip} skipped; {} LOD0 triangles; textures: {refs} referenced, {} unique, {stored} stored in {} dictionaries; files: ydr {:.2} MiB, ytd {:.2} MiB, ytyp {} B, total {:.2} MiB; {:.1}s",
        out.display(),
        built.iter().map(|b| b.2.geoms.iter().filter(|g| g.lod == 0).map(|g| g.indices / 3).sum::<u32>()).sum::<u32>(),
        pool.texs.len(),
        txds.len(),
        mib(bytes(".ydr")),
        mib(bytes(".ytd")),
        bytes(".ytyp"),
        mib(files.iter().map(|f| f.1).sum()),
        t0.elapsed().as_secs_f32()
    );
    Ok(())
}

/// 6m interior -> `mlo::Input`.
fn mlo_input(s: &fivem::InteriorSource) -> mlo::Input {
    mlo::Input {
        name: s.name.clone(),
        hash: s.hash,
        bbox: s.bbox,
        entities: s.entities.iter().map(|e| mlo::InEntity { archetype: e.archetype, position: e.position, rotation: e.rotation, scale_xy: e.scale_xy, scale_z: e.scale_z, lod_dist: e.lod_dist, room: e.room, set: e.entity_set }).collect(),
        rooms: s.rooms.iter().map(|r| mlo::InRoom { hash: r.hash, name: r.name.clone(), bounds: r.bounds }).collect(),
        portals: s.portals.iter().map(|p| mlo::InPortal { from: p.from, to: p.to, corners: p.corners }).collect(),
        portals_complete: s.portals_complete,
        sets: s.entity_sets.iter().map(|x| (x.hash, x.name.clone())).collect(),
    }
}

/// 6m surface id -> RDR2 materials.meta name, from `sixng surfaces`
/// (<work>/collision/levels/surface_categories.json); empty without it.
fn surface_names(work: &Path) -> HashMap<u16, &'static str> {
    let p = work.join("collision").join("levels").join("surface_categories.json");
    let Some(doc) = std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else {
        eprintln!("redm: {} missing (run `6ixpack sixng surfaces levels --navmeshes none --out <work>`): all collision is DEFAULT", p.display());
        return HashMap::new();
    };
    doc["surfaces"].as_object().map(|m| m.iter().filter_map(|(k, v)| Some((u16::from_str_radix(k, 16).ok()?, mlo::material_name(v["category"].as_str()?)))).collect()).unwrap_or_default()
}

/// Distance from `p` to triangle (a, b, c).
fn point_tri(p: [f32; 3], a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    let s = |x: [f32; 3], y: [f32; 3]| [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
    let d = |x: [f32; 3], y: [f32; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    // closest point (Ericson, Real-Time Collision Detection 5.1.5)
    let (ab, ac, ap) = (s(b, a), s(c, a), s(p, a));
    let (d1, d2) = (d(ab, ap), d(ac, ap));
    let q = if d1 <= 0.0 && d2 <= 0.0 {
        a
    } else {
        let bp = s(p, b);
        let (d3, d4) = (d(ab, bp), d(ac, bp));
        let cp = s(p, c);
        let (d5, d6) = (d(ab, cp), d(ac, cp));
        let (vc, vb, va) = (d1 * d4 - d3 * d2, d5 * d2 - d1 * d6, d3 * d6 - d5 * d4);
        let lerp = |x: [f32; 3], e: [f32; 3], t: f32| [x[0] + e[0] * t, x[1] + e[1] * t, x[2] + e[2] * t];
        if d3 >= 0.0 && d4 <= d3 {
            b
        } else if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
            lerp(a, ab, d1 / (d1 - d3))
        } else if d6 >= 0.0 && d5 <= d6 {
            c
        } else if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
            lerp(a, ac, d2 / (d2 - d6))
        } else if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
            lerp(b, s(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6)))
        } else {
            let den = 1.0 / (va + vb + vc);
            let (v, w) = (vb * den, vc * den);
            [a[0] + ab[0] * v + ac[0] * w, a[1] + ab[1] * v + ac[1] * w, a[2] + ab[2] * v + ac[2] * w]
        }
    };
    let e = s(p, q);
    d(e, e).sqrt()
}

/// Rotation convention check on the converted entities with a non-identity
/// rotation: their LOD0 vertex samples placed in interior space with the
/// orientation taken as conj(stored) (copy is right: the RDR2 convention)
/// and as stored itself, then the median distance to the interior's
/// collision triangles. The convention whose placement hugs the collision
/// is the 6m one.
fn rotation_check(src: &fivem::InteriorSource, converted: &HashMap<u32, String>, samples: &HashMap<String, Vec<[f32; 3]>>) -> Option<Value> {
    let c = src.collision.as_ref()?;
    let t = &c.all;
    let tri = |k: usize| t.triangles[k].map(|i| t.vertices[i as usize]);
    let near = |p: [f32; 3]| (0..t.triangles.len()).map(|k| { let [a, b, cc] = tri(k); point_tri(p, a, b, cc) }).fold(f32::MAX, f32::min);
    let (mut da, mut db, mut n) = (Vec::new(), Vec::new(), 0);
    for e in &src.entities {
        let r = e.rotation;
        if (r[3].abs() - 1.0).abs() < 1e-4 {
            continue;
        }
        let Some(vs) = converted.get(&e.archetype).and_then(|nm| samples.get(nm)) else { continue };
        n += 1;
        for (q, out) in [(mlo::conj(r), &mut da), (r, &mut db)] {
            for v in vs.iter().step_by(vs.len().div_ceil(40).max(1)) {
                let s = [v[0] * e.scale_xy, v[1] * e.scale_xy, v[2] * e.scale_z];
                let w = mlo::qrot(q, s);
                out.push(near([w[0] + e.position[0], w[1] + e.position[1], w[2] + e.position[2]]));
            }
        }
    }
    let rotated = src.entities.iter().filter(|e| (e.rotation[3].abs() - 1.0).abs() >= 1e-4).count();
    let med = |v: &mut Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v.get(v.len() / 2).copied()
    };
    let mean = |v: &[f32]| (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32);
    let far = |v: &[f32]| v.iter().filter(|&&d| d > 0.1).count();
    let (fa, fb) = (far(&da), far(&db));
    let (ea, eb) = (mean(&da), mean(&db));
    let (ma, mb) = (med(&mut da), med(&mut db));
    let summary = match (ma, mb, ea, eb) {
        (Some(a), Some(b), Some(x), Some(y)) => format!(
            "{n} converted entities with a rotation ({rotated} rotated in the interior), {} vertex samples each way: distance to the collision with orientation = conj(stored) (straight copy, RDR2 convention) median {a:.3} m, mean {x:.3} m, {fa} samples > 0.1 m; with orientation = stored median {b:.3} m, mean {y:.3} m, {fb} samples > 0.1 m; {}",
            da.len(),
            if x < y && fa <= fb { "the straight copy fits" } else if y < x && fb <= fa { "the conjugate fits better: CHECK" } else { "inconclusive" }
        ),
        _ => format!("no converted entity with a rotation ({rotated} rotated in the interior): not checked"),
    };
    Some(json!({"converted_rotated": n, "rotated": rotated, "median_conj_stored": ma, "median_stored": mb, "mean_conj_stored": ea, "mean_stored": eb, "far_conj_stored": fa, "far_stored": fb, "summary": summary}))
}

fn clone_tex(t: &ytd::Texture) -> ytd::Texture {
    let x = &t.tex;
    let tex = gta5::ytd::Tex { name: x.name.clone(), width: x.width, height: x.height, levels: x.levels, format: x.format, data: x.data.clone(), unk32: x.unk32, usage_data: x.usage_data, extra_flags: x.extra_flags };
    ytd::Texture { tex, srgb: t.srgb }
}

/// Resource manifest in the layout of the user's working custom interiors
/// (MLO spec section 7): everything streams from stream/, `this_is_a_map`,
/// no `data_file` lines (the interior ytyp loads through the map).
fn fxmanifest(int: &str) -> String {
    format!(
        "fx_version 'cerulean'
game 'rdr3'
rdr3_warning 'I acknowledge that this is a prerelease build of RedM, and I am aware my resources *will* become incompatible once RedM ships.'
lua54 'yes'
this_is_a_map 'yes'

-- 6m interior {int}: stream/{int}.ytyp (prop archetypes + the MLO), {int}_milo_.ymap
-- (placement), {int}.ybn + hi@{int}.ybn (collision), the props' .ydr and shared
-- {int}_txd*.ytd, and stream/_manifest.ymf (interior data dependencies and the
-- collision pair; RedM is reported to crash near a streamed interior without it).
"
    )
}

/// `--at x,y,z[,heading]`.
fn parse_at(s: &str) -> Result<[f32; 4], String> {
    let v: Vec<f32> = s.split(',').map(|x| x.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|e| format!("--at {s}: {e}"))?;
    match v[..] {
        [x, y, z] => Ok([x, y, z, 0.0]),
        [x, y, z, h] => Ok([x, y, z, h]),
        _ => Err(format!("--at {s}: want x,y,z[,heading]")),
    }
}
fn r3(v: [f32; 3]) -> Value {
    json!(v.map(|x| (x * 1000.0).round() / 1000.0))
}

/// interior.json (debug dump); returns the per-archetype report lines.
fn write_debug(out: &Path, src: &fivem::InteriorSource, outcome: &BTreeMap<String, Outcome>, txds: &[(String, usize, u64)]) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    let mut archs = Vec::new();
    for (n, o) in outcome {
        match o {
            Outcome::Skipped(why) => {
                lines.push(format!("  skipped   {n}: {why}"));
                archs.push(json!({"name": n, "converted": false, "reason": why}));
            }
            Outcome::Converted { tris, lod_tris, textures, ydr, txd } => {
                let tn = &txds[*txd].0;
                lines.push(format!("  converted {n}: {tris} triangles (LODs {lod_tris:?}), {textures} textures in {tn}, ydr {ydr} B"));
                archs.push(json!({"name": n, "converted": true, "triangles_lod0": tris, "triangles_per_lod": lod_tris, "textures": textures, "texture_dictionary": tn, "ydr_bytes": ydr}));
            }
        }
    }
    let col = src.collision.as_ref().map(|c| {
        let mut by: BTreeMap<String, usize> = BTreeMap::new();
        for m in &c.materials {
            *by.entry(format!("{m:04x}")).or_default() += 1;
        }
        let mut kinds = [0usize; 4];
        for p in &c.primitives {
            use sixng_core::bounds::NativePrim as P;
            kinds[match p {
                P::Box { .. } => 0,
                P::Sphere { .. } => 1,
                P::Capsule { .. } => 2,
                P::Cylinder { .. } => 3,
            }] += 1;
        }
        json!({"bounds": c.id, "link": c.method, "confidence": c.confidence, "space": "interior", "vertices": c.vertices.len(), "triangles": c.triangles.len(),
            "primitives": {"box": kinds[0], "sphere": kinds[1], "capsule": kinds[2], "cylinder": kinds[3]},
            "triangles_by_surface_id": by, "surface_ids": c.material_records.keys().map(|k| format!("{k:04x}")).collect::<Vec<_>>(), "bbox": [r3(c.bbox[0]), r3(c.bbox[1])]})
    });
    let doc = json!({
        "note": "DEBUG dump of `redm interior`, not read by RedM. Interior space unless noted. Entity `rotation` is x,y,z,w as stored in the 6m CEntityDef (the inverse of the entity's orientation, GTA V convention); placement `rotation` is the CMloInstanceDef rotation as stored (the interior's world orientation, not inverted). Indices: room.entities / entity_set.entities index `entities`; entity.room / entity_set index `rooms` / `entity_sets`; portal from/to index `rooms`.",
        "interior": src,
        "collision": col,
        "texture_dictionaries": txds.iter().map(|(n, k, b)| json!({"name": n, "textures": k, "pixel_bytes": b})).collect::<Vec<_>>(),
        "archetypes": archs,
    });
    write(&out.join("interior.json"), &serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?)?;
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gta5::ytd::{Tex, TexFormat};

    fn tex(name: &str, w: u16, seed: u8) -> ytd::Texture {
        let mut t = Tex { name: name.into(), width: w, height: w, levels: 1, format: TexFormat::DXT1, data: Vec::new(), unk32: 0, usage_data: 0, extra_flags: 0 };
        t.data = (0..t.mip_chain_len()).map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed)).collect();
        ytd::Texture { tex: t, srgb: true }
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn pool_dedups_by_content() {
        let mut p = TexPool::default();
        let (a, ra) = p.add(vec![tex("r6m_a", 16, 1), tex("r6m_b", 16, 2)]);
        assert_eq!((a, ra.len()), (vec![0, 1], 0));
        // same pixels under another name: stored once, renamed to the first name
        let (b, rb) = p.add(vec![tex("r6m_c", 16, 1)]);
        assert_eq!(b, vec![0]);
        assert_eq!(rb.get("r6m_c").map(String::as_str), Some("r6m_a"));
        // same name, other pixels: a second texture with a suffix
        let (c, rc) = p.add(vec![tex("r6m_a", 16, 9)]);
        assert_eq!(c, vec![2]);
        assert_eq!(rc.get("r6m_a").map(String::as_str), Some("r6m_a_1"));
        assert_eq!(p.texs.len(), 3);
        // colour space is part of the identity
        let mut lin = tex("r6m_d", 16, 1);
        lin.srgb = false;
        assert_eq!(p.add(vec![lin]).0, vec![3]);
    }

    #[test]
    fn renames_reach_every_slot() {
        let g = gta5::ydr::Geom { texture: "r6m_C".into(), normal: "r6m_c".into(), spec: "r6m_x_ma".into(), ..Default::default() };
        let mut l = vec![gta5::ydr::Mesh { name: "m".into(), geoms: vec![g] }];
        let ren: HashMap<String, String> = [("r6m_c".to_string(), "r6m_a".to_string())].into();
        rename_textures(&mut l, &ren);
        let g = &l[0].geoms[0];
        assert_eq!((g.texture.as_str(), g.normal.as_str(), g.spec.as_str()), ("r6m_a", "r6m_a", "r6m_x_ma"));
    }

    #[test]
    fn pack_shares_and_caps() {
        // textures 0..4 of 4 bytes each; models 0 and 1 share texture 1
        let sizes = [4u64; 5];
        let models = vec![vec![0, 1], vec![1, 2], vec![3, 4]];
        let (t, of) = pack(&models, &sizes, 12);
        // 0 and 1 together (12 bytes, texture 1 once), 2 alone: 3 more would exceed the cap
        assert_eq!(of[0], of[1]);
        assert_ne!(of[0], of[2]);
        assert_eq!(t[of[0]].bytes, 12);
        assert!(t.iter().all(|d| d.bytes <= 12));
        // no cap pressure: one dictionary, every texture once
        let (t, of) = pack(&models, &sizes, 1 << 20);
        assert_eq!((t.len(), t[0].bytes, of), (1, 20, vec![0, 0, 0]));
        // a model over the cap gets its own dictionary
        let (t, of) = pack(&[vec![0, 1, 2], vec![3]], &sizes, 8);
        assert_eq!(t.len(), 2);
        assert_eq!(t[of[0]].bytes, 12);
        // every model's textures are in its dictionary
        for (m, d) in [vec![0, 1, 2], vec![3]].iter().zip(&of) {
            assert!(m.iter().all(|x| t[*d].textures.contains(x)));
        }
    }

    #[test]
    fn fit_model_drops_top_mips() {
        let mut big = tex("r6m_big", 256, 1);
        big.tex.levels = 7; // 256 .. 4
        big.tex.data = (0..big.tex.mip_chain_len()).map(|i| i as u8).collect();
        let mut p = TexPool::default();
        let (ids, _) = p.add(vec![big, tex("r6m_small", 16, 2)]);
        let total = |p: &TexPool| ids.iter().map(|&i| ytd::stored_size(&p.texs[i]).unwrap()).sum::<u64>();
        let before = total(&p);
        let n = fit_model(&mut p, &ids, before / 2).unwrap();
        assert!(n >= 1 && total(&p) <= before / 2);
        assert_eq!(p.texs[ids[0]].tex.width, 256 >> n);
        // a set that cannot shrink (single mips) is left as is
        assert_eq!(fit_model(&mut p, &ids[1..], 1).unwrap(), 0);
    }

    #[test]
    fn stored_size_matches_written_dictionary() {
        let t = tex("r6m_q", 64, 3);
        let n = ytd::stored_size(&t).unwrap();
        let b = ytd::build_ytd(&[t]).unwrap();
        let d = read::ytd(&b).unwrap();
        assert_eq!(n, d.textures[0].block_count as u64 * d.textures[0].block_stride as u64);
    }

    #[test]
    fn args() {
        let a = parse(&s(&["--list", "--work", "w"])).unwrap();
        assert_eq!((a.list.as_deref(), a.work.as_deref()), (Some(""), Some(Path::new("w"))));
        let a = parse(&s(&["--list", "INT_5B", "--game", "g"])).unwrap();
        assert_eq!((a.list.as_deref(), a.game.as_deref()), (Some("int_5b"), Some(Path::new("g"))));
        let a = parse(&s(&["int_5B903326", "--game", "g", "--work", "w", "--out", "o", "--max-tex", "512", "--default-only"])).unwrap();
        assert_eq!(a, Args { name: Some("int_5b903326".into()), list: None, game: Some("g".into()), work: Some("w".into()), out: Some("o".into()), max_tex: Some(512), default_only: true, at: None, no_manifest: false });
        let a = parse(&s(&["x", "--at", "1,2,3,90", "--no-manifest"])).unwrap();
        assert_eq!((a.at, a.no_manifest), (Some([1.0, 2.0, 3.0, 90.0]), true));
        assert!(parse(&s(&["x", "--at", "1,2"])).is_err());
        assert!(parse(&s(&["a", "b"])).is_err());
        assert!(parse(&s(&["--out"])).is_err());
        assert!(parse(&s(&["--list", "x", "name"])).is_err());
        assert!(parse(&s(&["--max-tex", "big"])).is_err());
    }

    #[test]
    fn names() {
        assert_eq!(fivem::interior_hash("int_5B903326"), Some(0x5b90_3326));
        assert_eq!(fivem::interior_hash("5b903326"), Some(0x5b90_3326));
        assert_eq!(fivem::interior_hash("some_interior"), Some(gta5::rsc::joaat("some_interior")));
        assert_eq!(fivem::interior_hash(" "), None);
        assert_eq!(fivem::interior_name(0x5b90_3326), "int_5b903326");
        assert!(is_model_name("prop_bench_01a") && is_model_name(&"a".repeat(63)));
        assert!(!is_model_name("") && !is_model_name(&"a".repeat(64)) && !is_model_name("Prop") && !is_model_name("prop-1") && !is_model_name("prop.x"));
        let ar = vec!["levels.rpf".to_string()];
        assert_eq!(fivem::interior_resolvable(Some("levels.rpf@10#m"), &ar), Ok("levels.rpf@10#m".into()));
        assert!(fivem::interior_resolvable(None, &ar).is_err());
        assert!(fivem::interior_resolvable(Some("pack/story000/story000.rpf@10"), &ar).unwrap_err().contains("story000"));
        assert_eq!(output_name(0xab, Some("Prop_A")), "prop_a");
        assert_eq!(output_name(0xab, None), "r6m_000000ab");
        assert_eq!(output_name(0xab, Some("arch_000000ab")), "r6m_000000ab");
        assert_eq!(output_name(0xab, Some("bad-name")), "r6m_000000ab");
        assert!(is_model_name(&output_name(0xdead_beef, None)));
    }
}
