use std::fs::File;
use std::io::{self, BufReader, IsTerminal};
use std::path::PathBuf;

use grm_rs::CliSession;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupLoadFormat {
    Json,
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupAutocommit {
    Default,
    On,
    Off,
}

#[derive(Debug, PartialEq, Eq)]
enum SessionStartup {
    Fresh,
    Script {
        path: PathBuf,
    },
    Load {
        format: StartupLoadFormat,
        path: PathBuf,
        autocommit: StartupAutocommit,
    },
}

fn should_enable_color() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("session") => {
            let startup = match parse_session_startup(args.collect()) {
                Ok(startup) => startup,
                Err(message) => {
                    eprintln!("{message}");
                    eprintln!("{}", session_usage());
                    std::process::exit(1);
                }
            };
            let stdout = io::stdout();
            let writer = stdout.lock();
            match startup {
                SessionStartup::Script { path } => {
                    let file = match File::open(&path) {
                        Ok(file) => file,
                        Err(err) => {
                            eprintln!("failed to open script '{}': {err}", path.display());
                            std::process::exit(1);
                        }
                    };
                    let reader = BufReader::new(file);
                    let mut session =
                        CliSession::new_with_color(reader, writer, should_enable_color());
                    if let Err(err) = session.run_script().await {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }

                    let (state, _, writer) = session.into_parts();
                    let stdin = io::stdin();
                    let reader = BufReader::new(stdin.lock());
                    let mut session = CliSession::with_state_and_color(
                        state,
                        reader,
                        writer,
                        should_enable_color(),
                    );
                    if let Err(err) = session.continue_interactive().await {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }
                }
                SessionStartup::Load {
                    format,
                    path,
                    autocommit,
                } => {
                    let stdin = io::stdin();
                    let reader = BufReader::new(stdin.lock());
                    let mut session =
                        CliSession::new_with_color(reader, writer, should_enable_color());

                    let load_result = match format {
                        StartupLoadFormat::Json => session.load_session_json(&path),
                        StartupLoadFormat::Binary => session.load_session_binary(&path),
                    };
                    if let Err(err) = load_result {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }

                    let autocommit_result = match (format, autocommit) {
                        (_, StartupAutocommit::Default | StartupAutocommit::Off) => {
                            session.write_startup_autocommit_off()
                        }
                        (StartupLoadFormat::Json, StartupAutocommit::On) => session
                            .enable_autocommit_json(&path)
                            .and_then(|_| session.write_startup_autocommit_on(&path)),
                        (StartupLoadFormat::Binary, StartupAutocommit::On) => session
                            .enable_autocommit_binary(&path)
                            .and_then(|_| session.write_startup_autocommit_on(&path)),
                    };
                    if let Err(err) = autocommit_result {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }

                    if let Err(err) = session.continue_loaded_interactive().await {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }
                }
                SessionStartup::Fresh => {
                    let stdin = io::stdin();
                    let reader = BufReader::new(stdin.lock());
                    let mut session =
                        CliSession::new_with_color(reader, writer, should_enable_color());
                    if let Err(err) = session.run().await {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }
                }
            }
        }
        _ => {
            eprintln!("{}", session_usage());
            std::process::exit(1);
        }
    }
}

fn parse_session_startup(args: Vec<String>) -> Result<SessionStartup, String> {
    if args.is_empty() {
        return Ok(SessionStartup::Fresh);
    }

    let mut script = None;
    let mut load = None;
    let mut autocommit = StartupAutocommit::Default;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--script" => {
                let Some(path) = args.get(index + 1) else {
                    return Err("--script requires <path>".to_string());
                };
                script = Some(PathBuf::from(path));
                index += 2;
            }
            "--load" => {
                let Some(format) = args.get(index + 1) else {
                    return Err("--load requires json|bin and <path>".to_string());
                };
                let Some(path) = args.get(index + 2) else {
                    return Err("--load requires json|bin and <path>".to_string());
                };
                let format = match format.as_str() {
                    "json" => StartupLoadFormat::Json,
                    "bin" => StartupLoadFormat::Binary,
                    other => {
                        return Err(format!("unknown --load format '{other}'"));
                    }
                };
                load = Some((format, PathBuf::from(path)));
                index += 3;
            }
            "--autocommit" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("--autocommit requires on|off".to_string());
                };
                autocommit = match value.as_str() {
                    "on" => StartupAutocommit::On,
                    "off" => StartupAutocommit::Off,
                    other => {
                        return Err(format!("unknown --autocommit value '{other}'"));
                    }
                };
                index += 2;
            }
            other => {
                return Err(format!("unknown session option '{other}'"));
            }
        }
    }

    if script.is_some() && load.is_some() {
        return Err("--script and --load cannot be combined yet".to_string());
    }

    if let Some(path) = script {
        if autocommit != StartupAutocommit::Default {
            return Err("--autocommit requires --load".to_string());
        }
        return Ok(SessionStartup::Script { path });
    }

    if let Some((format, path)) = load {
        return Ok(SessionStartup::Load {
            format,
            path,
            autocommit,
        });
    }

    if autocommit != StartupAutocommit::Default {
        return Err("--autocommit requires --load".to_string());
    }

    Ok(SessionStartup::Fresh)
}

fn session_usage() -> &'static str {
    "Usage: grm session [--script <path> | --load json|bin <path> [--autocommit on|off]]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fresh_session() {
        assert_eq!(
            parse_session_startup(vec![]).unwrap(),
            SessionStartup::Fresh
        );
    }

    #[test]
    fn parses_load_json_with_autocommit_on() {
        assert_eq!(
            parse_session_startup(vec![
                "--load".to_string(),
                "json".to_string(),
                "session.json".to_string(),
                "--autocommit".to_string(),
                "on".to_string(),
            ])
            .unwrap(),
            SessionStartup::Load {
                format: StartupLoadFormat::Json,
                path: PathBuf::from("session.json"),
                autocommit: StartupAutocommit::On,
            }
        );
    }

    #[test]
    fn rejects_autocommit_without_load() {
        assert!(
            parse_session_startup(vec!["--autocommit".to_string(), "on".to_string(),])
                .unwrap_err()
                .contains("--load")
        );
    }
}
