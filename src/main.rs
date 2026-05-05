mod api;
mod config;
mod crypto;
mod mdns;

/// Shared result type — avoids pulling in `anyhow`.
pub(crate) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = parse_args()?;

    if matches!(args.action, Action::Init) {
        let path = args
            .config
            .map(std::path::PathBuf::from)
            .unwrap_or_else(config::default_config_path);
        config::write_example(&path)?;
        eprintln!("Example config written to: {}", path.display());
        eprintln!("Edit it with your api_url, client_id, and token, then re-run.");
        return Ok(());
    }

    let cfg = config::load(args.config.map(std::path::PathBuf::from))?;

    match args.action {
        Action::Schema => println!("{}", pretty_json(&api::fetch_schema(&cfg)?)),
        Action::Exec(json) => println!("{}", pretty_json(&api::execute(&cfg, &json)?)),
        Action::Init => unreachable!(),
    }

    Ok(())
}

// ── argument parsing ──────────────────────────────────────────────────────────────────
struct Args {
    config: Option<String>,
    action: Action,
}

enum Action {
    Schema,
    Exec(String),
    Init,
}

fn parse_args() -> Result<Args> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut config: Option<String> = None;
    let mut action: Option<Action> = None;
    let mut i = 0;

    while i < raw.len() {
        match raw[i].as_str() {
            "--config" | "-c" => {
                i += 1;
                if i >= raw.len() {
                    return Err("--config requires a path argument".into());
                }
                config = Some(raw[i].clone());
            }
            "--schema" | "-s" => action = Some(Action::Schema),
            "--exec" | "-e" => {
                i += 1;
                if i >= raw.len() {
                    return Err("--exec requires a JSON argument".into());
                }
                action = Some(Action::Exec(raw[i].clone()));
            }
            "--init" => action = Some(Action::Init),
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--version" | "-V" | "-v" => {
                println!("plainapp-cli {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            arg => {
                return Err(
                    format!("unknown argument: {arg}\nRun with --help for usage.").into(),
                )
            }
        }
        i += 1;
    }

    let action = action.ok_or(
        "no action specified; use --schema, --exec <JSON>, or --init\n\
         Run with --help for usage.",
    )?;
    Ok(Args { config, action })
}

fn print_help() {
    println!(
        "plainapp-cli {version}\n\
         Query and manage your Android phone via the PlainApp GraphQL API.\n\
         \n\
         USAGE:\n\
             plainapp-cli [OPTIONS] <ACTION>\n\
         \n\
         OPTIONS:\n\
             -c, --config <FILE>   Config file  [default: ~/.config/plainapp-cli/config.toml]\n\
             -h, --help            Print help\n\
             -V, --version         Print version\n\
         \n\
         ACTIONS:\n\
             -s, --schema              Fetch the full GraphQL schema (introspection)\n\
             -e, --exec <JSON>         Execute a GraphQL query / mutation\n\
                 --init                Write an example config file and exit\n\
         \n\
         EXAMPLES:\n\
             plainapp-cli --init\n\
             plainapp-cli --schema\n\
             plainapp-cli --exec '{{\"query\":\"{{ chatChannels {{ id name }} }}\",\"variables\":null}}'\n\
             plainapp-cli -c /path/to/config.toml -s",
        version = env!("CARGO_PKG_VERSION")
    );
}

// ── JSON pretty-printer ────────────────────────────────────────────────────────────────────

/// Indent a compact JSON string.  Falls back to raw input on non-JSON.
fn pretty_json(s: &str) -> String {
    let s = s.trim();
    if !s.starts_with('{') && !s.starts_with('[') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() * 2);
    let mut depth: usize = 0;
    let mut in_str = false;
    let mut escaped = false;

    for c in s.chars() {
        if escaped {
            out.push(c);
            escaped = false;
            continue;
        }
        if in_str {
            out.push(c);
            match c {
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            ' ' | '\t' | '\n' | '\r' => {}
            '"' => {
                in_str = true;
                out.push(c);
            }
            '{' | '[' => {
                out.push(c);
                depth += 1;
                out.push('\n');
                push_indent(&mut out, depth);
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                out.push('\n');
                push_indent(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(c);
                out.push('\n');
                push_indent(&mut out, depth);
            }
            ':' => out.push_str(": "),
            _ => out.push(c),
        }
    }
    out
}

fn push_indent(s: &mut String, depth: usize) {
    for _ in 0..depth {
        s.push_str("  ");
    }
}
