use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::{env, fs, io};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

use grm_service_api::{GrpcServerTlsOptions, GrpcWorkspaceService, ServiceSecurityConfig};
use tonic::transport::Server;

const GRM_SERVICE_SECURITY_PROFILE_ENV: &str = "GRM_SERVICE_SECURITY_PROFILE";
const GRM_SERVICE_SECURITY_CONFIG_ENV: &str = "GRM_SERVICE_SECURITY_CONFIG";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let addr: SocketAddr = args
        .next()
        .unwrap_or_else(|| "127.0.0.1:50051".into())
        .parse()?;
    let workspace_root = prepare_workspace_root(args.next().map(PathBuf::from))?;

    let security = service_security_config_from_env(|key| env::var_os(key))?;
    security.validate_bind_addr(addr)?;
    let service =
        GrpcWorkspaceService::with_local_workspace_root(&workspace_root, security).into_server();
    let tls = GrpcServerTlsOptions::from_env()?;
    let mut server = Server::builder();
    let transport = if let Some(tls) = tls {
        let requires_client_authentication = tls.requires_client_authentication();
        server = server.tls_config(tls.server_tls_config()?)?;
        if requires_client_authentication {
            "mutual TLS"
        } else {
            "TLS"
        }
    } else {
        "insecure local gRPC"
    };

    println!(
        "serving local GRM workspace gRPC shell on {addr}; workspace root: {}; transport: {transport}",
        workspace_root.display()
    );
    server.add_service(service).serve(addr).await?;

    Ok(())
}

fn service_security_config_from_env(
    getenv: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> io::Result<ServiceSecurityConfig> {
    let config_path = getenv(GRM_SERVICE_SECURITY_CONFIG_ENV).map(PathBuf::from);
    let Some(profile) = getenv(GRM_SERVICE_SECURITY_PROFILE_ENV) else {
        if let Some(config_path) = config_path {
            return ServiceSecurityConfig::secured_from_config_file(config_path)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()));
        }
        return Ok(ServiceSecurityConfig::anonymous_local());
    };
    match profile.to_string_lossy().as_ref() {
        "anonymous_local" => Ok(ServiceSecurityConfig::anonymous_local()),
        "docker_local_insecure" => Ok(ServiceSecurityConfig::docker_local_insecure()),
        "secured" => match config_path {
            Some(config_path) => ServiceSecurityConfig::secured_from_config_file(config_path)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string())),
            None => Ok(ServiceSecurityConfig::secured()),
        },
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{GRM_SERVICE_SECURITY_PROFILE_ENV} must be one of anonymous_local, docker_local_insecure, secured"
            ),
        )),
    }
}

fn prepare_workspace_root(explicit: Option<PathBuf>) -> io::Result<PathBuf> {
    let root = match explicit {
        Some(root) => root,
        None => default_workspace_root()?,
    };
    match fs::symlink_metadata(&root) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            create_private_workspace_root(&root)?;
        }
        Err(error) => return Err(error),
    }
    validate_workspace_root(&root)?;
    Ok(root)
}

fn default_workspace_root() -> io::Result<PathBuf> {
    default_workspace_root_from_env(|key| env::var_os(key))
}

fn default_workspace_root_from_env(
    getenv: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> io::Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let base = getenv("LOCALAPPDATA")
            .or_else(|| getenv("APPDATA"))
            .map(PathBuf::from)
            .ok_or_else(missing_user_data_dir)?;
        Ok(base.join("GRM").join("service-workspaces"))
    }

    #[cfg(target_os = "macos")]
    {
        let home = getenv("HOME")
            .map(PathBuf::from)
            .ok_or_else(missing_user_data_dir)?;
        Ok(home
            .join("Library")
            .join("Application Support")
            .join("grm")
            .join("service-workspaces"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let base = getenv("XDG_DATA_HOME")
            .filter(|value| Path::new(value).is_absolute())
            .map(PathBuf::from)
            .or_else(|| getenv("HOME").map(|home| PathBuf::from(home).join(".local/share")))
            .ok_or_else(missing_user_data_dir)?;
        Ok(base.join("grm").join("service-workspaces"))
    }
}

fn missing_user_data_dir() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        "workspace root must be provided when no per-user data directory is available",
    )
}

fn create_private_workspace_root(root: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700).create(root)
    }

    #[cfg(not(unix))]
    {
        fs::create_dir_all(root)
    }
}

