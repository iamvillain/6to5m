# 6to5m: GTA VI to GTA V / FiveM converter (CLI + MCP server)

**6to5m** is a pure-Rust **command-line tool and MCP server** that extracts **GTA VI (Xbox Series X|S version)** RPF9 archives and converts maps, textures, models and collision into **GTA V / FiveM** formats (ydr, ytd, ybn, ymap, ytyp, water.xml) as ready-to-stream FiveM resources. The binary is `6ixpack`.

Keywords: GTA 6, GTA VI, RPF9, RAGE, BCPack, Xbox Series X, GTA V, FiveM, CodeWalker, ydr, ytd, ybn, ymap, MLO, map conversion, modding, MCP, Model Context Protocol, AI agents, Rust.

## For AI agents: MCP server

Run `6ixpack mcp` as a stdio MCP server, and every CLI command becomes an MCP tool: `extract`, `audio`, `texture`, `model`, `convert`, `sixng`, `fivem`, `redm`. Each tool takes the same args as the CLI; pass `["--help"]` to discover options.

```json
{ "mcpServers": { "6to5m": { "command": "C:/path/to/6ixpack.exe", "args": ["mcp"] } } }
```

Claude Code: `claude mcp add 6to5m -- C:/path/to/6ixpack.exe mcp`

## CLI quick start

```
cargo build --release                       # -> target/release/6ixpack.exe (pure Rust, no DLLs)
6ixpack levels.rpf out/                     # extract an RPF9 archive
6ixpack fivem build --game <GTA VI dir> --out <resource> --bbox x0,y0,x1,y1   # map area -> FiveM resource
6ixpack fivem vehicle chino --game <GTA VI dir> --out <resource>              # one vehicle -> FiveM vehicle resource
6ixpack fivem weapon w_pi_scorpio_01 --game <GTA VI dir> --out <resource>       # guns -> FiveM add-on weapon resource (`all` for every gun)
6ixpack fivem ped cs_lucia --game <GTA VI dir> --out <resource>               # one ped -> FiveM ped resource (--list to preview)
6ixpack sixng index levels --game <GTA VI dir> --work <work>                  # once: levels.rpf index (~5 min)
6ixpack redm prop prop_am_bench_26a --game <GTA VI dir> --work <work> --out <resource>  # one prop -> RedM (RDR2) resource
6ixpack redm prop --list bench --work <work>                                  # find prop names
6ixpack redm prop prop_am_bench_int_01a --tints --game <GTA VI dir> --work <work>          # list its tint (colour) rows
6ixpack redm prop prop_am_bench_int_01a --tint-row 5 --as prop_am_bench_int_01a_blue ..    # a colour variant as its own model
6ixpack redm interior --list --work <work> --game <GTA VI dir>               # 6m interiors (needs the sixng passes below)
6ixpack redm interior int_d4c876ae --game <GTA VI dir> --work <work> --out <resource> [--at x,y,z,heading]  # interior -> RedM MLO resource
6ixpack redm convert ytd <dds-dir> out.ytd                                    # DDS -> RDR2 texture dictionary
6ixpack redm convert ytyp props.json out.ytyp                                 # archetype list -> RDR2 .ytyp (shape: redm --help)
```

## Status

What works today:

