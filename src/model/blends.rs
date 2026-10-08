use super::args::{Args, label_of, positive};
use super::solids::{loops, prism};
use super::{Combine, Model};
use crate::geometry::{self, Frame, Profile};
use crate::parse::Line;
use crate::select;
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

fn plane_normal(face: &Face) -> Option<Vector3> {
    match face.oriented_surface() {
        Surface::Plane(plane) => Some(plane.normal()),
        _ => None,
    }
}

fn smooth(edge: &Edge, owners: &[usize], faces: &[Face]) -> bool {
    let curve = edge.curve();
    let (t0, t1) = curve.range_tuple();
    let point = curve.subs((t0 + t1) / 2.0);
    let normals: Vec<Vector3> = owners
        .iter()
        .map(|&i| &faces[i])
        .filter_map(|face| {
            let surface = face.oriented_surface();
            let (u, v) = surface.search_parameter(point, None, 100)?;
            Some(surface.normal(u, v))
        })
        .collect();
    matches!(normals.as_slice(), [a, b] if a.dot(*b) > 1.0 - 1.0e-6)
}

fn into_face(edge: Vector3, own: Vector3, other: Vector3) -> Vector3 {
    let u = edge.cross(own).normalize();
    if u.dot(other) < 0.0 { u } else { -u }
}

fn by_corner(edges: &[Edge]) -> (Vec<Edge>, Vec<Edge>) {
    let mut group: Vec<usize> = (0..edges.len()).collect();
    fn root(group: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while group[i] != i {
            group[i] = group[group[i]];
            i = group[i];
        }
        i
    }
    let ends = |e: &Edge| [e.front().id(), e.back().id()];
    for i in 0..edges.len() {
        for j in i + 1..edges.len() {
            if ends(&edges[i]).iter().any(|v| ends(&edges[j]).contains(v)) {
                let (a, b) = (root(&mut group, i), root(&mut group, j));
                group[a] = b;
            }
        }
    }
    let busy: Vec<bool> = edges
        .iter()
        .map(|edge| {
            ends(edge)
                .iter()
                .any(|v| edges.iter().filter(|other| ends(other).contains(v)).count() >= 3)
        })
        .collect();
    let cornered_roots: Vec<usize> = (0..edges.len())
        .filter(|&i| busy[i])
        .map(|i| root(&mut group, i))
        .collect();
    let (cornered, chained): (Vec<usize>, Vec<usize>) =
        (0..edges.len()).partition(|&i| cornered_roots.contains(&root(&mut group, i)));
    (
        cornered.into_iter().map(|i| edges[i].clone()).collect(),
        chained.into_iter().map(|i| edges[i].clone()).collect(),
    )
}

fn turning(edge: &Edge) -> f64 {
    let curve = edge.curve();
    let (t0, t1) = curve.range_tuple();
    let tangents: Vec<Vector3> = (0..=16)
        .map(|i| curve.der(t0 + (t1 - t0) * i as f64 / 16.0))
        .filter(|d| d.magnitude() > 1.0e-12)
        .map(|d| d.normalize())
        .collect();
    tangents
        .windows(2)
        .map(|pair| pair[0].dot(pair[1]).clamp(-1.0, 1.0).acos())
        .sum()
}

pub(crate) fn divisions(edges: &[Edge]) -> std::num::NonZeroUsize {
    let most = edges.iter().map(turning).fold(0.0, f64::max);
    let count = (most / 3.0_f64.to_radians()).ceil().clamp(5.0, 48.0) as usize;
    std::num::NonZeroUsize::new(count).expect("at least five")
}

pub(crate) fn kernel_blend(
    solid: &Solid,
    edges: &[Edge],
    options: &FilletOptions,
    op: &str,
    selector: &str,
) -> Result<Solid> {
    let count = select::faces(solid).len();
    let shells = solid
        .boundaries()
        .iter()
        .map(|shell| {
            let mut shell = shell.clone();
            let owned: Vec<Edge> = edges
                .iter()
                .filter(|edge| shell.edge_iter().any(|own| own.is_same(edge)))
                .cloned()
                .collect();
            if !owned.is_empty() {
                fillet_edges(&mut shell, &owned, Some(options))
                    .map_err(|error| anyhow!("{op} of {} edge(s) failed: {error}", owned.len()))?;
            }
            Ok(shell)
        })
        .collect::<Result<Vec<_>>>()?;
    let result =
        Solid::try_new(shells).map_err(|error| anyhow!("{op} left an invalid solid: {error}"))?;
    if select::faces(&result).len() <= count {
        bail!("{op} could not change `{selector}`, the solid is unchanged");
    }
    Ok(super::exact::exact_edges(&result))
}

