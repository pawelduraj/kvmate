//! `cargo xtask dist`: the files of a release in `dist/v<version>/`: the firmware, the Windows installer and
//! the app without an installer. It runs on Windows in a terminal with ESP-IDF loaded. The firmware is built
//! from `sdkconfig.defaults` alone, in `target/firmware`, so local menuconfig changes stay out of it, and the
//! app embeds that exact image.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const TARGET: &str = "x86_64-pc-windows-msvc";

pub fn dist(root: &Path) -> Result<(), String> {
    check(root)?;
    let out = root.join("dist").join(format!("v{VERSION}"));
    if out.exists() {
        fs::remove_dir_all(&out).map_err(|e| return format!("{}: {e}", out.display()))?;
    }
    fs::create_dir_all(&out).map_err(|e| return format!("{}: {e}", out.display()))?;

    let firmware = out.join(format!("kvmate-firmware-v{VERSION}-esp32s3.bin"));
    copy(&build_firmware(root)?, &firmware)?;
    let mut tauri = Command::new("cargo");
    tauri.args(["tauri", "build", "--target", TARGET, "--bundles", "nsis"]);
    run(tauri.current_dir(root.join("crates/hub")).env("KVMATE_FIRMWARE_BIN", &firmware))?;
    let release = root.join("target").join(TARGET).join("release");
    copy(&release.join("kvmate.exe"), &out.join(format!("kvmate-v{VERSION}-windows-x64.exe")))?;
    let setup = release.join(format!("bundle/nsis/kvmate_{VERSION}_x64-setup.exe"));
    copy(&setup, &out.join(format!("kvmate-v{VERSION}-windows-x64-setup.exe")))?;
    let names = checksums(&out)?;

    println!("\n{}:", out.display());
    for name in names {
        println!("  {name}");
    }
    println!("\nnext: git tag v{VERSION} && git push origin v{VERSION}, then a draft release with these files:");
    println!("  gh release create v{VERSION} --draft --title \"kvmate {VERSION}\" dist/v{VERSION}/*");
    return Ok(());
}

fn check(root: &Path) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("the release app is a Windows app: run this on Windows".into());
    }
    if std::env::var_os("IDF_PATH").is_none() {
        return Err("ESP-IDF is not loaded: open an ESP-IDF terminal or run its export.ps1 first".into());
    }
    if !output(root, "git", &["status", "--porcelain"])?.is_empty() {
        return Err("uncommitted changes: a release is built from a commit".into());
    }
    if output(root, "cargo", &["tauri", "--version"]).is_err() {
        return Err("no Tauri CLI: cargo install tauri-cli --version \"^2\" --locked".into());
    }
    return Ok(());
}

fn build_firmware(root: &Path) -> Result<PathBuf, String> {
    let build = root.join("target/firmware");
    let sdkconfig = build.join("sdkconfig");
    if sdkconfig.exists() {
        fs::remove_file(&sdkconfig).map_err(|e| return format!("{}: {e}", sdkconfig.display()))?;
    }
    let idf_py = PathBuf::from(std::env::var_os("IDF_PATH").unwrap()).join("tools/idf.py");
    let mut idf = Command::new("python");
    idf.arg(idf_py).arg("-B").arg(&build).arg("-D").arg(format!("SDKCONFIG={}", sdkconfig.display()));
    idf.args(["-D", "IDF_TARGET=esp32s3", "build", "merge-bin", "-o", "kvmate-firmware-merged.bin", "--format", "raw"]);
    run(idf.current_dir(root.join("firmware")))?;
    return Ok(build.join("kvmate-firmware-merged.bin"));
}

fn checksums(dir: &Path) -> Result<Vec<String>, String> {
    let mut names = fs::read_dir(dir)
        .map_err(|e| return format!("{}: {e}", dir.display()))?
        .map(|entry| return entry.map(|e| return e.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| return e.to_string())?;
    names.sort();
    let mut sums = String::new();
    for name in &names {
        let data = fs::read(dir.join(name)).map_err(|e| return format!("{name}: {e}"))?;
        let hex: String = Sha256::digest(&data).iter().map(|b| return format!("{b:02x}")).collect();
        sums.push_str(&format!("{hex}  {name}\n"));
    }
    fs::write(dir.join("SHA256SUMS"), sums).map_err(|e| return e.to_string())?;
    names.push("SHA256SUMS".into());
    return Ok(names);
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    fs::copy(from, to).map_err(|e| return format!("{}: {e}", from.display()))?;
    return Ok(());
}

fn run(command: &mut Command) -> Result<(), String> {
    let status = command.status().map_err(|e| return format!("{:?}: {e}", command.get_program()))?;
    if !status.success() {
        return Err(format!("{:?} {:?} failed ({status})", command.get_program(), command.get_args()));
    }
    return Ok(());
}

fn output(dir: &Path, program: &str, args: &[&str]) -> Result<String, String> {
    let out =
        Command::new(program).args(args).current_dir(dir).output().map_err(|e| return format!("{program}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{program} {} failed", args.join(" ")));
    }
    return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
}
