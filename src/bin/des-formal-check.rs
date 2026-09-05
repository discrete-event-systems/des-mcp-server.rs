use std::path::PathBuf;

use des_mcp_server::formal::{DEFAULT_MAX_STATES, HARD_MAX_STATES, check_json};

const USAGE: &str = "Usage: des-formal-check [--max-states N] MODEL.json [MODEL.json ...]\n\
\n\
Checks every explicitly declared state reachable from the initial state.\n\
Exit codes: 0 = all models pass, 1 = a model violates a check, 2 = usage/input error.";

fn main() {
    match run(std::env::args().skip(1)) {
        Ok(exit_code) => std::process::exit(exit_code),
        Err(error) => {
            eprintln!("des-formal-check: {error}\n\n{USAGE}");
            std::process::exit(2);
        }
    }
}

fn run(args: impl IntoIterator<Item = String>) -> Result<i32, String> {
    let mut args = args.into_iter();
    let mut max_states = DEFAULT_MAX_STATES;
    let mut files = Vec::new();

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(0);
            }
            "--max-states" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "--max-states requires a value".to_string())?;
                max_states = raw
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --max-states value {raw:?}"))?;
                if !(1..=HARD_MAX_STATES).contains(&max_states) {
                    return Err(format!(
                        "--max-states must be between 1 and {HARD_MAX_STATES}"
                    ));
                }
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown option {flag:?}"));
            }
            file => files.push(PathBuf::from(file)),
        }
    }

    if files.is_empty() {
        return Err("at least one model file is required".to_string());
    }

    let mut model_failed = false;
    let mut input_failed = false;
    for (index, path) in files.iter().enumerate() {
        if index > 0 {
            println!("\n---\n");
        }
        match std::fs::read_to_string(path) {
            Ok(raw) => match check_json(&raw, max_states) {
                Ok(report) => {
                    println!("{}", report.render_markdown());
                    model_failed |= !report.passed();
                }
                Err(error) => {
                    eprintln!("{}: {error}", path.display());
                    input_failed = true;
                }
            },
            Err(error) => {
                eprintln!("{}: cannot read model: {error}", path.display());
                input_failed = true;
            }
        }
    }

    Ok(if input_failed {
        2
    } else if model_failed {
        1
    } else {
        0
    })
}
