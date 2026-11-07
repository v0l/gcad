use super::args::Args;
use super::skin::{interpolate, knots, uniform};
use super::solids::{loops, oriented, prism, regions};
use super::{Combine, Model, label_of};
use crate::geometry;
use crate::parse::Line;
use crate::select;
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;
use std::ops::Bound;

#[derive(Clone, Copy)]
struct Roll {
    centre: Point3,
    axis: Vector3,
    out: Vector3,
    along: Vector3,
    origin: Point3,
    normal: Vector3,
    radius: f64,
    outer: f64,
}

impl Roll {
    fn apply(&self, q: Point3) -> Point3 {
        let a = (q - self.centre).dot(self.axis);
        let s = (q - self.centre).dot(self.along);
        let t = (q - self.origin).dot(self.normal);
        let r = self.outer - t;
        let angle = s / self.radius;
        self.centre + self.axis * a + (self.out * angle.cos() + self.along * angle.sin()) * r
    }
}

const CURVE_SAMPLES: usize = 33;
const SURFACE_SAMPLES: usize = 17;

fn fit_curve(points: &[Point3]) -> Result<BsplineCurve<Point3>> {
    let params = uniform(points.len());
    let knots = knots(&params, 3);
    Ok(BsplineCurve::new(
        knots.clone(),
        interpolate(points, &params, &knots)?,
    ))
}

fn rolled_curve(curve: &Curve, roll: &Roll) -> Result<Curve> {
    let (t0, t1) = curve.range_tuple();
    let points: Vec<Point3> = (0..CURVE_SAMPLES)
        .map(|i| roll.apply(curve.subs(t0 + (t1 - t0) * i as f64 / (CURVE_SAMPLES - 1) as f64)))
        .collect();
    Ok(Curve::BsplineCurve(fit_curve(&points)?))
}

fn finite(range: (Bound<f64>, Bound<f64>)) -> Option<(f64, f64)> {
    match range {
        (Bound::Included(a), Bound::Included(b)) => Some((a, b)),
        _ => None,
    }
}

fn plane_range(plane: &Plane, corners: &[Point3]) -> ((f64, f64), (f64, f64)) {
    let (u, v) = (plane.axis_u(), plane.axis_v());
    let gram = Matrix2::new(u.dot(u), u.dot(v), u.dot(v), v.dot(v));
    let inverse = gram.invert().unwrap_or_else(Matrix2::identity);
    let (mut ur, mut vr) = (
        (f64::INFINITY, f64::NEG_INFINITY),
        (f64::INFINITY, f64::NEG_INFINITY),
    );
    for corner in corners {
        let d = *corner - plane.origin();
        let uv = inverse * Vector2::new(d.dot(u), d.dot(v));
        ur = (ur.0.min(uv.x), ur.1.max(uv.x));
        vr = (vr.0.min(uv.y), vr.1.max(uv.y));
    }
    (ur, vr)
}

fn rolled_surface(surface: &Surface, roll: &Roll, corners: &[Point3]) -> Result<Surface> {
    let (ur, vr) = match (surface, surface.parameter_range()) {
        (Surface::Plane(plane), _) => plane_range(plane, corners),
        (_, (u, v)) => (
            finite(u).ok_or_else(|| anyhow!("cannot wrap an unbounded surface"))?,
            finite(v).ok_or_else(|| anyhow!("cannot wrap an unbounded surface"))?,
        ),
    };
    let params = uniform(SURFACE_SAMPLES);
    let knots = knots(&params, 3);
    let rows: Vec<Vec<Point3>> = params
        .iter()
        .map(|&pv| {
            let points: Vec<Point3> = params
                .iter()
                .map(|&pu| {
                    roll.apply(surface.subs(ur.0 + (ur.1 - ur.0) * pu, vr.0 + (vr.1 - vr.0) * pv))
                })
                .collect();
            interpolate(&points, &params, &knots)
        })
        .collect::<Result<_>>()?;
    let columns: Vec<Vec<Point3>> = (0..rows[0].len())
        .map(|i| {
            let column: Vec<Point3> = rows.iter().map(|row| row[i]).collect();
            interpolate(&column, &params, &knots)
        })
        .collect::<Result<_>>()?;
    Ok(Surface::BsplineSurface(BsplineSurface::new(
        (knots.clone(), knots),
        columns,
    )))
}

