use super::args::{Args, label_of, positive};
use super::solids::{classify, loops, prism, regions};
use super::{Combine, Model};
use crate::geometry::{self, Profile, Segment};
use crate::parse::Line;
use crate::select;
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

struct Hit {
    point: Point3,
    face: Option<usize>,
}

impl Model {
    pub(crate) fn op_rib(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["t"], &[], false)?;
        let thickness = positive(args.number("t", &self.scope)?, "rib thickness")?;
        let pen = self
            .pen
            .clone()
            .ok_or_else(|| anyhow!("`rib` needs an open `pen` path of one `line`"))?;
        let [Segment::Line(to)] = pen.segments.as_slice() else {
            bail!(
                "`rib` follows one straight `line`; the pen path has {} segment(s)",
                pen.segments.len()
            );
        };
        let frame = self.sketch_frame();
        let solid = self.active("rib")?.clone();
        let (a, b) = (frame.at(pen.start.0, pen.start.1), frame.at(to.0, to.1));
        let along = (b - a).normalize();
        let mesh = geometry::mesh(&solid, geometry::mesh_tolerance(&solid));
        let faces = select::faces(&solid);
        let tolerance = self.tolerance() * 100.0;
        let face_at = |point: Point3, facing: Vector3| {
            faces.iter().position(|face| match face.oriented_surface() {
                Surface::Plane(plane) => {
                    plane.normal().dot(facing) < -1.0e-9
                        && (point - plane.origin()).dot(plane.normal()).abs() < tolerance
                }
                _ => false,
            })
        };
        let cast = |origin: Point3, direction: Vector3| -> Option<Hit> {
            geometry::ray_hits(&mesh, origin, direction)
                .into_iter()
                .filter(|(t, n)| *t > tolerance && n.dot(direction) < 0.0)
                .min_by(|x, y| x.0.total_cmp(&y.0))
                .map(|(t, _)| {
                    let point = origin + direction * t;
                    Hit {
                        point,
                        face: face_at(point, direction),
                    }
                })
        };
        let middle = a.midpoint(b);
        let side = frame.normal.cross(along);
        let inward = [side, -side]
            .into_iter()
            .filter_map(|n| cast(middle, n).map(|hit| (hit.point.distance(middle), n)))
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, n)| n)
            .ok_or_else(|| anyhow!("the rib line has no material beside it to join"))?;
        let first = cast(middle, -along)
            .ok_or_else(|| anyhow!("the rib line does not reach the solid at its start"))?;
        let last = cast(middle, along)
            .ok_or_else(|| anyhow!("the rib line does not reach the solid at its end"))?;
        let span = first.point.distance(last.point);
        let from = first.point;
        let mut chain: Vec<Hit> = vec![first];
        chain.extend((1..64).filter_map(|i| cast(from + along * (span * i as f64 / 64.0), inward)));
        let (p0, p1) = (chain[0].point, last.point);
        chain.push(last);
        let local = |p: Point3| frame.local(p);
        let line_of = |hit: &Hit| -> Option<((f64, f64), (f64, f64))> {
            let face = hit.face?;
            let Surface::Plane(plane) = faces[face].oriented_surface() else {
                return None;
            };
            let direction = plane.normal().cross(frame.normal);
            (direction.magnitude() > 1.0e-9).then(|| {
                (
                    local(hit.point),
                    (direction.dot(frame.x), direction.dot(frame.y)),
                )
            })
        };
        let mut boundary: Vec<(f64, f64)> = vec![local(p0)];
        for pair in chain.windows(2) {
            match (pair[0].face, pair[1].face) {
                (Some(f), Some(g)) if f == g => {}
                _ => match (line_of(&pair[0]), line_of(&pair[1])) {
                    (Some((p, d)), Some((q, e))) => {
                        let denominator = d.0 * e.1 - d.1 * e.0;
                        if denominator.abs() > 1.0e-12 {
                            let t = ((q.0 - p.0) * e.1 - (q.1 - p.1) * e.0) / denominator;
                            boundary.push((p.0 + d.0 * t, p.1 + d.1 * t));
                        }
                    }
                    _ => boundary.push(local(pair[1].point)),
                },
            }
        }
        boundary.push(local(p1));
        boundary.dedup_by(|x, y| (x.0 - y.0).hypot(x.1 - y.1) < 1.0e-9);
        if boundary.len() < 3 {
            bail!("the rib line and the solid enclose nothing");
        }
        let start = boundary[boundary.len() - 1];
        let segments: Vec<Segment> = boundary.iter().map(|p| Segment::Line(*p)).collect();
        let pad = (span * 0.02).max(1.0e-3);
        let mut distances = vec![-pad; segments.len()];
        distances[0] = 0.0;
        let (start, segments) = crate::offset::offset_path(start, &segments, &distances)
            .map_err(|e| anyhow!("rib: {e}"))?;
        let profiles = vec![Profile::Path { start, segments }];
        let base = frame.offset(-thickness / 2.0);
        let shapes = loops(&base, &profiles)?;
        let label = label_of(line);
        for region in regions(&shapes) {
            let tool = prism(
                &base,
                &shapes,
                &region,
                &profiles,
                frame.normal * thickness,
                (0.0, 0.0),
            )?;
            let groups = classify(&tool, frame.normal);
            self.record(&label, groups);
            self.merge(&label, tool, Combine::Add)?;
        }
        self.pen = None;
        self.describe_solid()
    }
}
