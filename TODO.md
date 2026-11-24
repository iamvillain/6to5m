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
- [x] First in-game check (2026-11-24, visual check in RedM): `prop_am_produce_box_m_orange_01a` and `prop_am_bench_int_01a_blue` (`--tint-row 5 --as prop_am_bench_int_01a_blue`), each its own resource with the ytyp as `DLC_ITYP_REQUEST`, spawn and render correctly: textures, the baked tint, the leather normal map and the `_ma` sheen. No collision (none is written). Two defects seen there, fixed since and not yet re-checked in game: glossy matte surfaces (the orange box cardboard, `default` shader) and LOD switches far too close (3.7 m on the box).
- [x] Loaded by RedM on those two props: deflate loose RSC8, 16-aligned (not page-aligned) physical data, a BlockMap claiming one virtual page on a 458 KB virtual segment, vanilla VFT values, the ytyp via `DLC_ITYP_REQUEST`.
- [ ] Not checked in RedM: no skeleton, no embedded bounds (no collision); other prop sizes and formats than the two above.
- [ ] Texture data is the linear DDS chain, unswizzled, zero-padded to BlockCount*BlockStride; BC chains trimmed to 4x4 like vanilla. Matches CodeX output; renders correctly on the two props checked in game (other formats unverified).
- [ ] `standard`: tangent w = sign(cross(N,T)·dP/dv), measured from vanilla p_canteen01x (all -1 there); TEXCOORD1 is (1,0) as in vanilla. Normal-map green convention DirectX (as GTA V / 6m); the leather normal map of prop_am_bench_int_01a_blue looks right in game.
- [x] Shaders by kind (`redm prop`; `ydr::shader_for`, one `ShaderDef` table entry per shader): opaque `standard` bucket 0, cutout `standard` bucket 3 (vanilla p_sign02x), decal `standard_decal` bucket 2 (32 + 192 B buffers, 1229 B block; vanilla p_helpwantedsign01x.yft), alpha `standard_alpha` bucket 1 (176 B, 972 B block; vanilla p_strmillwindow01ax layout, catalogue / rc_windows_01 defaults: glassiness 0, cavity 0, bumpiness 1), glass `standard_glass` bucket 1 (176 B, 972 B block, glassiness 1, bloommod 0.2/0.7; vanilla p_lamp01x: 12 of 15 vanilla glass rows use `standard_glass`). All take the material's normal map (flat stand-in) and real `_ma` (blank stand-in), PNXCTT layout; per-LOD bucket flags follow the buckets used. Parameter lists, sampler ids, buffer sizes and defaults equal the CodeX XML of those vanilla files (CodeX prints the names). `--default-only` and `redm convert ydr` keep the old mapping (`default` bucket 1 for alpha / decal / glass, bucket 3 for cutout). Cutout, decal and glass shaders: written from vanilla parameter lists, parse in CodeX, not yet seen in game.
- [ ] Open shaders: emissive (vanilla `emissive` 0x8F245469; stays `default` bucket 0, TODO in `shader_for`), water, multi-layer (`standard_2lyr`, terrain), vehicle paint, the decal variants (`standard_decal_blend`, `standard_dirt`, ...). No alpha reference parameter exists in these shaders (cutout is bucket 3 only); how the game thresholds cutout alpha is not known.
- [ ] Decals keep the 5 mm lift along the normal and the 6m vertex alpha (COLOR0.a). Vanilla p_helpwantedsign01x models its decal quads 1.5-2.9 mm in front of the board (no shader offset seen) with COLOR0.a 255; whether `standard_decal` reads COLOR0.a is unknown, so nothing was changed.
- [x] Tint rows: `redm prop <name> --tints` lists the embedded palette (size, rows of 256, colour of each LOD0-used column per row, filler/white = untinted); `--tint-row <n>` bakes row n (index n*256 + column; error listing the valid rows otherwise), `--as <model>` renames every output (files, drawable, archetype, txd) so variants sit side by side. Single props bake exact palette colours; `fivem build` keeps row 0 and 32-step rounding (`TintPick::BUILD`). Verified on prop_am_bench_int_01a (2051 entries = 8 rows + 3; row 0 columns 0/1 both white, row 5 column 1 = 70adde). Tinted copies are still capped at 512 px (`tint_dds`).
- [ ] The game picks the tint row per placed instance; which row a placement uses is not decoded for single props (`fivem build` reads tintValue but renders row 0, see buildings.rs).
- [x] Opaque geometry without a usable 6m normal map (e.g. the cardboard of prop_am_produce_box_m_orange_01a, normal key fc10991f64c38a2c absent from all 57 stores) was `default`, which has no roughness map: the engine's default gloss made it look like plastic in game. `redm prop` now writes every opaque geometry as `standard`, binding a flat normal map `r6m_flat_n` (one linear BC1 block of 128,128,255, as vanilla stores normal maps) and the blank (matte) `_ma` when there is no speculartex. `--default-only` unchanged; `redm convert ydr` unchanged (an OBJ material without `map_Bump` stays `default`). Not yet re-checked in game.
- [ ] LOD switch distances: 6m LodGroup distances (drawable +0x70) are about half of vanilla RDR2's for the same size. 6m: 188 of 495 levels.rpf props have LOD levels; High switch / bounding radius median 9.6 (p10 4.6, p90 27; 73 have a 0 High switch), each next switch x1.41 (median), absolute High switch median 6.3 m. Vanilla RDR2 props with LODs (8 benches and tables): only High + Med, High switch 22-37 m, switch / radius median 20.9 (13.6-27). `redm prop` multiplies by `--lod-scale` (default 2) with a floor `--lod-floor` x 1.41^k (default 20 m), archetype lodDist follows the scaled drawable; `--lod-scale 1 --lod-floor 0` writes the 6m values. A tunable assumption: what the 6m numbers mean exactly (a runtime LOD scale, screen-size metric) is not known, and the result is not checked in game.
- [ ] sRGB by slot in `redm prop`: every texture bound as diffuse is sRGB (44 6m diffuse maps in the 620-prop audit census have UNORM DX10 headers and were written linear before); `_n`/`_ma`/`_mb` linear (by name, or DX10 DDS code). `redm convert ytd` still decides by DDS header / name. Renders right in game on the two props checked (albedo sRGB, normal and `_ma` linear).
- [ ] Short mip chains (16 textures in the audit census, all map LOD / slod tiles or vehicle parts, e.g. 1024 px with 3 mips): the converter ships the 6m chain as stored (TTPF descriptor mip count, native blocks, no BCPack skip reported); not a converter fault, left as is. BC1 diffuse on cutout materials (13 census props): the 6m source is BC1 without punch-through alpha (255 of 259 are tinted terrain-multilayer layers) or an opaque BC7; no alpha is lost. Normal maps with blue 0 left as is (no vanilla evidence that blue is read).
- [ ] Skinned 6m drawables, fragments with physics, collision (`Bounds`), multi-archive (pack) props: not converted.
- [ ] `BoundingBox4.w` written 0 (vanilla non-zero, meaning unknown).
- [ ] Interiors (MLO archetypes, placement ymaps, collision, `_manifest.ymf`): not started here.

## Standard test region
Airport spawn, 1 km: `--bbox -2921,-1312,-1921,-312` (centre -2421,-812). Use it for all timings, file counts and in-game checks.
