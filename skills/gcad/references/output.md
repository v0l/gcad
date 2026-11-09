# Output and import

```sh
gcad export part.gcad part.step          # also .stl .obj .3mf
gcad export part.gcad part.svg --section y=0   # four-view drawing with a hatched section
gcad export part.gcad flat.dxf           # the sheet metal flat pattern
gcad export sheet.gdwg sheet.pdf         # the drawing sheets, also .svg
gcad export asm.gasm asm.step            # one product per part file, instances placed
gcad export hero.gcad hero.glb --height 1 --centre   # game mesh, also --scale, --tris N
gcad bom asm.gasm --csv
```

- STEP, STL, OBJ and 3MF carry the solids and mesh bodies; STEP writes colours and one product
  per part file. OBJ writes each part as a `g` group with an `.mtl`.
- GLB comes from `gcad export` or `gcad2glb in.gcad out.glb`; see [meshes](meshes.md) and
  [game-assets](game-assets.md). FBX is experimental and needs the game-assets build.
- A vendor STEP comes in with `part box vendor.step` in a `.gasm` (see [assemblies](assemblies.md)).
- Drawings write PDF and SVG, see [drawings](drawings.md).
