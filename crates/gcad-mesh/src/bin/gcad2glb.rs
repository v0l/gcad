use std::process::ExitCode;

fn main() -> ExitCode {
    let mut input = None;
    let mut output = None;
    let mut options = gcad_mesh::Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--detail" => match args.next().and_then(|v| v.parse().ok()) {
                Some(v) => options.detail = v,
                None => return usage("--detail takes a number"),
            },
            "--z-up" => options.y_up = false,
            "--tris" => match args.next().and_then(|v| v.parse().ok()) {
                Some(v) if v >= 4 => options.triangles = Some(v),
                _ => return usage("--tris takes a whole number of triangles, at least 4"),
            },
            "--max-error" => match args.next().and_then(|v| v.parse::<f64>().ok()) {
                Some(v) if v > 0.0 => options.max_error = Some(v),
                _ => return usage("--max-error takes a distance above 0 in model units"),
            },
            "-h" | "--help" => return usage(""),
            _ if input.is_none() => input = Some(arg),
            _ if output.is_none() => output = Some(arg),
            _ => return usage("too many arguments"),
        }
    }
    let (Some(input), Some(output)) = (input, output) else {
        return usage("needs an input and an output");
    };
    let source = match std::fs::read_to_string(&input) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let glb = gcad_mesh::build_with(&source, &options)
        .map_err(|e| e.to_string())
        .and_then(|model| model.to_glb().map_err(|e| format!("{e:#}")));
    match glb {
        Ok(bytes) => match std::fs::write(&output, bytes) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{output}: {e}");
                ExitCode::FAILURE
            }
        },
        Err(message) => {
            eprintln!("{input}: {message}");
            ExitCode::FAILURE
        }
    }
}

fn usage(problem: &str) -> ExitCode {
    if !problem.is_empty() {
        eprintln!("{problem}");
    }
    eprintln!("usage: gcad2glb [--detail N] [--z-up] [--tris N] [--max-error E] in.gcad out.glb");
    if problem.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
