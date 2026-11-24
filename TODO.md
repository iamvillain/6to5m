# TODO: GTA V / FiveM conversion

Done: Rust writers (`6ixpack convert`) and `6ixpack fivem build` (terrain, buildings incl. fragments, interiors and LOD chains, water, shared txds, incremental, self-check, --report); items 1-3, 7, 8 and most of 5 below.

Open gaps found:
- Fragment props convert as dynamic yft (fivem/frags.rs: one piece, a box per 6m collider with its mass; capsule/sphere/hull types 1, 2, 9, 14 as their bound box until their layouts are decoded; no breakage into parts yet). Textures missing from the ttdb pools come from the TTPF stores (re-encoded DXT1/DXT5); 3 in the test district are in neither.
- Interiors convert to MLOs by default (`src/gta5/fivem/interiors.rs`; checked with CodeWalker, not yet in game). Portals are decoded from the 6m archetype and entities sit in their rooms; entity sets are written off by default. Interior collision linked by sixng (exact, high or medium) is written per instance as `t6m_intcol_<hash>_<n>.ybn` with each polygon's room id picked by room bbox (6m bounds carry none; checked in CodeWalker, not in game). Interiors without linked collision keep only the world collision cells, so the player may count as outside there (`T6M_MLO_LIMBO=1` is the fallback). Interior lights are decoded from the interiors' own meta files during the build (archetypes defined in other meta files get no lights; `T6M_NO_LIGHTS=1` skips); intensity (1:1, `T6M_LIGHT_SCALE`) needs an in-game check. `--interiors` keeps the old world-space flattening. LOD parents that are prop batches (`*_batchlod`) are ignored.
- Terrain tile numbering differs from the Python run (153 vs 158 tiles).
- Nothing is checked in game yet; quaternion convention unverified (`--conjugate-rot`).

Next, from the FiveM port's requests:

1. `6ixpack fivem build --out <res> [--bbox ..] [--name ..]`: writes stream/ (ydr/ybn/ytd/ytyp/ymap), fxmanifest.lua (this_is_a_map, DLC_ITYP_REQUEST, WATER_FILE) and water.xml. Incremental: skip outputs that are up to date.
2. Reload every written file in Rust and report counts (verts, polys, textures); fail loudly on a bad file.
3. Shared txds: dedupe textures by key, group them per area/name family, cap each ytd at ~48 MB, set the ytyp textureDictionary, use a placeholder for keys that don't resolve, keep full mip chains.
4. Materials: map GTA VI shader params to GTA V shaders (normal_spec, cutout/decal, terrain_cb_w_4lyr with vertex-colour weights), driven by the `texture-gaps` and `shaders` census.
5. Placements: put interior- and group-local entities in world space (or emit MLO ytyp), keep the LOD chain from lod-links (parentIndex/numChildren), split ymaps spatially with correct extents, verify the quaternion convention (GTA V may store the inverse) against a known placement.
6. Collision: sixng collision → ybn with GTA V material ids from surface categories, one ybn per map cell.
7. Limits: names < 64 chars (hash fallback), ≤ 65k verts per geometry, drawable size caps, archetype bounds from the mesh.
8. `--report` JSON: converted, skipped and why (unnamed hashes, missing textures, fragments/skinned meshes).
9. Later: roads/paths → ynd, navmesh → ynv, lights → LODLights.

## fivem (open)
- BCPack textures: flat decodes rejected (placeholder) until the pure-Rust BCPack decoder lands in sixng-core ttpf.
- Collision materials come from inferred `sixng surfaces` categories (geometry-only unless paths/batches/navmeshes are exported first); verify in game, refine with road and plant cues.
- gtxd.meta shared txd chain unverified in game.
- Multi-archive models (pack/core_restofworld) not converted.
- LOD/SLOD drawables without a model link (unnamed hash) not converted.
- Airport: roads missing, far-apron floating poles, canopy cutout look — uninvestigated.

## fivem weapon (`src/gta5/fivem/weapons.rs`, `yft::build_ydr_skel`)

- [ ] Test in game (give WEAPON_W_PI_SCORPIO_01): grip/muzzle sockets, hand IK, animations.
- [x] Weapon textures: 6m weapon shaders (e68a4a98) carry only normal + packed material (739e3d0d, R = material id); the colour map is synthesized per id. Traced: every one of the 2,733 `weapon` (e68a4a98) shaders in data.rpf has params speculartex, 20245319, bumptex, speculartex2, materialid; 20245319 is never bound in any asset (null texture pointer), so the per-id finish palette is a runtime global the engine binds, not asset data. `materialid` (8adb8480, BC4, ids at R = 2 + 4n) is present on only 54 shaders; it sits in a type-0 texture record that sixng drawable::find_shaders skips. Not in data.rpf PSOs, 0x7a meta (find-u32 on weapon name hashes: 0 hits in data/common/packs/script/story) or named txds. Next: find the global in the executable or shaders.rpf.
- [ ] Real stats from 6m weapon data instead of GTA V class defaults; component variants (mags, sights) as GTA V components.

