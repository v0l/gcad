use super::Model;
use crate::geometry::{self, Frame, Profile};
use crate::select;
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

pub(crate) struct Loop {
    pub(crate) wire: Wire,
    pub(crate) outline: Vec<(f64, f64)>,
    pub(crate) profile: usize,
    pub(crate) whole: bool,
}

fn outline(wire: &Wire, frame: &Frame) -> Vec<(f64, f64)> {
    wire.edge_iter()
        .flat_map(|edge| {
            let curve = edge.oriented_curve();
            let (t0, t1) = curve.range_tuple();
            (0..16)
                .map(move |i| frame.local(curve.subs(t0 + (t1 - t0) * i as f64 / 16.0)))
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(crate) fn loops(frame: &Frame, profiles: &[Profile]) -> Result<Vec<Loop>> {
    let mut result = Vec::new();
    for (index, profile) in profiles.iter().enumerate() {
        let wires = profile.wires(frame).map_err(|error| anyhow!(error))?;
        let whole = wires.len() == 1;
        result.extend(wires.into_iter().map(|wire| Loop {
            outline: outline(&wire, frame),
            wire,
            profile: index,
            whole,
        }));
    }
    Ok(result)
}

fn contains(polygon: &[(f64, f64)], (x, y): (f64, f64)) -> bool {
    (0..polygon.len()).fold(false, |inside, i| {
        let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
        if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
            !inside
        } else {
            inside
        }
    })
}

pub(crate) fn regions(loops: &[Loop]) -> Vec<Vec<usize>> {
    let encloses = |outer: usize, inner: usize| {
        outer != inner
            && loops[inner]
                .outline
                .iter()
                .all(|&point| contains(&loops[outer].outline, point))
    };
    let containers: Vec<Vec<usize>> = (0..loops.len())
        .map(|i| (0..loops.len()).filter(|&j| encloses(j, i)).collect())
        .collect();
    let depth = |i: usize| containers[i].len();
    let parent = |i: usize| containers[i].iter().copied().max_by_key(|&j| depth(j));
    (0..loops.len())
        .filter(|&i| depth(i) % 2 == 0)
        .map(|outer| {
            std::iter::once(outer)
                .chain((0..loops.len()).filter(|&i| depth(i) % 2 == 1 && parent(i) == Some(outer)))
                .collect()
        })
        .collect()
}

pub(crate) fn oriented(mut solid: Solid) -> Solid {
    if geometry::volume(&solid) < 0.0 {
        solid.not();
    }
    solid
}

pub(crate) fn prism(
    frame: &Frame,
    loops: &[Loop],
    region: &[usize],
    profiles: &[Profile],
    direction: Vector3,
    insets: (f64, f64),
) -> Result<Solid> {
    if insets.0 == 0.0 && insets.1 == 0.0 {
        let wires: Vec<Wire> = region.iter().map(|&i| loops[i].wire.clone()).collect();
        let solid: Solid = profile::solid_from_planar_profile(wires, direction)
            .map_err(|error| anyhow!("cannot extrude the sketch: {error}"))?;
        return Ok(oriented(solid));
    }
    let [only] = region else {
        bail!("draft works on a profile without holes; cut the holes in a separate line");
    };
    if !loops[*only].whole {
        bail!("draft works on rect, circle, poly and slot");
    }
    tapered(frame, &profiles[loops[*only].profile], direction, insets)
}

pub(crate) fn tapered(
    frame: &Frame,
    profile: &Profile,
    direction: Vector3,
    (start, end): (f64, f64),
) -> Result<Solid> {
    let top_frame = Frame {
        origin: frame.origin + direction,
        ..*frame
    };
    let bottom = profile
        .inset(start)
        .map_err(|error| anyhow!(error))?
        .wire(frame);
    let top = profile
        .inset(end)
        .map_err(|error| anyhow!(error))?
        .wire(&top_frame);
    loft_wires(&[bottom, top])
}

pub(crate) fn loft_wires(wires: &[Wire]) -> Result<Solid> {
    let mut shell: Shell =
        builder::try_skin_wires(wires).map_err(|error| anyhow!("cannot loft: {error}"))?;
    let cap = |wire: Wire| -> Result<Face> {
        builder::try_attach_plane(&[wire]).map_err(|error| anyhow!("cannot cap the loft: {error}"))
    };
    let first = wires.first().ok_or_else(|| anyhow!("nothing to loft"))?;
    let last = wires.last().ok_or_else(|| anyhow!("nothing to loft"))?;
    shell.push(cap(first.inverse())?);
    shell.push(cap(last.clone())?);
    let solid = Solid::try_new(vec![shell])
        .map_err(|error| anyhow!("lofted solid is not closed: {error}"))?;
    Ok(oriented(solid))
}

pub(crate) fn classify(solid: &Solid, direction: Vector3) -> Vec<(&'static str, Surface)> {
    let along = direction.normalize();
    select::faces(solid)
        .iter()
        .map(|face| {
            let surface = face.oriented_surface();
            let group = match &surface {
                Surface::Plane(plane) if plane.normal().dot(along) < -0.999 => "start",
                Surface::Plane(plane) if plane.normal().dot(along) > 0.999 => "end",
                _ => "side",
            };
            (group, surface)
        })
        .collect()
}

impl Model {
    pub(crate) fn record(&mut self, label: &str, groups: Vec<(&str, Surface)>) {
        groups
            .into_iter()
            .for_each(|(group, surface)| self.groups.record(label, group, surface));
    }

    pub(crate) fn record_inverted(&mut self, label: &str, groups: Vec<(&str, Surface)>) {
        groups.into_iter().for_each(|(group, mut surface)| {
            surface.invert();
            self.groups.record(label, group, surface);
        });
    }
}
