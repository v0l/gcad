# Changelog

All notable changes to gcad are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Viewer simulation card with readings, `expect` checks, notes, sweep traces and a run button.
- Viewer draws a simulation's setup before it has a result: the mesh with loads and supports,
  or for flow the parts in their duct or air box, the wind and the heated faces.
- `bearing faces axial`: a round band held sideways at one ring, free to turn and tilt, so it carries
  force and no moment. `pin` on a short band suggests it.
- `support_stress` reading: the peak von Mises stress within one element size of supports and bonds.
- `torque axis=` and `wind toward=` take `x`, `y`, `z` and `-z`.
- Gear and worm losses on couples: `efficiency=` and `back=`, or a worm's `lead=` and `friction=`,
  with `joint.back` readings and worms that lock.
- `torque joint value` in `sim torque`: a steady torque on a joint, like wind.
- `joint ... drag=`: bearing and seal friction on a joint, added to the torque to drive it.
- A boss glued onto a face, including twisted faces and over an edge; twisted extrudes with holes.

### Changed
- `stress` and `safety` leave out one element size of each part around its supports and bonds,
  where a rigid hold is singular. A part smaller than the `mesh` size uses a sixth of its own
  diagonal, so a key or pin in a large assembly still counts towards `safety`.
- A probe just outside a coarse curved face reads on the mesh surface with a note, instead of failing.
- A `let` in an included file carries over to the file that includes it.
- Viewer shows a stale simulation result with a warning instead of hiding it.
- Viewer reloads when a result is written.

### Fixed
- STEP imports read each solid in its own length unit; inch vendor models came in 25.4 times too
  small.
- STEP surfaces of revolution with a circle profile, like the groove on KiCad electrolytic
  capacitors, import as tori, and curves on surfaces the kernel cannot convert import as cubic
  fits, instead of the solid becoming a box.
- STEP assemblies that monstertruck cannot walk, like KiCad and agentee board exports, come in with
  every part in place instead of all at the origin. Faceted breps and surface models are built
  from their polygons, and a solid that still cannot be read becomes a box and is named in the
  line's summary, instead of failing the import.
- Parts with a face narrower than the kernel's chord tolerance, like a 0.01 mm step where two
  bores meet, mesh for simulation. The surface came out open at most sizes; it is now rebuilt
  with finer boundary polylines until it closes.
- A shaft extruded in several pieces of one radius meshes at the size asked. Its seams left
  flat elements, so the mesher retried down to a fraction of a millimetre (1.2 million
  elements and two minutes for a 190 mm shaft).
- Revolved parts whose profile runs along the axis mesh for simulation; the zero-area faces
  the axis edge leaves are skipped.
- Smooth lofts between sections with more than 240 matching edges.

## [0.3.1] - 2026-10-12

### Added
- Heat transfer in flow simulations, and ducts.
- Early stop for flow runs that have settled.
- OpenFOAM heatsink comparison.
- Wind example, with CFD pictures in the README.

### Changed
- Flow results are read asynchronously and stored as f16.
- Finer meshes for clash checks.

### Fixed
- Release artifact path.

## [0.3.0] - 2026-10-12

### Added
- Simulations: static stress, steady heat, natural frequencies, linear buckling,
  thermal expansion, torque, frictionless and Coulomb friction contact.
- Tetrahedral mesher with a mesh convergence check and per-part mesh sizes.
- Bonded assemblies in simulations, with ties that carry moment, pin supports
  and torque loads. Parts can be left out of a simulation.
- GPU lattice Boltzmann flow solver (`sim flow`) with inlets, outlets, forces,
  curved walls, sponge layers and Smagorinsky turbulence.
- Wind loads on parts and assemblies, static loads from a flow result, and an
  OpenFOAM runner that exports a simulation's parts.
- Viewer: results coloured by field, value under the cursor, loads and supports,
  3D streamlines, flow slices with playback of unsteady results, animated
  particles, part opacity.
- Browser build of the viewer, deployed to Cloudflare Pages.
- Material libraries, with MatML and FreeCAD imports.
- Sheet metal tabs, flanges, hems and flat patterns.
- Structural members and cut lists.
- Bosses, lips, grooves, vents and snap hooks.
- Bearings, dowels, standoffs, screw holes, standard parts and gear extras.
- Angle measurement and two-distance chamfers.
- `solid=` to pick a solid from a STEP import.
- Parts with shared sizes may share a part file.

### Changed
- Viewer is built on the egui_bench 3D viewer, with part selection, ghosting,
  zoom and orbit about the cursor and screen-width edges.
- Coplanar faces are fused after booleans.
- Faster interference sweeps and part prefetch.
- Models build on web worker threads in the browser.

### Fixed
- Cuts only remove material below the sketch plane (#1).
- Overlap is estimated when a boolean fails (#2).
- Concentric circles are fitted about their mean (#3).
- Cuts grow off round walls (#5).
- STEP imports are healed (#6).

## [0.2.0] - 2026-10-08

### Added
- Modelled ISO metric threads that run out into chamfers and countersinks.

### Changed
- Fillets at corners and hole rims are built together, with exact end arcs and
  finer round fillets.
- Shells follow fillets.
- Exact lines and circles on boolean edges, and smooth STEP intersection curves.

### Fixed
- Inside corners where three edges meet are rounded.
- Fillets that leave the solid unchanged fail instead of passing silently.
- Edge gaps are checked in STEP output.

## [0.1.0] - 2026-10-08

First release.

### Added
- Line-per-operation part files (`.gcad`) with sketches, constraints, extrudes,
  revolves, sweeps, lofts, shells, drafts, ribs, holes, fillets, chamfers,
  patterns, mirrors, booleans, include, `if` and `--set`.
- Involute spur and internal gears.
- Assemblies (`.gasm`) with mates, joints, closed joint loops, couples,
  patterns, poses, exploded views and interference checks.
- Materials, mass and bills of materials.
- Export to STEP (with colours and assembly structure), STL, 3MF, OBJ and SVG
  drawings with section views, dimensions, parts lists and balloons.
- DXF and SVG import.
- egui viewer with a view cube, measuring, section plane, orthographic camera
  and joint sliders.
- Release builds, install script and agent skill.

[Unreleased]: https://github.com/v0l/gcad/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/v0l/gcad/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/v0l/gcad/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/v0l/gcad/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/v0l/gcad/releases/tag/v0.1.0