impl Model {
    pub(crate) fn op_wrap(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["faces"], &["depth"], true)?;
        let raise = match args.rest.as_slice() {
            [] => false,
            ["raise"] => true,
            _ => bail!(
                "`wrap` takes `raise` to stand the sketch up from the face instead of cutting it in"
            ),
        };
        let depth = args.number("depth", &self.scope)?;
        if depth <= 0.0 {
            bail!("depth must be above zero, got {depth}");
        }
        let solid = self.active("wrap")?.clone();
        let selector = args.text("faces")?;
        let faces = select::select_faces(selector, &solid, &self.groups, self.tolerance())?;
        let cylinder = match super::mate::cylinders(&solid, &faces).as_slice() {
            [one] => *one,
            [] => bail!("`{selector}` has no round face to wrap onto"),
            many => bail!(
                "`{selector}` has {} round faces on different axes; pick one",
                many.len()
            ),
        };
        let (frame, profiles) = self.take_sketch("wrap")?;
        if frame.normal.dot(cylinder.axis).abs() > 1.0e-6 {
            bail!(
                "draw the sketch on a plane along the cylinder's axis, like XZ for a cylinder along z"
            );
        }
        let out = (frame.normal - cylinder.axis * frame.normal.dot(cylinder.axis)).normalize();
        let clearance = (depth * 0.1).max(self.tolerance() * 100.0);
        let (inner, outer) = if raise {
            (cylinder.radius - clearance, cylinder.radius + depth)
        } else {
            (cylinder.radius - depth, cylinder.radius + clearance)
        };
        let roll = Roll {
            centre: cylinder.point,
            axis: cylinder.axis,
            out,
            along: cylinder.axis.cross(out),
            origin: frame.origin,
            normal: frame.normal,
            radius: cylinder.radius,
            outer,
        };
        let shapes = loops(&frame, &profiles)?;
        let reach = cylinder.radius * std::f64::consts::PI;
        let combine = if raise { Combine::Add } else { Combine::Remove };
        let label = label_of(line);
        let mut tools = Vec::new();
        for region in regions(&shapes) {
            let flat = prism(
                &frame,
                &shapes,
                &region,
                &profiles,
                frame.normal * (outer - inner),
                (0.0, 0.0),
            )?;
            let bounds = geometry::bounds(&flat);
            let span = (bounds.max() - bounds.min()).dot(roll.along).abs();
            if span >= 2.0 * reach {
                bail!(
                    "the sketch is {span:.3} wide, more than the {:.3} around the cylinder",
                    2.0 * reach
                );
            }
            let corners: Vec<Point3> = (0..8)
                .map(|i| {
                    let pick = |bit: usize, lo: f64, hi: f64| if i & bit == 0 { lo } else { hi };
                    Point3::new(
                        pick(1, bounds.min().x, bounds.max().x),
                        pick(2, bounds.min().y, bounds.max().y),
                        pick(4, bounds.min().z, bounds.max().z),
                    )
                })
                .collect();
            let failure = std::cell::RefCell::new(None);
            let rolled = flat.mapped(
                |p| roll.apply(*p),
                |c| {
                    rolled_curve(c, &roll).unwrap_or_else(|e| {
                        failure.borrow_mut().get_or_insert(e);
                        c.clone()
                    })
                },
                |s| {
                    rolled_surface(s, &roll, &corners).unwrap_or_else(|e| {
                        failure.borrow_mut().get_or_insert(e);
                        s.clone()
                    })
                },
            );
            if let Some(error) = failure.into_inner() {
                return Err(error);
            }
            tools.push(oriented(rolled));
        }
        let count = tools.len();
        self.merge_all(&label, tools, combine)?;
        let summary = self.describe_solid()?;
        Ok(format!(
            "{summary}; wrapped {count} shape(s) onto radius {:.3}",
            cylinder.radius
        ))
    }
}