impl Model {
    pub(crate) fn op_blend(&mut self, line: &Line, profile: FilletProfile) -> Result<String> {
        let args = Args::new(line, &["size", "edges"], &["to", "d2"], false)?;
        if args.text("size")? == "full" && matches!(profile, FilletProfile::Round) {
            return self.full_round(line, args.text("edges")?);
        }
        let size = positive(args.number("size", &self.scope)?, "size")?;
        let selector = args.text("edges")?;
        let solid = self.active(&line.op)?.clone();
        let tolerance = self.tolerance();
        let edges = select::select_edges(selector, &solid, &self.groups, tolerance)?;
        if edges.is_empty() {
            bail!("`{selector}` matched no edges");
        }
        let faces = select::faces(&solid);
        let owners = select::edge_owners(&faces);
        let edges: Vec<Edge> = edges
            .into_iter()
            .filter(|edge| {
                let mine = owners
                    .get(&edge.id())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                !smooth(edge, mine, &faces)
            })
            .collect();
        if edges.is_empty() {
            bail!(
                "every edge of `{selector}` joins faces smoothly, so there is no corner to round"
            );
        }
        if let Some(second) = args.optional_number("d2", &self.scope)? {
            if !matches!(profile, FilletProfile::Chamfer) {
                bail!("`d2=` is for chamfers");
            }
            return self.two_distance_chamfer(
                line,
                selector,
                &edges,
                size,
                positive(second, "d2")?,
            );
        }
        let flat = matches!(profile, FilletProfile::Chamfer);
        let (cornered, chained) = if args.has("to") {
            (Vec::new(), edges.clone())
        } else {
            by_corner(&edges)
        };
        let options = match args.optional_number("to", &self.scope)? {
            Some(end) => {
                let end = positive(end, "to")?;
                let closed = edges.iter().all(|edge| {
                    [edge.front(), edge.back()].iter().all(|vertex| {
                        edges
                            .iter()
                            .filter(|other| other.front() == *vertex || other.back() == *vertex)
                            .count()
                            == 2
                    })
                });
                if closed {
                    FilletOptions::variable(move |t| {
                        size + (end - size) * (1.0 - (2.0 * t - 1.0).abs())
                    })
                } else {
                    FilletOptions::variable(move |t| size + (end - size) * t)
                }
            }
            None => FilletOptions::constant(size),
        }
        .with_profile(profile)
        .with_division(divisions(&chained));
        let before: Vec<Surface> = select::faces(&solid)
            .iter()
            .map(|face| face.oriented_surface())
            .collect();
        let mut result = solid.clone();
        if !cornered.is_empty() {
            result = super::round::round_edges(&result, &cornered, size, flat)
                .map_err(|error| anyhow!("rounding a corner where three edges meet: {error}"))?;
        }
        if !chained.is_empty() {
            result = kernel_blend(&result, &chained, &options, &line.op, selector)?;
        }
        if !flat && !args.has("to") {
            self.rounded.extend(
                edges
                    .iter()
                    .filter(|edge| super::round::straight(edge))
                    .map(|edge| super::Rounded {
                        from: edge.front().point(),
                        to: edge.back().point(),
                        radius: size,
                    }),
            );
        }
        let label = label_of(line);
        select::faces(&result)
            .iter()
            .filter(|face| {
                !before
                    .iter()
                    .any(|surface| select::face_on(face, surface, tolerance))
            })
            .for_each(|face| self.groups.record(&label, "faces", face.oriented_surface()));
        self.solid = Some(result);
        Ok(format!(
            "{} edge(s); {}",
            edges.len(),
            self.describe_solid()?
        ))
    }

