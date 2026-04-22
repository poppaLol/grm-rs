use std::fs::File;
use std::io::{self, BufReader};

use grm_rs::CliSession;

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
                            eprintln!("failed to open script '{}': {}", path, err);
                            std::process::exit(1);
                        }
                    };
                    let reader = BufReader::new(file);
                    let mut session = CliSession::new(reader, writer);
                    if let Err(err) = session.run_script().await {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }
                }
                (None, None) => {
                    let stdin = io::stdin();
                    let reader = BufReader::new(stdin.lock());
                    let mut session = CliSession::new(reader, writer);
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
