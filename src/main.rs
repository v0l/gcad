use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use linecad::model::{Run, run_in};
use linecad::{export, parse, render, select};

#[derive(Parser)]
#[command(about = "Line-per-operation parametric CAD")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run every line and report what each one did
    Check { file: String },
    /// List the faces and edges a selector matches after the whole file (or up to --line)
    Query {
        file: String,
        selector: String,
        #[arg(long)]
        line: Option<usize>,
    },
    /// Write the solid as .step or .stl
    Export { file: String, output: String },
    /// Draw iso, top, front and right views into one PNG
    Render { file: String, output: String },
    /// Open the file in a window that rebuilds whenever it is saved
    View {
        file: String,
        /// Highlight what this selector matches
        #[arg(long, default_value = "")]
        select: String,
        /// Show the model as it is after this line
        #[arg(long)]
        line: Option<usize>,
    },
}

fn load(file: &str, until: Option<usize>) -> Result<Run> {
    let source = std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?;
    let lines: Vec<_> = parse::parse_program(&source)?
        .into_iter()
        .filter(|line| until.is_none_or(|last| line.number <= last))
        .collect();
    let dir = std::path::Path::new(file)
        .parent()
        .map(std::path::Path::to_path_buf);
    Ok(run_in(dir, &lines))
}

fn finished(run: &Run) -> Result<&monstertruck::modeling::Solid> {
    if let Some((line, Err(error))) = run.steps.last() {
        bail!("line {}: {}: {error:#}", line.number, line.text);
    }
    run.model
        .solid
        .as_ref()
        .ok_or_else(|| anyhow!("the file builds no solid"))
}

fn check(file: &str) -> Result<()> {
    let run = load(file, None)?;
    let width = run
        .steps
        .iter()
        .map(|(line, _)| line.text.len())
        .max()
        .unwrap_or(0);
    for (line, result) in &run.steps {
        match result {
            Ok(summary) => println!("{:>4} {:<width$}  ok  {summary}", line.number, line.text),
            Err(error) => println!("{:>4} {:<width$}  ERROR  {error:#}", line.number, line.text),
        }
    }
    match run.steps.last() {
        Some((line, Err(_))) => bail!("stopped at line {}", line.number),
        _ => finished(&run).map(|_| ()),
    }
}

fn query(file: &str, selector: &str, until: Option<usize>) -> Result<()> {
    let run = load(file, until)?;
    let solid = finished(&run)?;
    let tolerance = run.model.tolerance();
    let faces = select::faces(solid);
    let edge_part = selector.contains('&') || selector.contains('|');
    if !edge_part {
        let indices = select::select_faces(selector, solid, &run.model.groups, tolerance)?;
        println!("{} face(s)", indices.len());
        for i in indices {
            let face = &faces[i];
            let kind = match face.oriented_surface() {
                monstertruck::modeling::Surface::Plane(plane) => {
                    let n = plane.normal();
                    format!("plane normal ({:.3}, {:.3}, {:.3})", n.x, n.y, n.z)
                }
                _ => "curved".to_string(),
            };
            println!("  face {i}: {kind}, {} edges", face.edge_iter().count());
        }
    }
    let edges = select::select_edges(selector, solid, &run.model.groups, tolerance)?;
    println!("{} edge(s)", edges.len());
    for edge in edges {
        let (a, b) = (edge.front().point(), edge.back().point());
        println!(
            "  ({:.3}, {:.3}, {:.3}) -> ({:.3}, {:.3}, {:.3})",
            a.x, a.y, a.z, b.x, b.y, b.z
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Check { file } => check(&file),
        Command::Query {
            file,
            selector,
            line,
        } => query(&file, &selector, line),
        Command::Export { file, output } => {
            let run = load(&file, None)?;
            finished(&run)?;
            export::export(&run.model.solids(), &output)
        }
        Command::Render { file, output } => {
            let run = load(&file, None)?;
            finished(&run)?;
            render::render(&run.model.solids(), &output)
        }
        Command::View { file, select, line } => {
            linecad::view::run(file.into(), select, line).map_err(|error| anyhow!("{error}"))
        }
    }
}