- **Extraction**: RPF9 archive scanning and carving, names, placements, interiors, lights, paths, navmesh, collision, audio (Opus/Ogg/WAV), Bink movies, PSO/meta to JSON, vehicles and handling data.
- **Textures**: RAGE pools and TTPF stores to DDS/PNG; BCPack (Xbox hardware compression) decoded in pure Rust for BC1, BC3, BC4, BC5 and BC7 (BC6H partly).
- **Models**: drawables and fragments to OBJ/GLB.
- **GTA V / FiveM** (`6ixpack fivem build`): a ready resource for any map area.
  - `6ixpack fivem vehicle <name>`: a 6m vehicle (`<name>`, `_mid`, `_hi` fragments) to `<model>.yft`, `<model>_hi.yft` and `<model>.ytd` (GTA V bone names, skeleton pruned to 255 bones, rigid parts per bone, box physics for chassis and 4 wheels), plus vehicles/handling/carvariations/carcols meta and fxmanifest. With `6ixpack sixng vehicles --out <work>` data it uses the real rim, tyre, wheelScale and body colour. Doors stay shut.
  - `6ixpack fivem weapon <name|all>`: 6m modular guns (data.rpf base `w_<class>_<name>_<nn>` plus its parts, assembled at the `WAP_`/`AAP_` sockets with the first variant per slot) to `<model>.ydr` (GTA V weapon skeleton: Gun_Root, Gun_Muzzle/Nuzzle, Gun_GripR/L, Gun_VFX_Eject, WAPClip/Supp/Scop/FlshLasr/Grip) and `<model>.ytd`, plus weaponarchetypes/weapons/weaponanimations meta and fxmanifest. Stats, anims and audio are GTA V defaults per class; melee and thrown items are skipped. Weapon shaders have no colour map (normal + packed material only), so the colour map is generated from the packed material-id map (gunmetal / polymer finishes per id).
  - `6ixpack fivem ped <name>`: every packs.rpf drawable named `<name>_*` becomes a component (`head_000_u`, `uppr_000_u`, `jbib_000_u`, ...) of `<ped>.ydd`, skinned onto `<ped>.yft`, the standard GTA V ped skeleton (SKEL_ROOT, GTA V bone names and tags) with the 6m joint positions; plus `<ped>.ytd`, `<ped>.ymt`, peds.meta and fxmanifest. Diffuse only (flat normal/spec maps), one texture per drawable. Parts follow SixNG's default look (`--all` for every part). GM_ helper bones (girth, cull) skin to the body bone their name gives instead of being dropped. `--hair` / `--beard <groom>` add strand-hair grooms as cards; `--grooms` lists the grooms that fit the head.
  - Terrain tiles, water and buildings/props as ydr, with ytd textures re-encoded to DXT1/DXT5.
  - Textures picked by 6m slot hash (normal, mask and flat maps rejected), at their native 6m size.
  - Road and apron tiles: one opaque base layer plus alpha-blended decal layers, with the terrain sunk underneath.
  - Draw distances: big buildings never vanish before the props on them; small clutter streams only within 40-100 m.
  - One pass, no extraction: assets are read from the 6m .rpf at their offsets, converted in memory and written straight into `--out`; the output itself is the cache (fingerprint manifest, per-stage versions), so unchanged files are skipped and FXServer keeps its hashes.
  - Textures ship in their native block format; oversized dictionaries chain into parent ytds.
  - Texture dictionaries share common textures through a gtxd.meta parent chain.
  - Correct render buckets (opaque, alpha, decal, cutout).
  - LOD/SLOD chains (or `--no-lod`).
  - Static collision as ybn (welded, deduplicated, weapon-only shapes dropped unless `--weapon-collision`).
  - Water only over low or empty ground (no ocean ambience under the land); stale outputs pruned on a full build.
  - Texture records versioned per BC codec, so a decoder fix re-decodes only that format.
  - Every output is checked against a CodeWalker loader.
