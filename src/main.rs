use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use linecad::model::{Run, run_full};
use linecad::{export, parse, render, select};

#[derive(Parser)]
#[command(
    about = "Line-per-operation parametric CAD",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Open this file in the viewer (the default when no command is given)
    file: Option<String>,
    /// Set a variable, overriding the file's `let`, as name=value (repeatable)
    #[arg(long = "set", global = true, value_parser = parse_set)]
    set: Vec<(String, f64)>,
}

fn parse_set(text: &str) -> std::result::Result<(String, f64), String> {
    let (name, value) = text.split_once('=').ok_or("write it as name=value")?;
    let value = value
        .parse::<f64>()
        .map_err(|_| format!("`{value}` is not a number"))?;
    Ok((name.to_string(), value))
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
    /// Write the solid as .step, .stl, .obj, .3mf or an .svg drawing
    Export { file: String, output: String },
    /// Draw iso, top, front and right views into one PNG
    Render { file: String, output: String },
    /// Open the file in a window that rebuilds whenever it is saved
    View {
        file: Option<String>,
        /// Highlight what this selector matches
        #[arg(long, default_value = "")]
        select: String,
        /// Show the model as it is after this line
        #[arg(long)]
        line: Option<usize>,
    },
}

fn load(file: &str, until: Option<usize>, vars: &[(String, f64)]) -> Result<Run> {
    let source = std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?;
    let lines: Vec<_> = parse::parse_program(&source)?
        .into_iter()
        .filter(|line| until.is_none_or(|last| line.number <= last))
        .collect();
    let dir = std::path::Path::new(file)
        .parent()
        .map(std::path::Path::to_path_buf);
    Ok(run_full(dir, vars, &lines))
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

fn check(file: &str, vars: &[(String, f64)]) -> Result<()> {
    let run = load(file, None, vars)?;
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

fn query(file: &str, selector: &str, until: Option<usize>, vars: &[(String, f64)]) -> Result<()> {
    let run = load(file, until, vars)?;
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
    let cli = Cli::parse();
    let vars = cli.set;
    let command = cli.command.unwrap_or(Command::View {
        file: cli.file,
        select: String::new(),
        line: None,
    });
    match command {
        Command::Check { file } => check(&file, &vars),
        Command::Query {
            file,
            selector,
            line,
        } => query(&file, &selector, line, &vars),
        Command::Export { file, output } => {
            let run = load(&file, None, &vars)?;
            finished(&run)?;
            export::export_coloured(&run.model.parts(), &output)
        }
        Command::Render { file, output } => {
            let run = load(&file, None, &vars)?;
            finished(&run)?;
            render::render_coloured(&run.model.parts(), &output)
        }
        Command::View { file, select, line } => {
            linecad::view::run(file.map(Into::into), select, line, vars)
                .map_err(|error| anyhow!("{error}"))
        }
    }
}
