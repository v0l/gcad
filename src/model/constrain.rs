use super::Model;
use super::args::Args;
use crate::parse::{Line, eval, eval_point};
use anyhow::{Result, bail};

#[derive(Clone, Debug)]
pub struct SketchPoint {
    pub name: String,
    pub at: (f64, f64),
    pub fixed: bool,
}

#[derive(Clone, Debug)]
pub enum Constraint {
    Distance(usize, usize, f64),
    Horizontal(usize, usize),
    Vertical(usize, usize),
    Angle(usize, usize, f64),
    Coincident(usize, usize),
    Parallel([usize; 4]),
    Perpendicular([usize; 4]),
    Equal([usize; 4]),
    Midpoint(usize, usize, usize),
    Online(usize, usize, usize),
}

fn residuals(points: &[(f64, f64)], constraints: &[Constraint]) -> Vec<f64> {
    let span = |a: usize, b: usize| (points[b].0 - points[a].0, points[b].1 - points[a].1);
    let length = |(x, y): (f64, f64)| x.hypot(y).max(1.0e-12);
    constraints
        .iter()
        .flat_map(|constraint| match *constraint {
            Constraint::Distance(a, b, d) => vec![length(span(a, b)) - d],
            Constraint::Horizontal(a, b) => vec![points[b].1 - points[a].1],
            Constraint::Vertical(a, b) => vec![points[b].0 - points[a].0],
            Constraint::Angle(a, b, degrees) => {
                let (s, c) = degrees.to_radians().sin_cos();
                let (dx, dy) = span(a, b);
                vec![dy * c - dx * s]
            }
            Constraint::Coincident(a, b) => {
                let (dx, dy) = span(a, b);
                vec![dx, dy]
            }
            Constraint::Parallel([a, b, c, d]) | Constraint::Perpendicular([a, b, c, d]) => {
                let (u, v) = (span(a, b), span(c, d));
                let scale = length(u) * length(v);
                let value = match constraint {
                    Constraint::Parallel(_) => u.0 * v.1 - u.1 * v.0,
                    _ => u.0 * v.0 + u.1 * v.1,
                };
                vec![value / scale * length(u).max(length(v))]
            }
            Constraint::Equal([a, b, c, d]) => vec![length(span(a, b)) - length(span(c, d))],
            Constraint::Midpoint(m, a, b) => vec![
                points[m].0 - (points[a].0 + points[b].0) / 2.0,
                points[m].1 - (points[a].1 + points[b].1) / 2.0,
            ],
            Constraint::Online(p, a, b) => {
                let (u, w) = (span(a, b), span(a, p));
                vec![(u.0 * w.1 - u.1 * w.0) / length(u)]
            }
        })
        .collect()
}

fn solve_linear(mut m: Vec<Vec<f64>>, mut rhs: Vec<f64>) -> Option<Vec<f64>> {
    let n = rhs.len();
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| m[i][col].abs().total_cmp(&m[j][col].abs()))?;
        if m[pivot][col].abs() < 1.0e-300 {
            return None;
        }
        m.swap(col, pivot);
        rhs.swap(col, pivot);
        let pivot_row = m[col].clone();
        for row in col + 1..n {
            let factor = m[row][col] / pivot_row[col];
            m[row][col..]
                .iter_mut()
                .zip(&pivot_row[col..])
                .for_each(|(target, source)| *target -= factor * source);
            rhs[row] -= factor * rhs[col];
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let tail: f64 = (row + 1..n).map(|k| m[row][k] * x[k]).sum();
        x[row] = (rhs[row] - tail) / m[row][row];
    }
    Some(x)
}

fn rank(rows: &[Vec<f64>], tolerance: f64) -> usize {
    let mut m = rows.to_vec();
    let columns = m.first().map_or(0, Vec::len);
    let mut rank = 0;
    for col in 0..columns {
        let Some(pivot) = (rank..m.len())
            .filter(|&i| m[i][col].abs() > tolerance)
            .max_by(|&i, &j| m[i][col].abs().total_cmp(&m[j][col].abs()))
        else {
            continue;
        };
        m.swap(rank, pivot);
        let pivot_row = m[rank].clone();
        for row in m.iter_mut().skip(rank + 1) {
            let factor = row[col] / pivot_row[col];
            row[col..]
                .iter_mut()
                .zip(&pivot_row[col..])
                .for_each(|(target, source)| *target -= factor * source);
        }
        rank += 1;
    }
    rank
}

