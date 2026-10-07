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

fn into_face(edge: Vector3, own: Vector3, other: Vector3) -> Vector3 {
    let u = edge.cross(own).normalize();
    if u.dot(other) < 0.0 { u } else { -u }
}

impl Model {
    pub(crate) fn op_blend(&mut self, line: &Line, profile: FilletProfile) -> Result<String> {
        let args = Args::new(line, &["size", "edges"], &["to", "d2"], false)?;
        let size = positive(args.number("size", &self.scope)?, "size")?;
        let selector = args.text("edges")?;
        let solid = self.active(&line.op)?.clone();
        let tolerance = self.tolerance();
        let edges = select::select_edges(selector, &solid, &self.groups, tolerance)?;
        if edges.is_empty() {
            bail!("`{selector}` matched no edges");
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
        let options = match args.optional_number("to", &self.scope)? {
            Some(end) => {
                let end = positive(end, "to")?;
                FilletOptions::variable(move |t| size + (end - size) * t)
            }
            None => FilletOptions::constant(size),
        }
        .with_profile(profile);
        let before: Vec<Surface> = select::faces(&solid)
            .iter()
            .map(|face| face.oriented_surface())
            .collect();
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
                    fillet_edges(&mut shell, &owned, Some(&options)).map_err(|error| {
                        anyhow!("{} of {} edge(s) failed: {error}", line.op, owned.len())
                    })?;
                }
                Ok(shell)
            })
            .collect::<Result<Vec<_>>>()?;
        let result = Solid::try_new(shells)
            .map_err(|error| anyhow!("{} left an invalid solid: {error}", line.op))?;
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

    fn two_distance_chamfer(
        &mut self,
        line: &Line,
        selector: &str,
        edges: &[Edge],
        first: f64,
        second: f64,
    ) -> Result<String> {
        let (first_set, _) = selector
            .split_once('&')
            .filter(|_| !selector.contains('|'))
            .ok_or_else(|| anyhow!("`d2=` needs edges written as `a&b`: `{first}` is cut along faces `a`, `d2` along `b`"))?;
        let solid = self.active("chamfer")?.clone();
        let faces = select::faces(&solid);
        let on_first = select::select_faces(first_set, &solid, &self.groups, self.tolerance())?;
        let margin = (geometry::bounds(&solid).diameter() * 0.01).max(1.0e-3);
        let label = label_of(line);
        let mut tools = Vec::new();
        for edge in edges {
            let (a, b) = (edge.front().point(), edge.back().point());
            let curve = edge.curve();
            let (t0, t1) = curve.range_tuple();
            let middle = curve.subs((t0 + t1) / 2.0);
            if (middle - a.midpoint(b)).magnitude() > margin * 1.0e-3 {
                bail!("`d2=` chamfers straight edges; one of `{selector}` is curved");
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
                .ok_or_else(|| anyhow!("`d2=` chamfers edges between flat faces"))?;
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
