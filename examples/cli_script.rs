use std::io::Cursor;
use std::io::IsTerminal;

use grm_rs::CliSession;

fn should_enable_color() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

#[tokio::main]
async fn main() -> grm_rs::Result<()> {
    let script = include_str!("session_setup.grm");
    let input = Cursor::new(script);
    let output = Vec::new();
    let mut session = CliSession::new_with_color(input, output, should_enable_color());

    session.run_script().await?;

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output)
        .map_err(|err| grm_rs::GrmError::Backend(err.to_string()))?;

    println!("Ran examples/session_setup.grm");
    println!("{output}");

    Ok(())
}
