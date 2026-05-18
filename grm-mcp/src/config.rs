use std::path::PathBuf;

use grm_rs::{DurabilityFormat, GrmError, Result as GrmResult};

#[derive(Debug, Clone, Default)]
pub struct StartupOptions {
    pub load_json: Option<PathBuf>,
    pub load_bin: Option<PathBuf>,
    pub import_json: Option<PathBuf>,
    pub autocommit: Option<AutocommitTarget>,
    pub export_json: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct AutocommitTarget {
    pub format: SessionFileFormat,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFileFormat {
    Json,
    Binary,
}

impl SessionFileFormat {
    pub fn durability_format(self) -> DurabilityFormat {
        match self {
            Self::Json => DurabilityFormat::Json,
            Self::Binary => DurabilityFormat::Binary,
        }
    }
}

pub fn parse_startup_options<I>(args: I) -> GrmResult<StartupOptions>
where
    I: IntoIterator<Item = String>,
{
    let mut options = StartupOptions::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--load-json" => options.load_json = Some(next_path(&mut args, &arg)?),
            "--load-bin" => options.load_bin = Some(next_path(&mut args, &arg)?),
            "--import-json" => options.import_json = Some(next_path(&mut args, &arg)?),
            "--export-json" => options.export_json = Some(next_path(&mut args, &arg)?),
            "--autocommit-json" => {
                options.autocommit = Some(AutocommitTarget {
                    format: SessionFileFormat::Json,
                    path: next_path(&mut args, &arg)?,
                });
            }
            "--autocommit-bin" => {
                options.autocommit = Some(AutocommitTarget {
                    format: SessionFileFormat::Binary,
                    path: next_path(&mut args, &arg)?,
                });
            }
            "--help" | "-h" => {
                return Err(GrmError::Constraint(usage().to_string()));
            }
            _ => {
                return Err(GrmError::Constraint(format!(
                    "unknown argument '{arg}'\n{}",
                    usage()
                )));
            }
        }
    }
    Ok(options)
}

pub fn usage() -> &'static str {
    "Usage: grm-mcp [--load-json <path>] [--load-bin <path>] [--import-json <path>] [--export-json <path>] [--autocommit-json <path>] [--autocommit-bin <path>]"
}

fn next_path(args: &mut impl Iterator<Item = String>, flag: &str) -> GrmResult<PathBuf> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| GrmError::Constraint(format!("{flag} requires a path")))
}
