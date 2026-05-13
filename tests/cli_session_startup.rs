use std::io::{Cursor, Write};
use std::process::{Command, Stdio};

use grm_rs::CliSession;

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

    child.wait_with_output().unwrap()
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
