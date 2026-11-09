# Architecture

Status: design only. The architecture ops (`level`, `grid`, `wall_type`, `wall`, openings, slabs,
roofs, stairs, rooms, schedules, IFC) are rows marked `missing` in `docs/ops.md`, and none of
them exist on master yet. The design is in `docs/architecture.md` and its tests are in
`tests/arch.rs`.

Until they land, model a building from what exists:

- walls, slabs and posts as extruded parts in a `.gcad`, with `material` set from the library
  (`brick`, `pir`, `concrete-c30-37`, `c24`) so `bom` and simulations know what they are made of;
- timber and steel frames with `member` (see [sheet-metal-weldments](sheet-metal-weldments.md));
- structural, thermal and acoustic checks with `sim frame`, `sim conduction` and `sim acoustics`
  (see [simulation](simulation.md)), with `examples/frame`, `examples/conduction` and
  `examples/acoustics` as models;
- drawings with `.gdwg` (see [drawings](drawings.md)).

Fill this file in when stage A lands: the header, levels, grids and layered walls come first.
