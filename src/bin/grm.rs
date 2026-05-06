use std::fs::File;
use std::io::{self, BufReader, IsTerminal};

use grm_rs::CliSession;

fn should_enable_color() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("session") => {
            let stdout = io::stdout();
            let writer = stdout.lock();
            match (args.next().as_deref(), args.next()) {
                (Some("--script"), Some(path)) => {
                    let file = match File::open(&path) {
                        Ok(file) => file,
                        Err(err) => {
                            eprintln!("failed to open script '{path}': {err}");
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
                (None, None) => {
                    let stdin = io::stdin();
                    let reader = BufReader::new(stdin.lock());
                    let mut session =
                        CliSession::new_with_color(reader, writer, should_enable_color());
                    if let Err(err) = session.run().await {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }
                }
                _ => {
                    eprintln!("Usage: grm session [--script <path>]");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("Usage: grm session [--script <path>]");
            std::process::exit(1);
        }
    }
}