pub fn solve(
    points: &[SketchPoint],
    constraints: &[Constraint],
) -> Result<(Vec<(f64, f64)>, usize)> {
    let free: Vec<usize> = (0..points.len()).filter(|&i| !points[i].fixed).collect();
    let unpack = |x: &[f64]| -> Vec<(f64, f64)> {
        let mut at: Vec<(f64, f64)> = points.iter().map(|p| p.at).collect();
        free.iter()
            .enumerate()
            .for_each(|(k, &i)| at[i] = (x[2 * k], x[2 * k + 1]));
        at
    };
    let scale = points
        .iter()
        .map(|p| p.at.0.abs().max(p.at.1.abs()))
        .chain(constraints.iter().filter_map(|c| match c {
            Constraint::Distance(_, _, d) => Some(d.abs()),
            _ => None,
        }))
        .fold(1.0, f64::max);
    let cost = |x: &[f64]| {
        residuals(&unpack(x), constraints)
            .iter()
            .map(|r| r * r)
            .sum::<f64>()
    };
    let jacobian = |x: &[f64]| -> Vec<Vec<f64>> {
        let h = scale * 1.0e-7;
        let columns: Vec<Vec<f64>> = (0..x.len())
            .map(|j| {
                let (mut up, mut down) = (x.to_vec(), x.to_vec());
                up[j] += h;
                down[j] -= h;
                let (ru, rd) = (
                    residuals(&unpack(&up), constraints),
                    residuals(&unpack(&down), constraints),
                );
                ru.iter()
                    .zip(&rd)
                    .map(|(u, d)| (u - d) / (2.0 * h))
                    .collect()
            })
            .collect();
        let rows = columns.first().map_or(0, Vec::len);
        (0..rows)
            .map(|i| columns.iter().map(|c| c[i]).collect())
            .collect()
    };
    let mut x: Vec<f64> = free
        .iter()
        .flat_map(|&i| [points[i].at.0, points[i].at.1])
        .collect();
    let mut lambda = 1.0e-3;
    for _ in 0..500 {
        let current = cost(&x);
        if current < (scale * 1.0e-13).powi(2) || x.is_empty() {
            break;
        }
        let j = jacobian(&x);
        let r = residuals(&unpack(&x), constraints);
        let n = x.len();
        let jtj: Vec<Vec<f64>> = (0..n)
            .map(|a| {
                (0..n)
                    .map(|b| j.iter().map(|row| row[a] * row[b]).sum::<f64>())
                    .collect()
            })
            .collect();
        let jtr: Vec<f64> = (0..n)
            .map(|a| -j.iter().zip(&r).map(|(row, ri)| row[a] * ri).sum::<f64>())
            .collect();
        let mut improved = false;
        for _ in 0..30 {
            let mut damped = jtj.clone();
            (0..n).for_each(|a| damped[a][a] += lambda * (1.0 + jtj[a][a]));
            if let Some(step) = solve_linear(damped, jtr.clone()) {
                let trial: Vec<f64> = x.iter().zip(&step).map(|(a, b)| a + b).collect();
                if cost(&trial) < current {
                    x = trial;
                    lambda = (lambda / 3.0).max(1.0e-12);
                    improved = true;
                    break;
                }
            }
            lambda *= 4.0;
        }
        if !improved {
            break;
        }
    }
    let worst = residuals(&unpack(&x), constraints)
        .iter()
        .fold(0.0_f64, |m, r| m.max(r.abs()));
    if worst > scale * 1.0e-9 {
        bail!(
            "the constraints cannot all be met (off by {worst:.3e}); one of them conflicts with the others"
        );
    }
    let freedom = x.len()
        - if x.is_empty() || constraints.is_empty() {
            0
        } else {
            rank(&jacobian(&x), 1.0e-6)
        };
    Ok((unpack(&x), freedom))
}

