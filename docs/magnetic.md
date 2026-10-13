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

- `turn joint` is the rotor's `turn` joint; its axis is the slice normal.
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
- **The field.** The vector potential A_z on second-order triangles. Magnets enter as their
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