- **RDR2 / RedM** (`6ixpack redm`, first slice): RSC8 `.ytd` (v2) and static `.ydr` (v162) writers in pure Rust, `redm convert ytd|ydr` from DDS / OBJ, and `redm prop <name>` (one 6m drawable or fragment main drawable, all its LOD levels, `default` shader or `standard` with the real 6m normal map and an `_ma` material map made from the 6m speculartex; `_mb` is the vanilla blank) to a RedM resource. A `.ytyp` archetype (RSC8 meta, static prop: name = ydr stem, bounds from the drawable) is written with the prop and listed as `DLC_ITYP_REQUEST` in `fxmanifest.lua`, so the model is meant to spawn with `CreateObject(joaat(name))`; `redm convert ytyp <json>` builds one without game files. Tinted props: `--tints` lists the rows of the embedded colour palette, `--tint-row <n>` bakes one (exact colours), `--as <model>` writes it under another model name so several colour variants run side by side. Files parse in CodeX and match CodeX-written reference files (ytyp: byte-identical apart from vanilla-style bbMin.w/bbMax.w); not yet loaded in RedM, and no collision yet.
  - `redm interior` (MLO written, untested in game): `--list` shows every 6m interior (rooms, portals, entities, resolvable entities, linked collision, position); `redm interior <int_hash>` converts every entity archetype that has a model id with the `redm prop` path (`stream/<archetype>.ydr`, textures deduplicated into shared `<int>_txd<N>.ytd` under 14 MiB of pixel data each) and writes `<int>.ytyp` (prop archetypes + the `CMloArchetypeDef`: rooms, portals, the converted entities, entity sets; indices remapped), `<int>_milo_.ymap` (`CMloInstanceDef` at the 6m placement or `--at`), the collision pair `<int>.ybn` + `hi@<int>.ybn` (RSC8 v47, GeometryBVH built the CodeX way; primitives tessellated; 6m surfaces -> RDR2 materials via `sixng surfaces`) `_manifest.ymf` (PSO CPackFileMetaData as the working custom interiors have it: milo ymap -> interior ytyp and the ytyp itself as INTERIOR_DATA, interior bounds `[hi@<int>, <int>]`; schema section embedded from a working manifest) and `fxmanifest.lua` (`this_is_a_map`, no `data_file` lines), plus `interior.json` (debug dump). All RDR2 meta files come from one generic meta writer driven by the class/enum tables of the MLO spec (`src/rdr2/meta_defs.rs`, generated); data blocks match the spec's CodeX goldens byte for byte, ybn vertices/polygons/BVH too. Work data (levels.rpf): `sixng collision levels --out <work>` (26 s), `sixng interiors levels --out <work>` (48 s), `sixng placements levels --id-file <interior files> --out <work>` (1 s), `sixng surfaces levels --navmeshes none --out <work>` (8 s); `sixng link-archetypes levels` (75 s) optional. Only ~6% of interior entities have a model id in the indexed archives (levels.rpf carve covers 5.8% of the archive), so most entities are dropped.

## Roadmap

