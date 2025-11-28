use std::env;
use std::fs::{read, write};
use std::path::PathBuf;

use anyhow::{Result, anyhow};

const LINKER_SCRIPT_NAME: &str = "memory.x";

fn main() -> Result<()> {
    let out = PathBuf::from(
        env::var_os("OUT_DIR").ok_or(anyhow!("failed to find OUT_DIR environment variable"))?,
    );

    write(out.join(LINKER_SCRIPT_NAME), read(LINKER_SCRIPT_NAME)?)?;

    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed={}", LINKER_SCRIPT_NAME);
    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");

    Ok(())
}
