use std::io::Cursor;

use grm_rs::CliSession;

#[tokio::main]
async fn main() -> grm_rs::Result<()> {
    let script = include_str!("session_setup.grm");
    let input = Cursor::new(script);
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run_script().await?;

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output)
        .map_err(|err| grm_rs::GrmError::Backend(err.to_string()))?;

    println!("Ran examples/session_setup.grm");
    println!("{output}");

    Ok(())
}
