//! Emit a bounded binary atlas fixture for the isolated renderer candidate.
use anyhow::{Result, ensure};
use std::io::Write;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(
        args.len() == 2,
        "usage: game_asset_renderer_fixture NEW_FILE.bin"
    );
    let bytes = gcad::view::asset::renderer_fixture()?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?;
    file.write_all(&bytes)?;
    println!(
        "PASS: two gcad atlas charts, three prepared map slots, four levels; {} bytes",
        bytes.len()
    );
    Ok(())
}