## fivem vehicle (`src/gta5/fivem/vehicles.rs`, `src/gta5/yft.rs`)
Loads in CodeWalker; not yet tested in game.
- Physics: one box child per exterior 6m physics group (doors, bonnet, boot, bumpers, windows, wings, mirrors, plates) with 6m collider masses, composite BVH past 5 children; joint limits are left to GTA V (6m vehicle fragments have no limits block). Unverified in game whether doors open; boxes are AABBs of each part, not the 6m hull colliders.
- Wheels: rim + tyre drawables from `sixng vehicles` (scale 1.625 x wheelScale is fitted, not decoded); right-side rim orientation unverified.
- Paint is a solid texture of the configuration's body colour (stored ARGB read as linear); add vehicle_paint shaders so carcols colours apply, plus glass, lights, dirt and damage.
- Modular parts (bumpers, intakes, roof attachments in part_groups) are not added; handling PSO fields are unnamed, so handling stays default.
- Handling, seats, layout and meta are sedan defaults; map 6m vehicle data when it is decoded.

## fivem ped (`src/gta5/fivem/ped.rs`, `src/gta5/ped.rs`, `src/gta5/ped_bones.rs`)
Loads in CodeWalker; not yet tested in game.
- Textures: diffuse only, the one on the largest geometry; normal and spec maps are flat. Some clothing textures are not in TTPF ("not found"), so those drawables get grey.
- Some clothing vertices name bones in neither the 6m ped skeleton nor the drawable (e.g. cs_lucia tshirtcrop: 6681 influences dropped). Find those palette bones.
- Part selection is SixNG's default look (one part per outfit slot; `--all` for every part). Characters without their own head (cs_jason) use the legacy head and whole-body copies. No props, no texture variations, no base body under clothing.
- Facial bones keep GTA V reference positions (only 78 of 101 bones are placed from 6m).
- Hair and beards: `--hair` / `--beard <groom id>` draw a strand groom as crossed cards (flat `--hair-color`, rigid on SKEL_Head), and `--grooms` lists the grooms that fit the head. Nothing in the game files links a ped to its grooms, so you pick one by eye. Still to do: brows and lashes, an alpha strand texture, card UVs from the root UVs (A2), and skinning beards to the jaw.
- Head faces: heads bind no textures; albedo/normal come from a "heads" material preset (formats/07 §7.5). cs_lucia's head is in no preset, so it stays grey. Wire presets for tagged heads.
- Head skin variant 0x8c (slot 9 as 4 x u32 joints) is not decoded by sixng-core; cs_lucia's head uses the ordinary 4+4 x u8 layout.

## fivem features (from the FiveM migration)
- Road paths to .ynd (AI traffic), from `sixng paths`
- Navmesh to .ynv (ped/AI pathing)
- LODLights / DistantLODLights (`src/gta5/fivem/lodlights.rs`) are written but unverified: check in CodeWalker and at night in game (intensity byte = intensity x5.3125, clamped 255)
- Minimap texture and zone names
- Region split (`--split`) done; verify in game, and split the LOD/SLOD ymaps and terrain per region if common grows too big
- Coordinate offset option: GTA VI's map reaches y 9474, past GTA V's ~±8000 limit
- Los Santos removal in Rust: stub ymaps/ybns for vanilla _citye/_cityw/_hills, packing the ~11k ybn stubs into fewer files

