# Magnetic simulation plan

`sim magnetic` solves the magnetic field of motors, actuators and couplings: torque and force
on the moving part, cogging, torque per amp, back-EMF, and later the losses that heat them.
It starts on a 2D slice through the motor, the way FEMM does, and moves to the 3D tet mesh on
the same spec lines.

## The spec

```
sim magnetic
part motor.gasm
turn rotor
length 30
magnet magnets* Br=1.25 toward=radial
iron stator rotor_core material=m19
coil a+ a- turns=40 current=5
sweep rotor steps=60
mesh 0.4
expect cogging<0.02
```

- `turn joint sweep= steps=` is the rotor's `turn` joint; its axis is the slice normal.
- `slice at=` places the 2D slice along that axis, the middle of the parts unless given.
  `length` is the stack length the 2D answer is multiplied by, the rotor's own length unless
  given.
- `magnet parts Br= toward=` names permanent magnets: `toward=radial`, `-radial`, `x,y,z`
  (parallel), or `halbach=n` for n segments per pole pair. `mu=` is the recoil
  permeability, 1.05 unless given. Materials can carry `Br` and `mu` so `magnet` can be left out.
- `iron parts material=` names steel by its B-H curve. The library carries common electrical
  steels (M19, M27, M330-35A, 1010 low carbon, cast iron); a material table can add its own as
  `bh = [[H, B], ...]`.
- `coil parts turns= current=` gives a winding's turns and current; a coil side and its
  return are `a+` and `a-`. `phase a parts...` groups coils into a phase, and `drive
  a=5 b=-2.5 c=-2.5` or `drive sine amps=5 advance=0` sets the phase currents for a sweep.
- `sweep joint steps=` turns the rotor through a pole pitch (or a full turn with `full`).

Readings: `torque` (N·m about the joint), `force.x/y/z` on the moving part, `cogging`
(peak to peak torque with no current), `kt` (torque per amp), `flux.a` (flux linkage per
phase, Wb), `emf.a` (peak back-EMF per 1000 rpm), `airgap` (peak airgap flux density), and
`b` (peak flux density in each iron part). The field is |B| on the slice.

## Method

- **The slice.** Each part is cut by the plane and its section becomes regions; the air
  around them fills a disc three times the parts' radius. The airgap is a ring, split along
  a circle with `n` evenly spaced nodes, rotor and stator meshed apart with spade's
  constrained Delaunay refinement and joined on that circle. The rotor turns in whole node
  pitches, so the mesh is never rebuilt during a sweep and cogging is not buried in meshing
  noise.
- **The field.** The vector potential A_z on first-order triangles, as in FEMM. Magnets enter as their
  magnetization's equivalent current, coils as `N I / area` over their section. The far
  boundary holds A_z at 0.
- **Iron.** The reluctivity follows the B-H curve, solved by Newton's method on the
  energy with a line search; a monotone cubic through the table keeps the Jacobian
  continuous.
- **Torque.** Arkkio's method, the Maxwell stress averaged over the whole airgap ring, which
  is far less sensitive to the mesh than a single contour. The coenergy derivative over the
  sweep is a second answer the tests compare against it.
- **Back-EMF.** Flux linkage per phase from A_z over the coil sides (times turns and length),
  differentiated over the sweep.

## Phases

1. **Linear 2D magnetostatics.** Slice mesher, triangle FEM, magnets and coils, linear iron.
   Checks: the field inside and outside a long round wire and a round bar magnet against the
   closed forms; a slotless surface magnet motor's airgap field and torque against Zhu's
   analytical model.
2. **Nonlinear iron.** B-H curves and Newton. Check: a C-core actuator's force against FEMM
   (`tools/femm/`, xfemm in Docker) at currents that saturate it.
3. **Rotation.** Airgap split and sweeps: cogging, `kt`, back-EMF. Check: a 12 slot 10 pole
   surface magnet motor against FEMM over a full electrical cycle.