- [x] BCPack BC4
- [ ] BCPack BC6H: preset endpoints refitted per GTA6-Asset-Info (7f3e1d3), mip-check bad blocks 12.7% -> 7.3%; BC7 and BC6H now decode through upstream SixNG's fitted rebuilders (5c21d7d, d4b14fa). Live re-measure pending
- [x] Road and asphalt surfaces
- [x] Four-layer roads and ground: terrain_cb_w_4lyr.sps with the four 6m layer diffuses, lookup weights baked into Colour1 (no layer bump maps yet)
- [x] LOD/SLOD drawables with no linked model (unresolved archetypes 84,216 to 37; `#member` model ids resolved, so every airport placement converts)
- [x] Placements in `story000` (10.8k entities) and `shared_minigame000` (20.3k): per-archive sixng LOD/link files, placed with archive-aware model offsets when indexed (their MLO interiors not yet); levels.rpf placements resolve 104,377/104,379
- [x] Interiors as GTA V MLOs: `t6m_int.ytyp` (CMloArchetypeDef with decoded rooms, entity sets and the interior entities) and `t6m_int_X_Y.ymap` (CMloInstanceDef); `--interiors` still flattens them into world space instead
- [x] MLO portals decoded from 6m (`995072ca`) and entities attached to their rooms (`T6M_MLO_LIMBO=1` puts them all in limbo); entity sets start off
- [x] Room ids in interior collision (so the game knows the player is inside a room)
- [x] MLO lights: 6m light sets of each interior entity (archetype + entity, decoded during the build, no extra pass) as CExtensionDefLightEffect; intensity and capsule extents copied 1:1 (`T6M_LIGHT_SCALE`); MLO lodDist 60-200 m so interior contents only stream near the player
- [x] Tint palettes: the embedded palette entry (vertex colour blue, row 0: tintValue-as-row renders wrong, 5849b7b) is baked into tinted copies of the colour texture (32-step colours, max 512 px); the palette is no longer picked as a colour map
- [x] Quality audit: UVs no longer V-flipped (6m is DirectX top-left), COLOR0 mapped to GTA V ambient (r,r,255,255) so buildings are not black under artificial light
- [x] Buildings use `normal_spec.sps` with their real 6m normal maps (7624a1a)
- [x] Colour textures picked by 6m slot hash, glass on glass_env with its real diffuse, texture size from texel density (77ad6e2)
- [x] standard_dirt grime layer (diffusetex2) as a COLOR0.a-gated decal pass, using the vendored SixNG ShaderConst reader (1fd492e)
- [x] Decals keep 6m vertex alpha (edge fades, 9c4c3cd)
- [x] Collision in 128 m cells, welded 5 cm and simplified per material (fa3e66f)
- [x] Per-placement tint rows: placements with `tintValue` > 0 use an archetype variant `<name>__tint<row>`, shared by every placement with that row
- [x] Prop batch per-instance tint (`ba049770` +0x34 RGBA): multiplies the base colour via tinted texture copies (fivem/batches.rs bake)
- [x] Fragments with physics: fragment props become dynamic `.yft` (one rigid piece, one physics box per 6m collider placed by its group bone, 6m masses; non-box collider types use their bound box; breakable multi-part props stay one piece)
- [x] Peds to GTA V: `6ixpack fivem ped <name>` (skinned ydd components, yft skeleton retargeted to the GTA V ped skeleton, ytd, ymt, peds.meta; loads in CodeWalker, in-game test pending)
- [x] Vehicles to GTA V: `6ixpack fivem vehicle <name>` (yft + vehicles/handling/carvariations/carcols metas; in-game test pending)
- [x] Weapons to GTA V formats: `6ixpack fivem weapon <name|all>` (ydr/ytd + weapons.meta; in-game test pending)
- [x] gtxd.meta parent chain checked by every `fivem build` (one parent each, no cycles, every txd in stream/, each drawable's textures found in its txd chain, no texture stored twice); in-game check pending
- [x] Road paths to .ynd (AI traffic): `fivem build` part `paths`, whole 512 m node areas over the bbox, links only into written areas
- [x] Navmesh to .ynv (ped/AI pathing): `fivem build` part `navmesh`, 6m tiles clipped into 150 m cells, decimated and merged under the 16k-polygon cap, edge adjacency across written cells
- [x] Lights to ymap LODLights / DistantLODLights (night lighting): exterior lights (entity + archetype sets) as `t6m_lodlights_X_Y` / `t6m_distlights_X_Y`, packed as CodeWalker; the archetype-to-meta-file index is built in the interior-ents pass (not yet checked in CodeWalker or in game)
- [x] Instanced batches: grass to ymap `GrassInstanceList`s (`t6m_grass_<x>_<y>.ymap`), prop batches (vegetation) to merged per-batch drawables
- [x] Minimap and zone names: the region's terrain as shaded relief in the six radar tiles `minimap_sea_<row>_<col>.ytd` (4500 m tiles from (-4140, 8400), fitted against vanilla); 512 m zones named after the dominant 6m archetype prefix (`<work>/zone_names.json` renames them), shown by `t6m_zones.lua`. Not yet tested in game
- [x] Split output into several resources by region: `--split <metres>` writes `<out>/[<name>]/` with one resource per region plus `<name>_common` (ytyps, LOD/SLOD ymaps, shared txds, terrain, water) and an `ensure.cfg`; regions over `--split-max <MB>` (default 1024) are quartered. Not yet tested in game
- [x] 6m LOD data: drawable LOD levels in the ydr LodGroup (Med/Low), batch LOD parents (fixes ORPHANHD), lodDist from 6m values, txds capped at 14 MB physical with small-family packing, state-variant duplicates dropped (548c6a9, 4064fb0). Not yet tested in game
- [x] yft "Invalid fixup" crash fixed at the root: fragment composite bounds no longer write child flag arrays (GTA V fragments carry none); `--verify` walks every yft pointer (eaa7ef9). In-game retest pending
- [x] Textures: one flat txd per model family (6m shares textures by global hash, no parent chain; c737ddc); tint palette fill entry (255,187,255) read as untinted, fixing magenta glass, walls and skids (dae5aa9). In-game retest pending
- [x] Collision keeps 6m's native box/sphere/capsule/cylinder bounds as ybn primitives, not tessellated meshes (4946197); fragment placement checked against 6m colliders (045e440); batch index reads system pages only (9fb4b56). Not yet tested in game
- [x] Four-layer terrain in GTA V's own terrain_cb_w_4lyr layout: 8 samplers (diffuse + bump per layer, 6m layer normal maps), GTA V vertex colour layout, SixNG UV tiling (225ea7f, 3ac1e89); fixes the beige apron. Not yet tested in game
- [x] Texture identity root fix: decode.rs read each texture's hash from the previous record (wrong neighbour textures); texture cache keyed by 64-bit identity (f326434). Not yet tested in game
- [x] Collision surface materials: 6m surface ids map to GTA V materials (concrete, tarmac, gravel, dirt, sand, marsh, soil, grass, rock) through `sixng surfaces` categories; needs `<work>/collision/levels/surface_categories.json`, else DEFAULT. Not yet tested in game
- [x] Coordinate offset: `--offset dx,dy[,dz]` moves every output (ymaps, terrain/collision ybns, interiors, water, minimap, zones); `--bbox` stays in 6m coordinates. GTA VI reaches y 9474, past GTA V's ~±8000
- [x] Los Santos removal in Rust (part `lsremove`): reads GTA V's RPF7 archives (pure-Rust NG/AES decrypt; keys from GTA5.exe + CodeWalker's magic.dat, cached in `<work>/gta5_keys.bin`), indexes the 8,666 ymaps and 10,837 ybns of _citye/_cityw/_hills incl. DLCs once, and writes empty stand-ins for those seen from the region. The ybn stand-ins are byte-identical, so they share one file in the output cache. Not yet tested in game
- [x] One-pass conversion: 6m .rpf straight into the resources dir given by `--out` (CLI and MCP argument; e.g. `server-data/resources/[t6m]`), and that output is the cache (fingerprint manifest `<work>/index/fivem_manifest.json`; unchanged assets skipped); no extract step, intermediates or build dir. Output cache removed (584be29); a `[name]` --out is written in place, split moves files between its resources (renames, no links or build dir). source textures decoded in memory, records only (8515490); placements, collision, LOD/interior tables and batches held in memory via sixng vfs (eaa4370): only scan indexes and fingerprint/texture records on disk

