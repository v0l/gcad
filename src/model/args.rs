use crate::parse::{Scope, eval};
use anyhow::{Result, bail};
use monstertruck::modeling::{EuclideanSpace, Point3, Vector3};

pub use gcad_mesh::args::{Args, label_of, positive};

pub(crate) fn point3(text: &str, scope: &Scope) -> Result<Point3> {
    match crate::parse::split_top_level(text, ',').as_slice() {
        [x, y, z] => Ok(Point3::new(
            eval(x, scope)?,
            eval(y, scope)?,
            eval(z, scope)?,
        )),
        _ => bail!("`{text}` is not a 3D point, write it as x,y,z"),
    }
}

pub(crate) fn direction(text: &str, scope: &Scope) -> Result<Vector3> {
    let (sign, name) = match text.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, text),
    };
    match name.to_ascii_lowercase().as_str() {
        "x" | "y" | "z" => Ok(axis(name)? * sign),
        _ => point3(text, scope).map(|p| p.to_vec()),
    }
}

pub(crate) fn axis(text: &str) -> Result<Vector3> {
    match text.to_ascii_lowercase().as_str() {
        "x" => Ok(Vector3::unit_x()),
        "y" => Ok(Vector3::unit_y()),
        "z" => Ok(Vector3::unit_z()),
        other => bail!("axis must be x, y, z or a datum `axis`, got `{other}`"),
    }
}

pub(crate) fn describe_point(point: Point3) -> String {
    format!("{:.3},{:.3},{:.3}", point.x, point.y, point.z)
}

pub(crate) fn combine_mode(args: &Args<'_>) -> Result<super::Combine> {
    match args.values.get("mode").copied() {
        None | Some("add") => Ok(super::Combine::Add),
        Some("cut") => Ok(super::Combine::Remove),
        Some("intersect") => Ok(super::Combine::Common),
        Some(other) => bail!("mode must be add, cut or intersect, got `{other}`"),
    }
}