4. **`sim magnetic`.** The spec, readings, render, viewer.
5. **Losses.** Time-harmonic A_z with `j omega sigma A` for eddy currents in magnets, sleeves
   and conductors (AC copper loss); iron loss from the B waveform over a sweep with Bertotti's
   hysteresis, eddy and excess terms. The loss becomes `heat` for `sim thermal` and `sim flow`.
   Check: a conducting plate in a sinusoidal field against the skin-depth solution, and
   FEMM's time-harmonic solver.
6. **3D.** Reduced and total scalar potentials on the second-order tets for magnets and iron,
   coils as Biot-Savart source fields, Maxwell stress on a surface around the moving part.
   Checks: TEAM problem 13 (nonlinear steel plates around a coil, measured), and the 2D motor
   with a long stack against phase 3.

## Status

Phases 1 to 4 are built for magnets, coils and steel (`src/sim/magnetic/`, the spec lines in
`docs/format.md`, `examples/magnetic/`). Not yet: `phase` and `drive` (sinusoidal currents that
follow the rotor through a sweep), `kt`, the force on a part that slides, losses, and 3D.

- **Slice mesher** (`plane.rs`). Every outline is resampled to the element size and put into
  spade's constrained Delaunay triangulation with points closer than a fifth of an element
  merged, so two parts that touch share their boundary. Three circles of `n` evenly spaced
  points sit in the airgap at a quarter, half and three quarters of it; refinement keeps
  constraint edges whole, so the rings stay aligned. Triangles inside the middle circle get
  their own copies of its points, and a rotation by `k` steps ties copy `j` to stator point
  `j + k`.
- **Solver** (`field.rs`). The energy is minimized by Newton's method with a line search on
  it; each step is a sparse Cholesky solve. Steel's H(B) is a monotone cubic through the table
  (Fritsch and Butland slopes, so it never wiggles) carried on at the slope of air past the
  last point, and its energy is the exact integral of that cubic. The outer circle is
  open: `dA/dr = -A/r` there, exact for a dipole and only an offset for a net current. Holding
  `A = 0` on it instead read a magnet's outside field 4% low with the air three times the
  test's radius away, since a dipole's image in a fixed circle falls off as `(r/R)^2`.
- **Steel tables** are FEMM's own `M-19 Steel` and `1010 Steel`, read from its `matlib.dat`.

Checks (`tests/magnetic.rs`):

1. A round wire's field inside and out, element by element: 0.9% RMS of the surface field
   on 0.5 mm elements (a first-order triangle holds one value of B).
2. A round magnet holds half its remanence inside (0.599 T of 0.6), and its dipole field
   outside within 0.7% RMS.
3. A saturating M-19 toroid wound inside and out, at 100, 600 and 3000 ampere-turns: the
   flux between the windings matches the integral of the B-H curve within 1%.
4. A magnet turned by a wire beside it: the torque from the wire's force on the magnet's
   dipole field (the reaction is exact in 2D) within 1% at every angle, and the coenergy's
   derivative within 3%. Gap rings of 360 and 720 points and elements of 0.4 to 0.1 mm all read
   within 1%.
5. The 12 slot 10 pole surface magnet motor in the test (M-19 stator, 1010 rotor, 1.2 T
   magnets, 400 ampere-turns a coil side), against xfemm built from
   `github.com/crobarcro/xfemm` (`femm_motor_reference` writes the same geometry as a FEMM
   Lua script, smart meshing off). Loaded torque at seven angles agrees within 0.4%:

| angle | FEMM | gcad |
|---|---|---|
| 0° | -25.785 N·m/m | -25.791 |
| 1.5° | -38.871 | -38.804 |
| 3° | -49.969 | -49.938 |

   Cogging is 1% of that, and FEMM's (a new mesh at each angle) wanders by about 0.05 N·m/m
   between neighbouring angles. The fixed mesh here is symmetric about the mid pitch to 9% at
   0.5 mm and settles to a peak of 0.533 N·m/m at 0.15 mm (0.565 at 0.5 mm, FEMM 0.50 to 0.52).

FEMM's default smart mesh put 700,000 nodes on that motor and its conjugate gradient solve did
not finish in five minutes; the reference turns it off with `[DoSmartMesh] = 0`. A shaft hole
drawn with 720 sides (0.05 mm segments) had the same effect on both meshers.