## Contributing

Fork it, make it better, send a PR. Humans and AI agents both welcome.

Agents: start from `TODO.md`, stay 100% Rust, never commit game files, and credit any code you borrow.

## Build

Pure Rust, no C code, DLLs or external tools. `cargo build --release` → `target/release/6ixpack.exe`.

## Use

```
6ixpack [-n] [-v] <file.rpf> [outdir]   scan and extract payloads (cached scan index)
6ixpack audio   <args...>               AWC/ADAT audio: list, Opus, Ogg, WAV
6ixpack texture <args...>               RAGE textures and TTPF stores -> DDS/PNG (BCPack decodes)
6ixpack model   <args...>               drawables -> OBJ (cached model walk)
6ixpack sixng   <args...>               placements, vehicles, GLB, names, interiors, movies, ...
6ixpack convert <kind> ...              GTA V files: ytd, ydr, ybn, ymap, ytyp
6ixpack fivem build --game .. --out ..   ready FiveM map resource (terrain, buildings, water)
6ixpack fivem vehicle <name> ..          FiveM vehicle resource from one 6m vehicle
6ixpack fivem weapon <name|all> ..       FiveM weapon resource from 6m guns
6ixpack fivem ped <name> ..              FiveM ped resource from one 6m ped (cs_lucia, NPCs)
6ixpack redm convert ytd|ydr|ytyp ..    RDR2 (RSC8) ytd from DDS, ydr from OBJ, ytyp from JSON
6ixpack redm prop <name> ..              RedM resource from one 6m prop (--list <pattern>)
6ixpack redm interior <name> ..          RedM MLO resource from one 6m interior (--list)
6ixpack redm check <file>                re-read an RDR2 ytd/ydr/ytyp and print its structure
6ixpack mcp                             stdio MCP server: extract, audio, texture, model, convert, sixng, fivem, redm tools
```

