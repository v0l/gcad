use anyhow::{Result, anyhow, bail};
use clap::{Parser, Subcommand};
use gcad::model::{Run, run_path};
use gcad::{export, render, select};

#[derive(Parser)]
#[command(
    about = "Parametric CAD written like G-code: one operation per line",
    version,
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
    Check {
        file: String,
        /// Show how long each line took
        #[arg(long)]
        time: bool,
    },
    /// List the faces and edges a selector matches after the whole file (or up to --line)
    Query {
        file: String,
        selector: String,
        #[arg(long)]
        line: Option<usize>,
    },
    /// Write the solid as .step, .stl, .obj, .3mf or an .svg drawing
    Export {
        file: String,
        output: String,
        /// For an .svg drawing, add a section view cut across this plane, like y=0
        #[arg(long)]
        section: Option<gcad::drawing::Section>,
    },
    /// List the parts an assembly is made of, counted by file, body and variables
    Bom {
        file: String,
        /// Write CSV instead of a table
        #[arg(long)]
        csv: bool,
    },
    /// Draw iso, top, front and right views into one PNG
    Render {
        file: String,
        output: String,
        /// Draw an assembly with its parts moved apart as its `explode` lines say
        #[arg(long)]
        explode: bool,
    },
    /// Print the format reference: every operation, selector and assembly line
    Docs,
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
    run_path(std::path::Path::new(file), vars, until)
}

fn finished(run: &Run) -> Result<()> {
    if let Some((line, Err(error))) = run.steps.last() {
        bail!("line {}: {}: {error:#}", line.number, line.text);
    }
    if run.model.solids().is_empty() {
        bail!("the file builds no solid");
    }
    Ok(())
}

fn check(file: &str, vars: &[(String, f64)], time: bool) -> Result<()> {
    let run = load(file, None, vars)?;
    let width = run
        .steps
        .iter()
        .map(|(line, _)| line.text.len())
        .max()
        .unwrap_or(0);
    for (k, (line, result)) in run.steps.iter().enumerate() {
        let spent = match (time, run.took.get(k)) {
            (true, Some(d)) => format!("{:>7.0} ms ", d.as_secs_f64() * 1000.0),
            _ => String::new(),
        };
        match result {
            Ok(summary) => println!(
                "{:>4} {:<width$}  {spent}ok  {summary}",
                line.number, line.text
            ),
            Err(error) => {
                println!(
                    "{:>4} {:<width$}  {spent}ERROR  {error:#}",
                    line.number, line.text
                )
            }
        }
    }
    if time {
        let total: std::time::Duration = run.took.iter().sum();
        println!("{:.2} s in all", total.as_secs_f64());
    }
    let bodies = run.model.body_names();
    if !run.model.assembly && bodies.len() > 1 {
        println!(
            "note: {} separate bodies ({}). If they are separate parts, bring this file into a .gasm with `part` to place, join and check them; if they are one part, `combine` them",
            bodies.len(),
            bodies.join(", ")
        );
    }
    match run.steps.last() {
        Some((line, Err(_))) => bail!("stopped at line {}", line.number),
        _ => finished(&run),
    }
}

fn query(file: &str, selector: &str, until: Option<usize>, vars: &[(String, f64)]) -> Result<()> {
    let run = load(file, until, vars)?;
    finished(&run)?;
    let solid = run
        .model
        .solid
        .as_ref()
        .ok_or_else(|| anyhow!("`query` looks at a part file's current body"))?;
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
        Command::Check { file, time } => check(&file, &vars, time),
        Command::Docs => {
            print!("{}", include_str!("../docs/format.md"));
            Ok(())
        }
        Command::Query {
            file,
            selector,
            line,
        } => query(&file, &selector, line, &vars),
        Command::Export {
            file,
            output,
            section,
        } => {
            let run = load(&file, None, &vars)?;
            finished(&run)?;
            if let Some(section) = section {
                if !output.to_ascii_lowercase().ends_with(".svg") {
                    bail!("--section only applies to an .svg drawing");
                }
                let parts = run.model.parts();
                std::fs::write(
                    &output,
                    gcad::drawing::drawing_with(&parts, Some(section)),
                )?;
                return Ok(());
            }
            export::export_model(&run.model, std::path::Path::new(&file), &output)
        }
        Command::Bom { file, csv } => {
            let run = load(&file, None, &vars)?;
            finished(&run)?;
            let items = gcad::bom::items(&run.model, std::path::Path::new(&file));
            if csv {
                print!("{}", gcad::bom::csv(&items));
            } else {
                println!("{}", gcad::bom::table(&items));
            }
            Ok(())
        }
        Command::Render {
            file,
            output,
            explode,
        } => {
            let run = load(&file, None, &vars)?;
            finished(&run)?;
            let parts = run.model.exploded_parts(if explode { 1.0 } else { 0.0 });
            let parts: Vec<_> = parts.iter().map(|(s, c)| (s, *c)).collect();
            render::render_coloured(&parts, &output)
        }
        Command::View { file, select, line } => {
            gcad::view::run(file.map(Into::into), select, line, vars)
                .map_err(|error| anyhow!("{error}"))
        }
    }
}
