use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| manifest_dir.join("../.."));
    let console_dir = workspace.join("apps/console");
    let dist_index = console_dir.join("dist/index.html");
    let placeholder = manifest_dir.join("console-placeholder/index.html");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("console");

    println!("cargo:rerun-if-changed={}", dist_index.display());
    println!(
        "cargo:rerun-if-changed={}",
        console_dir.join("package.json").display()
    );
    println!("cargo:rerun-if-changed={}", placeholder.display());

    if out_dir.exists() {
        fs::remove_dir_all(&out_dir).expect("clear console embed dir");
    }
    fs::create_dir_all(&out_dir).expect("create console embed dir");

    if dist_index.is_file() {
        copy_dir(&console_dir.join("dist"), &out_dir).expect("copy console dist");
        return;
    }

    if npm_on_path() && console_dir.join("package.json").is_file() {
        run_npm(&console_dir, &["ci"]);
        run_npm(&console_dir, &["run", "build"]);
        if !dist_index.is_file() {
            panic!("npm run build did not produce {}", dist_index.display());
        }
        copy_dir(&console_dir.join("dist"), &out_dir).expect("copy console dist");
        return;
    }

    fs::copy(&placeholder, out_dir.join("index.html")).expect("copy console placeholder");
}

fn npm_on_path() -> bool {
    Command::new("npm")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn run_npm(dir: &Path, args: &[&str]) {
    let status = Command::new("npm")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|err| panic!("spawn npm {args:?}: {err}"));
    if !status.success() {
        panic!("npm {args:?} failed: {status}");
    }
}

fn copy_dir(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}