Add `--help` to any command. `--threads N` works everywhere.

## Credits

Built on the work of:

- **offs** — [rpf9-extract](https://github.com/offs/rpf9-extract) (no license published): the archive scanner, carver and report code this tool started from.
- **breadbbbb** — [rpf9-explorer](https://github.com/breadbbbb/rpf9-explorer) (no license published): model, texture and audio decoding references; the 1.2 model recovery (zero-radius drawables, empty-bounds geometry fallback, optional fragment header pointers, 16-bit bone palettes) is ported in `src/model/decode.rs`.
- **holdmysocks** — [SixNG](https://github.com/holdmysocks/SixNG) (GPL-3.0): vendored in `vendor/sixng-core` and the `sixng` command (plus its BC6H one-region y-state rule, a4063d3); synced to 0.5.0 for drawable-dictionary members (per-member model names, bounds and exports). Four-layer ground shaders (layer slots, lookup-weight channels, detail tiling; 6046d19, 1456a9b) drive the fivem terrain_cb_w_4lyr roads and ground.
- **ShrewBrew** — [OpenRPF9](https://github.com/shrewbrewsh/OpenRPF9) (MIT): stream-carving approach.
- **rpf9-audio contributors** — [rpf9-audio](https://github.com/jaylikesbunda/rpf9-audio) (MIT): AWC/ADAT and Ogg Opus handling.
- **opus-decoder** (BSD-3-Clause, port of libopus): built-in Opus decoding.
- **dexyfex** — [CodeWalker](https://github.com/dexyfex/CodeWalker) (MIT): GTA V resource formats (RSC7, ytd, ydr, ybn, ymap/ytyp), ported to Rust.
- **Ahmet Dedeler** — [bcpack-rs](https://github.com/Ahmet-Dedeler/bcpack-rs) (MIT/Apache-2.0): BCPack rANS lanes and BC1 decoder (git dependency).
- **ItsNotRexed** — [Leonida](https://github.com/ItsNotRexed/Leonida) (GPL-3.0): clean-room BCPack BC3/BC4/BC5/BC6H/BC7 rebuilders and tables, and the pool self-calibrator, ported in `vendor/sixng-core/src/bcpack*.rs`.
  Ships a fitted BCPack history (`vendor/sixng-core/data/bcpack-history-v1.bin`, embedded at build) so fresh machines skip calibration while `BCPACK_HISTORY_VERSION` matches.
- **R0Y00** — [Xbox2Windows](https://github.com/R0Y00/Xbox2Windows) (MIT): BCPack BC7 mode-6 golden test vector.
- **TrinitronClient** — [GTA6-Asset-Info](https://github.com/TrinitronClient/GTA6-Asset-Info) (GPL-3.0, docs): MLO portal, tint palette and format references.
- **Sollumz** — [Sollumz](https://github.com/Sollumz/Sollumz) (GPL-3.0): GTA V ped bone ids and yft/ytyp/meta format reference.
- **Sollumz_RDR** — [Sollumz_RDR](https://github.com/Sollumz/Sollumz_RDR) (GPL-3.0): RDR2 shader catalogue facts (per-shader texture/sampler/cbuffer lists, vertex semantics) for the `redm` writers. The RDR2 binary layouts were taken from vanilla RDR2 files and from CodeX (dexyfex) output used only as a parsing oracle; no CodeX code is used.
- **TomBox** (MIT/Apache-2.0): the vertex-normal stream layout (octahedral 10:10 / 8:8, from the game's mesh shader) in `tools/rage/meshlets.py`, ported in `src/model/decode.rs`.

If you are one of these authors and want your credit, link or code changed or removed, open an issue.

## License

GPL-3.0 (see `LICENSE`), as required by the vendored SixNG code. Third-party code keeps its own license as listed above.

Not affiliated with Rockstar Games or Take-Two Interactive. No game files are included; you need your own copy.