## redm (`src/rdr2/`: RSC8 ytd v2 / ydr v162, `6ixpack redm`)
Done: RSC8 container (root first, blocks by size, 16-aligned, deflate), ytd writer, static ydr writer (`default`, `standard` with real 6m normal maps, all 6m LOD levels), `redm convert ytd|ydr`, `redm prop <name>` / `--list`, `redm check`, in-crate reader + tests. Every output parses in CodeX (`cx.xmlfile` exit 0, vertex counts and texture names as written); golden cubes/ytds match CodeX-written files structurally (only VFT values, Unknowns ptr 0, stub MipLevels 1 and sRGB choice differ, all on purpose).
- [x] `.ytyp` archetype writer (`src/rdr2/ytyp.rs`, RSC8 v3 meta: CMapTypes + CBaseArchetypeDef, constant class/enum tables, meta pointers), `redm convert ytyp <json>`, `redm check` walk; `redm prop` writes `stream/<name>.ytyp` (name = assetName = txd = ydr stem, bounds from the written drawable) and lists it in fxmanifest (`files` + `DLC_ITYP_REQUEST`). Golden r6m_one/r6m_two: virtual segment byte-identical to CodeX output except bbMin.w/bbMax.w (copy of x, as vanilla); CodeX readback XML equal. Static-prop values copy vanilla p_canteen02x (flags 0x30C20000, DUMMY_OBJECT, no collision).
- [ ] ytyp lodDist for props is a heuristic (last finite drawable LOD switch, else 2x farthest switch, min 30x radius clamped 40..100 m); 6m's own archetype lodDist (archetype_lod.jsonl) not used yet.
- [ ] **Next slice: collision**: embedded `Bounds` in the ydr (Composite v2 + GeometryBVH v2) with archetype `physicsDictionary` = model name (field already supported).
- [x] Material maps: `redm prop` writes a real `_ma` per `standard` material from the 6m speculartex (c6f46e79, same layout: R metal mask and G roughness copied, B AO copied unless flat 128 = "no AO authored" -> 255, A 255), linear BC1, full mips, size clamped by `--max-tex` and the material's diffuse; tinted copies share their material's `_ma` (`Geom::spec`, `r6m_<cache id>_ma`). Missing / undecodable / <= 8 px speculartex -> vanilla `blank_ma` (0,243,255,255), also on the OBJ path (`redm convert ydr --ytd`). `_mb` is deliberately always vanilla `blank_mb` (0,0,127,255): 6m speculartex2 (739e3d0d) G indexes 6m's own detail array. `fivem build` untouched (opt-in flag on rpf9_mesh). Checked on prop_am_bench_int_01a (37.1,112.9,213.8), prop_am_bench_26a (0,47.5,254.4), orange box (blank_ma; both speculartex keys in no store).
- [ ] `_ma` A channel (RDR2 porosity / wetness?) written 255; meaning unverified. 6m speculartex2 detail (`_mb` R wear/scratch, G detail index) not converted. In-game look of the converted maps unverified; the flat-128 -> 255 AO rule is an inference.
- [ ] In-game verification in RedM: resource loads, `CreateObject(joaat(name))` spawns, textures/LODs right, DLC_ITYP_REQUEST caveats (Rockstar Editor crash reports).
- [ ] Not tested in RedM at all: deflate loose RSC8 accepted? 16-aligned (not page-aligned) physical data, block map claiming 1 virtual page for >64 KB segments (every real prop so far is 64-128 KB virtual), no skeleton, no embedded bounds (no collision), vanilla VFT values; ytyp never loaded in game.
- [ ] Texture data is the linear DDS chain, unswizzled, zero-padded to BlockCount*BlockStride; BC chains trimmed to 4x4 like vanilla. Matches CodeX output; GPU-side correctness in game unverified.
- [ ] `standard`: tangent w = sign(cross(N,T)·dP/dv), measured from vanilla p_canteen01x (all -1 there); TEXCOORD1 is (1,0) as in vanilla. Normal-map green convention assumed DirectX (as GTA V / 6m), unverified.
- [ ] Draw buckets: alpha/decal/glass go to `default` bucket 1, cutout bucket 3; no dedicated RDR2 alpha/decal/glass/emissive shaders, no tint palettes beyond baked copies.
- [x] Tint rows: `redm prop <name> --tints` lists the embedded palette (size, rows of 256, colour of each LOD0-used column per row, filler/white = untinted); `--tint-row <n>` bakes row n (index n*256 + column; error listing the valid rows otherwise), `--as <model>` renames every output (files, drawable, archetype, txd) so variants sit side by side. Single props bake exact palette colours; `fivem build` keeps row 0 and 32-step rounding (`TintPick::BUILD`). Verified on prop_am_bench_int_01a (2051 entries = 8 rows + 3; row 0 columns 0/1 both white, row 5 column 1 = 70adde). Tinted copies are still capped at 512 px (`tint_dds`).
- [ ] The game picks the tint row per placed instance; which row a placement uses is not decoded for single props (`fivem build` reads tintValue but renders row 0, see buildings.rs).
- [ ] A part whose 6m normal map is missing from every texture store falls back to `default` (no normal map): e.g. the cardboard of prop_am_produce_box_m_orange_01a, normal key fc10991f64c38a2c absent from all 57 stores.
- [ ] sRGB: albedo sRGB, `_n`/`_ma`/`_mb` linear (by name, or DX10 DDS code). RedM rendering of either unverified.
- [ ] Skinned 6m drawables, fragments with physics, collision (`Bounds`), multi-archive (pack) props: not converted.
- [ ] `BoundingBox4.w` written 0 (vanilla non-zero, meaning unknown).
- [ ] **Interior conversion (`redm interior`, `src/rdr2/interior.rs`, `mlo.rs`, `meta.rs`, `meta_defs.rs`, `ymap.rs`, `ybn.rs`).** Done: interior data (`fivem::interior_source`), props + shared ytds, `<int>.ytyp` with the `CMloArchetypeDef` (entities of unconverted archetypes dropped, room / portal / set indices remapped, portal corners reordered so the normal points away from roomFrom, undecoded portals -> all in limbo), `<int>_milo_.ymap` (contentFlags 73, ORPHANHD, flags 1572872, numExitPortals = portals to limbo, `--at`), `<int>.ybn` (0x3E/0x7F3BEC0 child from the movement shapes) + `hi@<int>.ybn` (0x2/0x610000 child from all shapes), fxmanifest in the working-interior layout. Generic meta writer reproduces the old prop ytyp byte for byte and the spec goldens' data blocks (MLO ytyp except the prop archetype values, props ymap, milo ymap); ybn geometry arrays byte-identical to both CodeX goldens (`R6M_MLO_SPEC=<spec dir> cargo test` compares). CodeX reads back every file of int_d4c876ae / int_24688994 (exit 0, counts / placement equal).
  - [x] `_manifest.ymf` (`src/rdr2/manifest.rs`): PSIN / PMAP / PSCH writer and reader; PSCH (620 B) embedded from the omm_lafitte working manifest (same five block types); both working samples rebuild byte for byte from their own values (`R6M_MANIFESTS=<dir>` test; sdcourt's opaque MapDataGroups block kept as is). Written by `redm interior`: milo ymap -> ytyp (INTERIOR_DATA), ytyp (INTERIOR_DATA, no prop-pack dependencies: every entity archetype is in the interior's own ytyp), interior -> [hi@<int>, <int>] (lafitte order; sdcourt lists [<int>, hi@<int>]). CodeX reads it back. In-game: unverified; whether an empty itypDepArray is fine is unknown (the samples list game prop packs).
  - [ ] Primitives (boxes, capsules) are tessellated into triangles (sixng-core bounds mesher); Box / Capsule polygon or child layouts are only partly verified.
  - [ ] Entity rotation: copied as stored (6m CEntityDef = inverse orientation = RDR2 interior entity convention). Checked on int_e8f8fe99 (5 rotated converted entities, vertex-to-collision distance mean 0.023 m straight copy vs 0.054 m conjugate) and int_ea19142a (0.041 vs 0.043); weak evidence, not in game.
  - [ ] Room names are `room_<6m hash>` (6m names not recovered), timecycle 0, room flags 32, portal flags 0 (exit) / 64 (inner), MLOInstflags 0, lodDist 150: values from the spec's samples, semantics unknown. No `_strm` ymap (6m interiors carry no exterior entities here). Placement at 6m coordinates lands at an arbitrary RDR2 spot: use `--at`.
  - Work data: `sixng collision`, `sixng interiors`, `sixng placements --id-file` (interior files only); whole-archive collision is needed for sixng's collision link (bounds ordering), placements can be limited to the interior files.
  - Resolution: entities load by sixng model id (`PropBatch::source_model`, `fivem build`'s route: `voff_of` + index entry at that offset, members and indexed PACK_ARCHIVES included; outputs without a usable name are `r6m_<hash>`). Over the 98 interiors only 3058 of 49416 entity placements (1246 of 6674 archetypes) have a model id: 2875 by levels.rpf name, 5 by archetype link, 178 in pack/core_restofworld (not a PACK_ARCHIVE, not loaded, as in `fivem build`); story000, shared_minigame000, story000_restofworld and packs.rpf add none. The other 46358 have an archetype def in levels.rpf but no drawable in any index: the levels.rpf carve covers 5.8% of the archive (zlib streams found without the encrypted TOC; packs.rpf: 95.5%), so the models are most likely in levels.rpf data the carver does not find. Fix belongs in sixng-core's index. Best test MLO today: int_d4c876ae (5 rooms, 6 portals, 80 entities, 12 resolvable).
  - Dictionaries: a model whose own textures exceed the 14 MiB cap has its largest textures' top mips dropped until it fits (`fit_model`).
  - `fivem build` reads levels.rpf interiors.jsonl through the in-memory vfs, but nothing in the build writes it for levels.rpf (only for packs): MLOs from levels.rpf may be missing from `fivem build` unless `--placements` points elsewhere. Not checked further.

## Standard test region
Airport spawn, 1 km: `--bbox -2921,-1312,-1921,-312` (centre -2421,-812). Use it for all timings, file counts and in-game checks.
