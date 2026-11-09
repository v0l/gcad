# Simulations

A `.gsim` file simulates a part or assembly. `gcad sim x.gsim` prints readings, writes
`x.result.bin`, and exits 1 when an `expect` line fails. `gcad check x.gsim` reprints readings and
says when the result is stale; `gcad render x.gsim x.png` plots them. `-q` silences progress.
`gcad sim x.gsim --dry-run` sizes a run first. Kinds are listed by `gcad docs`; limits are in
`docs/simulation.md`.

Contents: torque, static, thermal, assemblies, modal and buckling, flow, frame, conduction,
acoustics, sims server.

## Torque

Sizes joint motors. Readings are the worst holding torque (N·m) or force (N) per joint.

```text
sim torque
part arm.gasm
payload hand 300
sweep shoulder elbow steps=12
expect shoulder<15 sun_s_turn<4
```

- `payload name grams` hangs at the centre of mass, or `at=x,y,z`. Parts need a `material` to weigh.
- Couples pass load on: a sun at `ratio=4` reads a quarter, which is the motor's torque. With
  lossy couples `joint.back` is the torque the load pushes back with (0 when a worm locks).
- `torque az 6.3` adds a steady torque, `joint ... drag=0.05` bearing friction (N·m).
- `couple ... module=1 face=10 gear=wheel pinion=worm` adds mesh forces, Lewis and Hertz safety;
  `speed az 1` (rpm) adds power and mesh heat.

## Static

Linear stress on a part (mm, N, MPa).

```text
sim static
part bracket.gcad
material aluminium
mesh 2
base: fix <Z
load boss.end force=0,-200,0
top: probe 18,12,12
expect stress<80 safety>2
```

- Supports: `fix`, `slide faces` (normal only), `pin h.side`, `bearing seat.side` (add `axial` on
  one), `bearing kfl:bore.side spindle axial C0=2900` (a part turning in another's bore).
- Loads: `load faces force=x,y,z`, `pressure faces 10`, `torque shaft.end 5`, `warm 40` (thermal
  expansion), `warm -100 part=bolt` (preloads a bolted joint).
- Readings: `stress`, `displacement`, `safety`, reactions, `probe.stress`. Peaks at supports and
  sharp inside corners are singularities: fillet the corner or probe away from it.
- `converge` re-solves on a coarser mesh and names readings that moved over 5%; trust the others.

## Thermal

```text
sim thermal
part heatsink.gcad
cold: temperature base.start 20
convect fins.side h=10 ambient=25
heat base.end 3
```

`source 3` heats the volume (W). Readings: `temperature` (hottest), `coolest`, heat through each
labelled boundary, `probe.temperature`. Still air `h` is 5-10 W/m²·K.

## Assemblies

Static, thermal and modal take a `.gasm`: touching parts are bonded, each in its own material;
pick faces as `part:faces` (`fix base:<Z`). `contact a b` lets a pair press and part instead of
bonding (`friction=0.3` holds until it slips). `leave ctl* *screw*` drops parts that carry no
load (`*` matches any run of letters).

## Modal and buckling

`sim modal` with `fix` lines and `modes 6` reads `mode1`... in Hz with effective mass shares. No
supports means free-free. `sim buckling` reads a load factor under the `static` loads; keep it
above about 3.

## Flow

Wind on a part or assembly on the GPU (lattice Boltzmann).

```text
sim flow
part head.gasm
leave *screw* *nut*
wind 33.3 toward=0,1,0 about=0,0,0
mesh 5
```

Speed in m/s, `toward` is where the air goes, moments are about `about`, `mesh` is the cell in
mm. Run `--dry-run` first for cells and GPU memory. Readings: `drag`, `side`, `lift` (N), `cda`,
`moment.x/y/z`, `part.drag`, `pressure`, `suction`. Round members under about 10 cells across
read up to 1.8x too draggy. Leave parts out only inside a closed box. `wind flow.gsim` in a
`sim static` loads parts with that pressure and needs a fresh flow result. `gcad render w.gsim
f.png --flow` (or `--lines`) draws the air.

## Frame

Buildings: each body is a straight prismatic member joined where ends meet (column floor to
girder centreline, girder between column faces). `fix`, `pin`, `slide` take end faces
(`left:<Z`); `load left:>Z force=...` on an end is a joint force, on a side face (`girder:+Z`) a
line load; `gravity 0,0,-9.81`; `combine uls weight:1.35 live:1.5` reads `uls.stress`;
`release beam:<X` pins an end; `modes 3`, `buckling 2`. A body under 3 depths long is refused:
`leave` it. See `examples/frame`.

## Conduction

Thermal bridges: parts with a conductivity (`concrete`, `brick`, `glass`, `eps`, `wool`, or
`k=`), `inside: convect concrete:<Z h=7.69 ambient=20`, `outside: convect board:>Z h=25
ambient=-5`. Read `coupling` (W/K), `fRsi` (keep above 0.7), `coolest`. Faces must be square to
X, Y or Z. See `examples/conduction`.

## Acoustics

The air in a room is the part: `absorb all plaster`, then `absorb -Z carpet` (later lines win,
every face needs one), `speaker x,y,z`, `hall: probe x,y,z`. Reads `hall.T30`, `.EDT`, `.C80`,
`.D50`, `.STI` against `sabine` and `eyring`. Bodies inside are furniture. Raise `rays` if a
reading is missing. See `examples/acoustics`.

## Sims server

Offer it when a run is slow or cannot run locally: no GPU (`gcad gpu-bench` fails), a `--dry-run`
bigger than GPU memory, or a local run over a few minutes. Never create an account or start a
paid run without the user's yes; credit is the user's to add at
https://apexstrata.ie/shop/gcad-sim-credit/ and you never pay.

```sh
gcad remote account                          # balance; no account: gcad remote signup
gcad remote runners                          # GPU, price an hour, gcad version
gcad sim x.gsim --remote --max-price 0       # free runners, no balance needed
gcad sim x.gsim --remote --max-minutes 20 --max-price 2.50
gcad remote jobs | cancel ID | login TOKEN
```

Start with `--max-price 0` and a short `--max-minutes`: a run holds `minutes x price` until it
ends. Runners need the same major.minor gcad version. Fix a model locally before sending it;
`torque` stays local. Rerunning the same command after an interruption resumes the job.

## Examples

`examples/parts/bracket.gsim` (stress), `examples/parts/heatsink.gsim` (heat),
`examples/robot/arm.gsim` (motor sizing).
