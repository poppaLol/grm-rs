use std::env;
use std::fs;
use std::io::{self, Read};

use grm_service_api::certificate_fingerprint_from_pem_or_der;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = match env::args_os().nth(1) {
        Some(path) => fs::read(path)?,
        None => {
            let mut bytes = Vec::new();
            io::stdin().read_to_end(&mut bytes)?;
            bytes
        }
    };
    let fingerprint = certificate_fingerprint_from_pem_or_der(&bytes)?;
    println!("{}", fingerprint.as_hex());
    Ok(())
}
