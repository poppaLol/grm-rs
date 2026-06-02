use std::io::{Cursor, Write};
use std::process::{Command, Stdio};

use grm_rs::CliSession;
use grm_service_api as svc;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

async fn seed_json_session(path: &std::path::Path) {
    let input = Cursor::new(format!(
        "model.define User userId name:string:required\nnode.create User name=Alice\nsession.save --json {}\nsession.exit\n",
        path.display()
    ));
    let mut session = CliSession::new(input, Vec::new());
    session.run().await.unwrap();
}

fn run_grm_session(args: &[&str], input: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_grm"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    drop(child.stdin.take());

    child.wait_with_output().unwrap()
}

fn run_grm_service_session(
    endpoint: &str,
    workspace_ref: &str,
    mode: Option<&str>,
    input: &str,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_grm"));
    command
        .arg("session")
        .env("GRM_BACKEND", "grpc")
        .env("GRM_SERVICE_ENDPOINT", endpoint)
        .env("GRM_WORKSPACE_REF", workspace_ref);
    if let Some(mode) = mode {
        command.env("GRM_SERVICE_WORKSPACE_MODE", mode);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    drop(child.stdin.take());

    child.wait_with_output().unwrap()
}

async fn start_workspace_service(
    root: &std::path::Path,
) -> (
    String,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = svc::GrpcWorkspaceService::with_local_workspace_root(root).into_server();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(service)
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown_rx.await;
            })
            .await
    });

    (format!("http://{addr}"), shutdown_tx, server)
}

#[tokio::test]
async fn session_load_startup_opens_existing_json_with_autocommit_off() {
    let tempdir = tempfile::tempdir().unwrap();
    let json_path = tempdir.path().join("session.json");
    seed_json_session(&json_path).await;

    let output = run_grm_session(
        &[
            "session",
            "--load",
            "json",
            json_path.to_str().expect("test path is utf-8"),
        ],
        "session.describe\nsession.exit\n",
    );

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Loaded session from JSON file"));
    assert!(stdout.contains("Autocommit: off."));
    assert!(stdout.contains("Loaded graph session ready"));
    assert!(stdout.contains("Session Summary"));
    assert!(stdout.contains("| node | User"));
}

#[tokio::test]
async fn session_load_startup_autocommit_on_persists_edits() {
    let tempdir = tempfile::tempdir().unwrap();
    let json_path = tempdir.path().join("session.json");
    seed_json_session(&json_path).await;

    let output = run_grm_session(
        &[
            "session",
            "--load",
            "json",
            json_path.to_str().expect("test path is utf-8"),
            "--autocommit",
            "on",
        ],
        "node.create User name=Bob\nsession.exit\n",
    );

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Autocommit: on ->"));

    let input = Cursor::new(format!(
        "session.load --json {}\nnode.find User name=Bob\nsession.exit\n",
        json_path.display()
    ));
    let mut session = CliSession::new(input, Vec::new());
    session.run().await.unwrap();
    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Node User userId=2 {name=Bob}"));
}

#[tokio::test]
async fn grpc_service_session_create_open_reopen_uses_binary_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let (endpoint, shutdown_tx, server) = start_workspace_service(temp.path()).await;

    let create_endpoint = endpoint.clone();
    let created = tokio::task::spawn_blocking(move || {
        run_grm_service_session(
            &create_endpoint,
            "cli-create-open",
            Some("create"),
            "model.define User userId name:string:required\nnode.create User name=Ada\nsession.describe\nsession.save --json should-stay-local.json\nsession.exit\n",
        )
    })
    .await
    .unwrap();

    assert!(created.status.success());
    let stdout = String::from_utf8(created.stdout).unwrap();
    assert!(stdout.contains("Service-backed workspace session ready"));
    assert!(stdout.contains("Backend: gRPC workspace storage"));
    assert!(stdout.contains("Workspace: cli-create-open"));
    assert!(stdout.contains("Mode: create"));
    assert!(stdout.contains("Persistence format: binary (default)"));
    assert!(stdout.contains("Scope: ExecuteWorkspace"));
    assert!(stdout.contains("Command is local-only or not supported in gRPC service CLI mode yet"));
    assert!(temp.path().join("cli-create-open.bin").exists());
    assert!(!temp.path().join("cli-create-open.json").exists());

    let open_endpoint = endpoint.clone();
    let opened = tokio::task::spawn_blocking(move || {
        run_grm_service_session(
            &open_endpoint,
            "cli-create-open",
            Some("open"),
            "node.find User name=Ada\nsession.describe\nsession.exit\n",
        )
    })
    .await
    .unwrap();

    assert!(opened.status.success());
    let stdout = String::from_utf8(opened.stdout).unwrap();
    assert!(stdout.contains("Mode: open"));
    assert!(stdout.contains("Persistence format: binary (default)"));
    assert!(stdout.contains("Node User id=1 {name=Ada}"));

    let reopen_endpoint = endpoint.clone();
    let reopened = tokio::task::spawn_blocking(move || {
        run_grm_service_session(
            &reopen_endpoint,
            "cli-create-open",
            None,
            "node.find User name=Ada\nsession.describe\nsession.exit\n",
        )
    })
    .await
    .unwrap();

    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();

    assert!(reopened.status.success());
    let stdout = String::from_utf8(reopened.stdout).unwrap();
    assert!(stdout.contains("Mode: open"));
    assert!(stdout.contains("Node User id=1 {name=Ada}"));
    assert!(stdout.contains("Stored rows: 1 nodes, 0 edges"));
    assert!(stdout.contains("| node | User | 1"));
}

#[tokio::test]
async fn grpc_service_session_node_find_supports_traversal_terms() {
    let temp = tempfile::tempdir().unwrap();
    let (endpoint, shutdown_tx, server) = start_workspace_service(temp.path()).await;

    let output = tokio::task::spawn_blocking(move || {
        run_grm_service_session(
            &endpoint,
            "cli-traversal-smoke",
            Some("create"),
            "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Ada\nnode.create Post title=Traversal\nedge.create Authored from=1 to=2 year=2026\nnode.find User name=Ada via=out:Authored:Post end.title=Traversal edge.year=2026 return=end order=title:asc limit=1 offset=0\nnode.find User name=Ada via=out:Authored:Post return=edge\nsession.exit\n",
        )
    })
    .await
    .unwrap();

    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Service-backed workspace session ready"));
    assert!(stdout.contains("Mode: create"));
    assert!(stdout.contains("Persistence format: binary (default)"));
    assert!(stdout.contains("Node Post id=2 {title=Traversal}"));
    assert!(stdout.contains("Edge Authored id=1 from=1 to=2 {year=2026}"));
    assert!(temp.path().join("cli-traversal-smoke.bin").exists());
}
