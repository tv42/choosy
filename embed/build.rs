// wasm-pack is full of crazy complexity, talk about uploading
// to NPM, downloading binaries from internet whenever, and
// littering ~/.cache when I prefer things to be more contained.
// I'm sure somebody felt it was needed, but all I'm doing is
// serving a wasm file to browser.
//
// I simply have no faith in wasm-pack doing the right thing,
// for me personally. I have even less faith in me remembering
// to run some wasm-creating wrapper when needed.
// Just make the build do the right thing!
//
// Let's try to do something simpler.
//
// See also https://github.com/rustwasm/wasm-pack/issues/251
//
// Also, what's with the *hidden* cache dir,
// ~/.cache/.wasm-pack?

use anyhow::anyhow;
use cargo_metadata::MetadataCommand;
use std::env;
use std::fs;
use std::io;
use std::process::{Command, Stdio};

fn cargo_build_frontend() -> Result<(), anyhow::Error> {
    let cargo = env::var_os("CARGO").unwrap();

    const DIR: &str = "../frontend";
    println!("cargo:rerun-if-changed={}/Cargo.toml", DIR);
    println!("cargo:rerun-if-changed={}/src", DIR);
    println!("cargo:rerun-if-changed=../protocol/Cargo.toml");
    println!("cargo:rerun-if-changed=../protocol/src");

    // pass `-vv` to cargo build if you want to see
    // output from build scripts and their subprocesses.
    let status = Command::new(cargo)
        .args(&[
            "build",
            "--lib",
            "--release",
            "-v",
            "--target=wasm32-unknown-unknown",
            // using the same target directory would deadlock on
            // flocking target/release/.cargo-lock which is held by
            // the parent `cargo build --release`.
            //
            // (non-release builds work because the wasm is always in
            // release mode).
            //
            // avoid this by giving the frontend build its own target
            // directory. this means any building done directly via
            // `cargo -p frontend` duplicates effort & caching -- but
            // those builds would likely not target wasm anyway.
            //
            // https://github.com/rust-lang/cargo/issues/6412#issuecomment-539976015
            "--target-dir=./target",
        ])
        .current_dir(DIR)
        .stdin(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(anyhow!("wasm cargo build failed"));
    }
    Ok(())
}

// wasm-bindgen demands that the executable and the library
// used have matching versions.
fn get_wasm_bindgen_version() -> String {
    let options: Vec<String> = ["--locked"].iter().map(|s| s.to_string()).collect();
    let metadata = MetadataCommand::new()
        .manifest_path("../frontend/Cargo.toml")
        .other_options(options)
        .exec()
        .expect("Cannot run cargo-metadata");
    let package = metadata
        .packages
        .into_iter()
        .find(|pkg| pkg.name == "wasm-bindgen")
        .expect("Cannot find dependency on wasm-bindgen");
    package.version.to_string()
}

fn cargo_install_wasm_bindgen(
    version: &str,
    out_dir: &str,
    tmp_dir: &str,
) -> Result<String, anyhow::Error> {
    // it would be nice to use a cargo-spawning library
    // written by someone else, but the `cargo` crate is
    // an impenetrable behemoth, and the `escargot` crate
    // doesn't seem to have e.g. install.

    let cargo = env::var_os("CARGO").unwrap();
    let status = Command::new(cargo)
        .args(&[
            "install",
            "--root",
            out_dir,
            "--version",
            version,
            "--features=vendored-openssl",
            "wasm-bindgen-cli",
        ])
        .stdin(Stdio::null())
        // This can use prodiguous amount of temp file space,
        // and on many modern boxes $TMPDIR points of a tmpfs
        // where individual users are limited to e.g. 10% of
        // physical RAM. Without this, builds on 8GB hosts can
        // fail.
        .env("TMPDIR", tmp_dir)
        .status()?;
    if !status.success() {
        return Err(anyhow!("cargo install wasm-bindgen-cli failed"));
    }
    let executable = out_dir.to_string() + "/bin/wasm-bindgen";
    Ok(executable)
}

fn create_dir_if_not_exist(p: &str) -> io::Result<()> {
    fs::create_dir(p).or_else(|error| match error.kind() {
        io::ErrorKind::AlreadyExists => Ok(()),
        _ => Err(error),
    })?;
    Ok(())
}

fn main() -> Result<(), anyhow::Error> {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = env::var("OUT_DIR")?;
    let tmp_dir = out_dir.to_owned() + "/tmp";
    create_dir_if_not_exist(&tmp_dir)?;

    cargo_build_frontend().expect("cargo build of frontend");

    let wasm_bindgen_version = get_wasm_bindgen_version();
    println!("cargo:warning=wasm-bindgen v{}", wasm_bindgen_version);
    // it will actually be installed in <out_dir>/bin/
    let wasm_bindgen = cargo_install_wasm_bindgen(&wasm_bindgen_version, &out_dir, &tmp_dir)?;

    println!("cargo:warning=see {}", out_dir);

    // this creates <wasm_out>/wasm.js & wasm_bg.wasm
    let wasm_out = out_dir + "/wasm";
    create_dir_if_not_exist(&wasm_out)?;
    let status = Command::new(wasm_bindgen)
        .args(&[
            "--out-dir",
            &wasm_out,
            "--no-typescript",
            "--target=web",
            "../frontend/target/wasm32-unknown-unknown/release/choosy_frontend.wasm",
        ])
        .stdin(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(anyhow!("wasm-bindgen failed"));
    }

    // wasm-pack runs wasm-opt (C++), but it does it by downloading a binary,
    // in an incredibly obfuscated way.
    //
    // https://github.com/WebAssembly/binaryen
    //
    // ```
    // wasm-opt input.wasm -o output.wasm -O
    // ```

    Ok(())
}