fn validate_workspace_root(root: &Path) -> io::Result<()> {
    let symlink_metadata = fs::symlink_metadata(root)?;
    if symlink_metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("workspace root '{}' must not be a symlink", root.display()),
        ));
    }
    if !symlink_metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("workspace root '{}' must be a directory", root.display()),
        ));
    }

    #[cfg(unix)]
    validate_unix_workspace_root(root, &symlink_metadata)?;

    Ok(())
}

#[cfg(unix)]
fn validate_unix_workspace_root(root: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    let current_uid = unsafe { libc::geteuid() };
    if metadata.uid() != current_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "workspace root '{}' must be owned by the current user",
                root.display()
            ),
        ));
    }

    let mode = metadata.permissions().mode();
    if mode & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "workspace root '{}' must not be accessible by group or other users",
                root.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_workspace_root_uses_per_user_data_directory() {
        let root = default_workspace_root_from_env(|key| match key {
            "XDG_DATA_HOME" => Some("/home/alice/.local/state".into()),
            "HOME" => Some("/home/alice".into()),
            _ => None,
        })
        .unwrap();

        assert!(root.ends_with(Path::new("grm").join("service-workspaces")));
        assert!(!root.starts_with(env::temp_dir()));
    }

    #[test]
    fn default_workspace_root_requires_user_data_directory() {
        let err = default_workspace_root_from_env(|_| None).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn explicit_workspace_root_is_created_and_accepted() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("private-root");

        let prepared = prepare_workspace_root(Some(root.clone())).unwrap();

        assert_eq!(prepared, root);
        assert!(prepared.is_dir());
    }

    #[test]
    fn default_security_profile_is_anonymous_local() {
        let security = service_security_config_from_env(|_| None).unwrap();
        let public: SocketAddr = "0.0.0.0:50051".parse().unwrap();

        assert!(security.validate_bind_addr(public).is_err());
    }

    #[test]
    fn docker_local_insecure_profile_allows_container_bind() {
        let security = service_security_config_from_env(|key| {
            (key == GRM_SERVICE_SECURITY_PROFILE_ENV).then_some("docker_local_insecure".into())
        })
        .unwrap();
        let public: SocketAddr = "0.0.0.0:50051".parse().unwrap();

        assert!(security.validate_bind_addr(public).is_ok());
    }

    #[test]
    fn unknown_security_profile_is_rejected() {
        let result = service_security_config_from_env(|key| {
            (key == GRM_SERVICE_SECURITY_PROFILE_ENV).then_some("surprise".into())
        });
        let Err(err) = result else {
            panic!("unknown security profile should be rejected");
        };

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn security_config_file_selects_secured_profile() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("security.json");
        fs::write(
            &config,
            r#"{
              "certificate_mappings": [
                {
                  "fingerprint_sha256": "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                  "principal": { "issuer": "local-demo", "subject": "client" }
                }
              ],
              "permission_table": {
                "version": "test-policy-v1",
                "assignments": [
                  {
                    "principal": { "issuer": "local-demo", "subject": "client" },
                    "scope": { "kind": "service" },
                    "permissions": [
                      { "action": "workspace.create", "resource": { "kind": "service" } }
                    ]
                  }
                ]
              }
            }"#,
        )
        .unwrap();

        let security = service_security_config_from_env(|key| match key {
            GRM_SERVICE_SECURITY_PROFILE_ENV => Some("secured".into()),
            GRM_SERVICE_SECURITY_CONFIG_ENV => Some(config.clone().into_os_string()),
            _ => None,
        })
        .unwrap();
        let public: SocketAddr = "0.0.0.0:50051".parse().unwrap();

        assert!(security.validate_bind_addr(public).is_ok());
    }

    #[test]
    fn invalid_security_config_file_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("security.json");
        fs::write(&config, r#"{"certificate_mappings":[]}"#).unwrap();

        let Err(err) = service_security_config_from_env(|key| match key {
            GRM_SERVICE_SECURITY_PROFILE_ENV => Some("secured".into()),
            GRM_SERVICE_SECURITY_CONFIG_ENV => Some(config.clone().into_os_string()),
            _ => None,
        }) else {
            panic!("invalid security config should be rejected");
        };

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn workspace_root_rejects_non_directory_final_path() {
        let file = tempfile::NamedTempFile::new().unwrap();

        let err = prepare_workspace_root(Some(file.path().to_path_buf())).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn workspace_root_rejects_symlink_final_path() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let err = prepare_workspace_root(Some(link)).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn workspace_root_rejects_group_or_other_accessible_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("open-root");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();

        let err = prepare_workspace_root(Some(root)).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }
}