impl Model {
    fn point_index(&self, name: &str) -> Result<usize> {
        self.points
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(|| anyhow::anyhow!("no point `{name}`; declare it with `point {name}`"))
    }

    fn resolve(
        &mut self,
        points: Vec<SketchPoint>,
        constraints: Vec<Constraint>,
    ) -> Result<String> {
        let (solved, freedom) = solve(&points, &constraints)?;
        self.points = points;
        self.constraints = constraints;
        for (point, at) in self.points.iter_mut().zip(&solved) {
            point.at = *at;
            self.scope.insert(format!("{}.x", point.name), at.0);
            self.scope.insert(format!("{}.y", point.name), at.1);
        }
        let place = |p: &SketchPoint| format!("{} {:.3},{:.3}", p.name, p.at.0, p.at.1);
        Ok(format!(
            "{}; {freedom} degree(s) of freedom left",
            self.points.iter().map(place).collect::<Vec<_>>().join(", ")
        ))
    }

    pub(crate) fn op_point(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["name"], &["near"], true)?;
        let name = args.text("name")?.to_string();
        if !name.starts_with(|c: char| c.is_ascii_alphabetic()) || name.contains(['.', ',']) {
            bail!("`{name}` is not a point name; start it with a letter");
        }
        if self.points.iter().any(|p| p.name == name) {
            bail!("point `{name}` already exists");
        }
        let (at, fixed) = match (
            args.rest.as_slice(),
            args.optional_point("near", &self.scope)?,
        ) {
            ([], Some(near)) => (near, false),
            ([], None) => {
                let k = self.points.len() as f64 + 1.0;
                (
                    (
                        (k * 2.39996).cos() * 10.0 * k,
                        (k * 2.39996).sin() * 10.0 * k,
                    ),
                    false,
                )
            }
            ([at], None) => (eval_point(at, &self.scope)?, true),
            _ => bail!(
                "write `point name x,y` for a fixed point or `point name [near=x,y]` for a free one"
            ),
        };
        let mut points = self.points.clone();
        points.push(SketchPoint { name, at, fixed });
        self.resolve(points, self.constraints.clone())
    }

    pub(crate) fn op_constrain(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &[], true)?;
        let words = args.rest.clone();
        let count = match line.op.as_str() {
            "dist" | "angle" | "horizontal" | "vertical" | "coincident" => 2,
            "midpoint" | "online" => 3,
            _ => 4,
        };
        if words.len() < count {
            bail!("`{}` needs {count} points", line.op);
        }
        let ids = words[..count]
            .iter()
            .map(|name| self.point_index(name))
            .collect::<Result<Vec<_>>>()?;
        let extra = &words[count..];
        let value = |what: &str| -> Result<f64> {
            match extra {
                [text] => eval(text, &self.scope),
                _ => bail!("write `{} {} <{what}>`", line.op, words[..count].join(" ")),
            }
        };
        if !matches!(line.op.as_str(), "dist" | "angle") && !extra.is_empty() {
            bail!("`{}` takes {count} points and nothing else", line.op);
        }
        if ids[0] == ids[1] {
            bail!("a constraint needs two different points");
        }
        let constraint = match line.op.as_str() {
            "dist" => Constraint::Distance(ids[0], ids[1], value("distance")?),
            "angle" => Constraint::Angle(ids[0], ids[1], value("degrees")?),
            "horizontal" => Constraint::Horizontal(ids[0], ids[1]),
            "vertical" => Constraint::Vertical(ids[0], ids[1]),
            "coincident" => Constraint::Coincident(ids[0], ids[1]),
            "midpoint" => Constraint::Midpoint(ids[0], ids[1], ids[2]),
            "online" => Constraint::Online(ids[0], ids[1], ids[2]),
            "parallel" => Constraint::Parallel([ids[0], ids[1], ids[2], ids[3]]),
            "perpendicular" => Constraint::Perpendicular([ids[0], ids[1], ids[2], ids[3]]),
            _ => Constraint::Equal([ids[0], ids[1], ids[2], ids[3]]),
        };
        let mut constraints = self.constraints.clone();
        constraints.push(constraint);
        self.resolve(self.points.clone(), constraints)
    }
}
