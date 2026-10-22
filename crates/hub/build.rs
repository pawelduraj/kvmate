//! CI passes a ready firmware image in `KVMATE_FIRMWARE_BIN`. Without it the image is put together
//! here from `firmware/build`, so a developer build always carries the firmware last built.

use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    bundle_firmware();
    tauri_build::build();
}

fn bundle_firmware() {
    println!("cargo:rerun-if-env-changed=KVMATE_FIRMWARE_BIN");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("kvmate-firmware.bin");
    let image = match env::var("KVMATE_FIRMWARE_BIN") {
        Ok(path) => {
            println!("cargo:rerun-if-changed={path}");
            fs::read(&path).unwrap_or_else(|e| panic!("KVMATE_FIRMWARE_BIN={path}: {e}"))
        }
        Err(_) => merge_idf_build(Path::new("../../firmware/build")).unwrap_or_else(|| {
            println!("cargo:warning=no firmware build in firmware/build: this app build cannot install firmware");
            return Vec::new();
        }),
    };
    fs::write(out, image).unwrap();
}

fn merge_idf_build(dir: &Path) -> Option<Vec<u8>> {
    let args = dir.join("flasher_args.json");
    println!("cargo:rerun-if-changed={}", args.display());
    let json: serde_json::Value = serde_json::from_str(&fs::read_to_string(&args).ok()?).ok()?;
    let mut image = Vec::new();
    for (offset, file) in json["flash_files"].as_object()? {
        let offset = usize::from_str_radix(offset.trim_start_matches("0x"), 16).ok()?;
        let path = dir.join(file.as_str()?);
        println!("cargo:rerun-if-changed={}", path.display());
        let data = fs::read(&path).ok()?;
        if image.len() < offset + data.len() {
            image.resize(offset + data.len(), 0xFF);
        }
        image[offset..offset + data.len()].copy_from_slice(&data);
    }
    return Some(image);
}
