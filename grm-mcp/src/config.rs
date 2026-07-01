use std::net::SocketAddr;
use std::path::PathBuf;

use grm_rs::{DurabilityFormat, GrmError, Result as GrmResult};

#[derive(Debug, Clone, Default)]
pub struct StartupOptions {
    pub transport: TransportOptions,
    pub load_json: Option<PathBuf>,
    pub load_bin: Option<PathBuf>,
    pub import_json: Option<PathBuf>,
    pub autocommit: Option<AutocommitTarget>,
    pub export_json: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum TransportOptions {
    Stdio,
    Http {
        bind: SocketAddr,
        path: String,
        allowed_hosts: Vec<String>,
    },
}

impl Default for TransportOptions {
    fn default() -> Self {
        Self::Stdio
    }
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
            "--transport" => {
                let value = next_value(&mut args, &arg)?;
                options.transport = match value.as_str() {
                    "stdio" => TransportOptions::Stdio,
                    "http" | "streamable-http" => match options.transport {
                        TransportOptions::Http {
                            bind,
                            ref path,
                            ref allowed_hosts,
                        } => TransportOptions::Http {
                            bind,
                            path: path.clone(),
                            allowed_hosts: allowed_hosts.clone(),
                        },
                        TransportOptions::Stdio => TransportOptions::Http {
                            bind: "127.0.0.1:8080".parse().expect("valid default HTTP bind"),
                            path: "/mcp".into(),
                            allowed_hosts: default_allowed_hosts(),
                        },
                    },
                    other => {
                        return Err(GrmError::Constraint(format!(
                            "unsupported --transport '{other}'; expected 'stdio' or 'http'"
                        )));
                    }
                };
            }
            "--http-bind" => {
                let bind = next_value(&mut args, &arg)?;
                let bind = bind.parse::<SocketAddr>().map_err(|err| {
                    GrmError::Constraint(format!("--http-bind requires host:port: {err}"))
                })?;
                let (path, allowed_hosts) = match options.transport {
                    TransportOptions::Http {
                        ref path,
                        ref allowed_hosts,
                        ..
                    } => (path.clone(), allowed_hosts.clone()),
                    TransportOptions::Stdio => ("/mcp".into(), default_allowed_hosts()),
                };
                options.transport = TransportOptions::Http {
                    bind,
                    path,
                    allowed_hosts,
                };
            }
            "--http-path" => {
                let path = normalize_http_path(next_value(&mut args, &arg)?)?;
                let (bind, allowed_hosts) = match options.transport {
                    TransportOptions::Http {
                        bind,
                        ref allowed_hosts,
                        ..
                    } => (bind, allowed_hosts.clone()),
                    TransportOptions::Stdio => (
                        "127.0.0.1:8080".parse().expect("valid default HTTP bind"),
                        default_allowed_hosts(),
                    ),
                };
                options.transport = TransportOptions::Http {
                    bind,
                    path,
                    allowed_hosts,
                };
            }
            "--http-allowed-host" => {
                let host = next_value(&mut args, &arg)?;
                if host.trim().is_empty() {
                    return Err(GrmError::Constraint(
                        "--http-allowed-host requires a non-empty host or host:port".into(),
                    ));
                }
                let (bind, path, mut allowed_hosts) = match options.transport {
                    TransportOptions::Http {
                        bind,
                        ref path,
                        ref allowed_hosts,
                    } => (bind, path.clone(), allowed_hosts.clone()),
                    TransportOptions::Stdio => (
                        "127.0.0.1:8080".parse().expect("valid default HTTP bind"),
                        "/mcp".into(),
                        default_allowed_hosts(),
                    ),
                };
                allowed_hosts.push(host);
                options.transport = TransportOptions::Http {
                    bind,
                    path,
                    allowed_hosts,
                };
            }
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
    "Usage: grm-mcp [--transport stdio|http] [--http-bind <host:port>] [--http-path <path>] [--http-allowed-host <host-or-host:port>] [--load-json <path>] [--load-bin <path>] [--import-json <path>] [--export-json <path>] [--autocommit-json <path>] [--autocommit-bin <path>]"
}

fn next_path(args: &mut impl Iterator<Item = String>, flag: &str) -> GrmResult<PathBuf> {
    next_value(args, flag).map(PathBuf::from)
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> GrmResult<String> {
    args.next()
        .ok_or_else(|| GrmError::Constraint(format!("{flag} requires a value")))
}

fn normalize_http_path(path: String) -> GrmResult<String> {
    if path.trim().is_empty() {
        return Err(GrmError::Constraint(
            "--http-path requires a non-empty absolute path".into(),
        ));
    }
    if !path.starts_with('/') {
        return Err(GrmError::Constraint(
            "--http-path must start with '/'".into(),
        ));
    }
    Ok(path)
}

fn default_allowed_hosts() -> Vec<String> {
    vec!["localhost".into(), "127.0.0.1".into(), "::1".into()]
}
