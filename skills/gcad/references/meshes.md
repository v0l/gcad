# Meshes

Characters, creatures and organic props are mesh bodies, not solids. Never write triangles.
Fenced `gcad` blocks are checked by the test suite; `text` blocks are fragments.

Contents: blockout, shells, sculpting, finish, rigging, looking, game runtime, examples.

```gcad
torso: blob 0,0,20 r=7,5,8
head: blob 0,-2,33 r=5 blend=2
arm_l: limb 7,0,26 12,-1,19 14,-2,12 r=2,1.6,1.3 blend=1.5
mirror x of=arm_l
leg_l: limb 3.5,0,13 3.5,-1,7 3.5,-3,1 r=2.6,2,1.8 blend=1.5
mirror x of=leg_l
remesh 0.5
color #c9a27e
```

## Blockout

Labelled `blob` and `limb` lines that `blend` into each other: torso, neck, head, muzzle, limbs
with a joint per bend, tail. Model the +x side as `_l` and `mirror x of=` it. Get the silhouette
right in `gcad render` first: proportions are most of the result and sculpting only nudges.
Legs are long and thin (a fox's leg is as long as its body is deep, a fifth as thick), bodies
slim, paws small, heads narrow wedges.

## Shells

Eyes, noses, inner ears, claws, teeth are their own `mode=shell` blobs: meshed finer than the
body, crisp at any size, with their own `color ... part=` and `gloss=`. An eye is three shells
(eyeball, dark pupil in front with `gloss=0.9`, eyelid crease). Never dent the body for eyes:
dents read as smudges. A shell must stand proud of the surface by about its thickness.

## Sculpting

Mesh fine, sculpt with brushes, decimate once at the end.

- `remesh 0.1` for a 30-unit head (edge about a third of the finest brush); `relax 6` right
  after so joints get even triangles. `remesh dynamesh 0.1` re-evens a stretched sculpt;
  `dyntopo on detail=0.03` refines under each brush.
- Define a brush once, then stroke:

```text
brush clay type=clay size=0.6 strength=0.08 falloff=smooth
brush dam type=dam_standard size=0.15 strength=0.06 lazy=0.5
brush pore type=stamp size=1.5 strength=0.02 alpha=pores(0.15,3)
stroke clay from=x,y,z to=x,y,z
stroke dam x,y,z x,y,z x,y,z pressure=0.3,1,0.3
dab pore at=x,y,z
```

- `size` is a radius in model units (`40px` is pixels of the last `gcad inspect` view), `strength`
  the height. Also `falloff=`, `edge=`, `spacing=`, `lazy=`, `mirror=x`, `geodesic=on` (thin parts
  like ear rims). Change one later with `brush clay strength=0.04`. `gcad docs` lists the types;
  `dam_standard` cuts creases and lids, `snake_hook` pulls horns and ears (add `detail=`),
  `stamp` takes `alpha=` pores, scales, wrinkles, hair or an image.
- A stroke starts at the surface point nearest each path point; the reply says how far off that
  was, and a large number means a wrong coordinate. Put a point at least every brush size along
  a curve. A brush needs at least three edges across its radius (`inspect --wire`).
- Protect finished areas: `mask sphere=x,y,z r=`, `mask by=cavity r=1`, `mask invert|blur 2|grow
  0.3|clear`; `isolate head`, `hide ear_l`, `show`. Brushes and `select` ops skip masked vertices.
- `group lip sphere=x,y,z r=1` makes a polygroup. `join arm_l body blend=1.5` softens a seam.
  `layer wrinkles` records the strokes after it (`amount=0.5` dials it). `level base`, then
  `subdivide 1` and `project base` restores a region.
- Colour parts that should differ. Markings are their own blobs blended into the body, and
  get `soft=` so they fade (`color #f4efe6 part=chest soft=1.5`); hard borders read as patches.

## Finish

Last line of each body: `finish decimate=20000` (count or `10%`) or `finish max_error=0.02`. It
keeps part borders and reports the triangles and error. Put it behind a variable (`if budget>0
finish decimate=budget`) so renders keep full detail, or export with `--tris 20000`. `finish`
goes before `skin`. `focus=head,jaw weight=30` keeps the face finer than the body.

## Rigging

One line each, after `remesh` and all sculpting. Order: `bone` lines (`parent=`, `head=`,
`tail=`; `mirror x of=arm_l`), `skin auto`, `skin part=head bone=head_b` for rigid parts,
`clip name fps=` with `key t= bone= rotate= axis=` lines, `pose clip=name t=` then `gcad render`.

- `gcad skincheck f.gcad` after binding: no edge over 0.35, 0 unrelated, rigid core near 1, no
  flipped triangles in any clip key.
- Fix a bad joint with `skin smooth sphere=x,y,z r= passes=20`, or `skin paint` (several points,
  `strength=-0.5` removes weight, `mirror=x`) then `skin smooth`; `skin mirror` copies a side.
- A turning forearm needs `bone lower_l ... twist=3`. `skin auto joint=` widens the blend, `heat=`
  above 1 keeps weights near the closest bone. GLB players skin linearly, so judge weights
  without `skin blend dq=1`.
- The viewer (`gcad f.gcad`) has an animation card to play and scrub clips and pose bones.

## Looking

Look between every step. A model seen three times is a blockout; fifteen looks, fixing what each
showed, is an asset.

- `gcad render f.gcad out.png` for the whole model.
- `gcad inspect f.gcad out.png --part head --view front|right|top` for a detail at its own size;
  `--zoom x,y,z,r` close-up, `--wire` triangles, `--path x,y,z --path x,y,z` previews a stroke.
- `--pick x,y` prints the surface point, normal, part and curvature under a pixel. Use it for
  `dab at=`, stroke points, shell `at=` and `mask sphere=`; never guess small coordinates.
- `--line N` shows what line N selected; `moved 0 vertices` in `gcad check` means a miss.

Stop when a close-up of the face, a side view and a three-quarter view all look right.

## Game runtime

Ship the text and build at load: `gcad_mesh::build(include_str!("hero.gcad"))` (crate
`crates/gcad-mesh`, no CAD kernel) gives vertex buffers, joints, weights, clips and
`bone_matrices(clip, t)`; `to_glb()` or `gcad2glb in.gcad out.glb` writes GLB; `include/gcad_mesh.h`
is the C ABI. Only mesh and rig lines run there. `gcad check-mesh part.gcad` builds a flat-solid
prop both ways and fails if a body's volume or size differs by more than 3%. See `docs/gcad-mesh.md`; `gcad2glb` also writes
files mixing solid and mesh bodies. Crate layout is in `docs/gcad-crates.md`.

## Worked example: a sitting fox

What separated this from the agents' foxes (`bench/minds-eye/ceiling/fox.gcad`, full file with sculpt and colours): a `limb` spine and a `limb` tail that curls round the feet, thin straight forelegs, haunches as flat wide blobs on the floor, a wedge head (muzzle and jaw as tapering `limb` lines), ears flattened and cupped after `remesh`, eyes and nose as `mode=shell`, markings as parts with `soft=`, and about fifteen renders. Build the whole animal at once, render, then correct the proportion that is off.

```gcad
let hz=23 hy=-3.6
hips: blob 0,3.2,5.6 r=4.6,5,5 blend=2
torso: limb 0,2.4,7 0,-0.6,12.5 0,-1.8,16.5 r=4.6,3.8,3.1 blend=2.5
chest: blob 0,-3,13.2 r=3.3,2.6,4.6 blend=2
haunch_l: blob 3.6,3.6,4.6 r=2.6,4.6,4.2 rx=-20 blend=1.6
mirror x of=haunch_l
neck: limb 0,-1.8,16.5 0,hy+0.4,hz-2.6 r=2.8,2.4 blend=1.6
head: blob 0,hy,hz r=3.3,3,2.8 blend=1.4
muzzle: limb 0,hy-2,hz-0.6 0,hy-6,hz-1.2 r=1.5,0.55 blend=1.2
ear_l: limb 1.9,hy+0.4,hz+1.9 2.9,hy+0.7,hz+4.6 3.5,hy+0.8,hz+6.4 r=1.6,0.95,0.12 blend=0.9
mirror x of=ear_l
leg_l: limb 1.7,-2.6,11 1.7,-3.4,5.5 1.7,-3.7,1.2 r=1.25,0.85,0.75 blend=1.2
mirror x of=leg_l
tail: limb 0,7.6,3 3.6,9.2,2.4 7.2,6.6,2.2 7.8,1.4,2.1 6,-3.2,1.9 r=1.2,2.4,2.9,2.6,1.6 blend=1.4
tip: limb 6,-3.2,1.9 4.6,-5.4,1.8 r=1.6,0.5 blend=1
nose: blob 0,hy-6.25,hz-0.95 r=0.62,0.5,0.45 mode=shell
eye_l: blob 1.6,hy-2.05,hz+0.55 r=0.95,0.42,0.6 rz=-30 ry=-14 mode=shell
mirror x of=eye_l
remesh 0.12
relax 4
ear_cup: select ear_l mirror=x falloff=0.6
scale 1,0.42,1
select all
color #c9581c
color #f3ede3 part=chest soft=1.8
color #f3ede3 part=tip soft=0.8
color #2a1a12 part=leg_l soft=1.5
color #2a1a12 part=leg_r soft=1.5
color #3a2216 part=ear_l soft=1
color #3a2216 part=ear_r soft=1
color #15100c part=nose gloss=0.9
color #d89a2a part=eye_l gloss=0.6
color #d89a2a part=eye_r gloss=0.6
```

Flat-sided parts (armour plates, crates, signs) are `block` lines, not blobs: `cuirass: block 0,0,53 size=15,9.6,10.4 round=2.6 blend=1`. A sphere used as a plate reads as a lump.

Known limits (open issues): mesh parts have no metal material, so armour reads as grey plastic; there is no plate op that thickens a surface patch; no fur; `pinch` or `scale` on a selection that borders a blended part tears the seam, so select the part and its neighbour together.

## Examples

`examples/mesh/creature.gcad` (blockout and sculpt), `fox.gcad` (anatomy, colours, `--set
budget=8000`), `rigged_creature.gcad` (bones, skin, clips), `hero.gcad` (focus finish).