    fn full_round(&mut self, line: &Line, selector: &str) -> Result<String> {
        let (owner, group) = selector
            .split_once('.')
            .filter(|(_, g)| *g == "end" || *g == "start")
            .ok_or_else(|| {
                anyhow!("`fillet full` rounds an extrusion's `label.end` or `label.start`")
            })?;
        let prism_of = self
            .prisms
            .get(owner)
            .cloned()
            .ok_or_else(|| anyhow!("`{owner}` is not an extrusion"))?;
        let [
            Profile::Rect {
                center,
                width,
                height,
                radius,
            },
        ] = prism_of.profiles.as_slice()
        else {
            bail!("`fillet full` rounds the end of an extruded rect");
        };
        if *radius > 0.0 {
            bail!("`fillet full` rounds the end of a rect with square corners");
        }
        let alone = prism_of.alone.is_some()
            && prism_of.alone.as_ref()
                == Some(
                    &select::faces(self.active("fillet")?)
                        .iter()
                        .map(Face::id)
                        .collect(),
                );
        if !alone {
            bail!(
                "`fillet full` needs `{owner}` to be the whole solid; round it before adding other features"
            );
        }
        let frame = prism_of.frame;
        let along = frame.normal * prism_of.distance.signum();
        let length = prism_of.distance.abs();
        let (long, short, across, long_size) = if width >= height {
            (frame.x, frame.y, *height, *width)
        } else {
            (frame.y, frame.x, *width, *height)
        };
        let r = across / 2.0;
        if r >= length {
            bail!("the extrusion is {length} long, too short for a full round of radius {r}");
        }
        let (base, up) = if group == "end" {
            (frame.at(center.0, center.1), along)
        } else {
            (frame.at(center.0, center.1) + along * length, -along)
        };
        let section = Frame {
            origin: base - long * (long_size / 2.0),
            x: short,
            y: up,
            normal: long,
        };
        let straight = length - r;
        let profiles = vec![Profile::Path {
            start: (-r, 0.0),
            segments: vec![
                crate::geometry::Segment::Line((r, 0.0)),
                crate::geometry::Segment::Line((r, straight)),
                crate::geometry::Segment::Arc {
                    to: (-r, straight),
                    via: (0.0, length),
                },
                crate::geometry::Segment::Line((-r, 0.0)),
            ],
        }];
        let shapes = loops(&section, &profiles)?;
        let tool = prism(
            &section,
            &shapes,
            &[0],
            &profiles,
            long * long_size,
            (0.0, 0.0),
        )?;
        let label = label_of(line);
        let groups = select::faces(&tool)
            .iter()
            .filter(|face| !matches!(face.oriented_surface(), Surface::Plane(_)))
            .map(|face| ("faces", face.oriented_surface()))
            .collect();
        self.record(&label, groups);
        self.solid = None;
        self.merge(&label, tool, Combine::Add)?;
        self.describe_solid()
    }

    fn two_distance_chamfer(
        &mut self,
        line: &Line,
        selector: &str,
        edges: &[Edge],
        first: f64,
        second: f64,
    ) -> Result<String> {
        let solid = self.active("chamfer")?.clone();
        let faces = select::faces(&solid);
        let on_first = if first == second {
            Vec::new()
        } else {
            let (first_set, _) = selector
                .split_once('&')
                .filter(|_| !selector.contains('|'))
                .ok_or_else(|| anyhow!("`d2=` needs edges written as `a&b`: `{first}` is cut along faces `a`, `d2` along `b`"))?;
            select::select_faces(first_set, &solid, &self.groups, self.tolerance())?
        };
        let margin = (geometry::bounds(&solid).diameter() * 0.01).max(1.0e-3);
        let label = label_of(line);
        let mut tools = Vec::new();
        for edge in edges {
            let (a, b) = (edge.front().point(), edge.back().point());
            let curve = edge.curve();
            let (t0, t1) = curve.range_tuple();
            let middle = curve.subs((t0 + t1) / 2.0);
            if (middle - a.midpoint(b)).magnitude() > margin * 1.0e-3 {
                bail!("this chamfer works on straight edges; one of `{selector}` is curved");
            }
            let owners: Vec<usize> = (0..faces.len())
                .filter(|&i| faces[i].edge_iter().any(|e| e.is_same(edge)))
                .collect();
            let [f0, f1] = owners[..] else {
                bail!("an edge of `{selector}` does not join two faces")
            };
            let (fa, fb) = if on_first.contains(&f0) {
                (f0, f1)
            } else {
                (f1, f0)
            };
            let (na, nb) = plane_normal(&faces[fa])
                .zip(plane_normal(&faces[fb]))
                .ok_or_else(|| anyhow!("this chamfer works on edges between flat faces"))?;
            let along = (b - a).normalize();
            let (ua, ub) = (into_face(along, na, nb), into_face(along, nb, na));
            let frame = Frame::from_normal(a - along * margin, along);
            let corner = |p: Point3| frame.local(p - along * margin);
            let (on_a, on_b) = (a + ua * first, a + ub * second);
            let slant = (on_a - on_b).normalize();
            let reach = margin + first + second;
            let points = vec![
                corner(on_a + slant * margin),
                corner(a + (na + nb) * reach),
                corner(on_b - slant * margin),
            ];
            let cutter = Profile::Polygon { points };
            let shapes = loops(&frame, std::slice::from_ref(&cutter))?;
            tools.push(prism(
                &frame,
                &shapes,
                &[0],
                std::slice::from_ref(&cutter),
                along * ((b - a).magnitude() + 2.0 * margin),
                (0.0, 0.0),
            )?);
        }
        for tool in tools {
            self.merge(&label, tool, Combine::Remove)?;
        }
        Ok(format!(
            "{} edge(s); {}",
            edges.len(),
            self.describe_solid()?
        ))
    }
}
