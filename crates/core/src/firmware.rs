//! The firmware image bundled with the hub, and installing or removing it through the board's
//! "COM" port with espflash. The image is one merged file (bootloader + partition table +
//! application) written to the board from address 0.
//!
//! Only the flash memory is written or erased. eFuses are never written, so the board stays
//! usable for anything else afterwards. Before writing, the chip must be an ESP32-S3 without
//! Secure Boot or flash encryption; otherwise nothing is changed.

use espflash::connection::{Connection, ResetAfterOperation, ResetBeforeOperation};
use espflash::flasher::{FlashSize, Flasher};
use espflash::target::{Chip, ProgressCallbacks};
use serde::Serialize;
use serialport::{FlowControl, SerialPortType, UsbPortInfo};

const IMAGE_MAGIC: u8 = 0xE9;
const CHIP_ID_ESP32S3: u16 = 9;
const PARTITION_TABLE_OFFSET: usize = 0x8000;
const PARTITION_ENTRY_MAGIC: [u8; 2] = [0xAA, 0x50];
const PARTITION_TYPE_APP: u8 = 0x00;
const APP_DESC_MAGIC: u32 = 0xABCD_5432;
const APP_DESC_OFFSET: usize = 32;
const FLASH_BAUD: u32 = 921_600;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ImageInfo {
    pub version: String,
    pub project: String,
    pub built: String,
    pub idf_version: String,
    #[serde(serialize_with = "serialize_hex")]
    pub build_id: [u8; 8],
    pub size: usize,
}

fn serialize_hex<S: serde::Serializer>(bytes: &[u8; 8], s: S) -> Result<S::Ok, S::Error> {
    return s.serialize_str(&crate::encoding::hex(bytes));
}

#[derive(Clone, Debug)]
pub struct FirmwareImage {
    pub bytes: &'static [u8],
    pub info: ImageInfo,
}

impl FirmwareImage {
    pub fn new(bytes: &'static [u8]) -> Option<Result<FirmwareImage, String>> {
        if bytes.is_empty() {
            return None;
        }
        return Some(parse(bytes).map(|info| return FirmwareImage { bytes, info }));
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    return Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?));
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    return Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?));
}

fn text_at(b: &[u8], at: usize, len: usize) -> Option<String> {
    let raw = b.get(at..at + len)?;
    let end = raw.iter().position(|&c| return c == 0).unwrap_or(len);
    return Some(String::from_utf8_lossy(&raw[..end]).into_owned());
}

fn check_image(b: &[u8], at: usize, what: &str) -> Result<(), String> {
    if b.get(at) != Some(&IMAGE_MAGIC) {
        return Err(format!("{what}: no ESP image header"));
    }
    match u16_at(b, at + 12) {
        Some(CHIP_ID_ESP32S3) => return Ok(()),
        Some(id) => return Err(format!("{what}: image for another chip (chip id {id}), an ESP32-S3 is needed")),
        None => return Err(format!("{what}: image truncated")),
    }
}

fn app_offset(b: &[u8]) -> Result<usize, String> {
    for i in 0..95 {
        let e = PARTITION_TABLE_OFFSET + i * 32;
        let entry = b.get(e..e + 32).ok_or("partition table truncated")?;
        if entry[..2] != PARTITION_ENTRY_MAGIC {
            break;
        }
        if entry[2] == PARTITION_TYPE_APP {
            return Ok(u32_at(entry, 4).unwrap() as usize);
        }
    }
    return Err("no app partition in the partition table".into());
}

fn parse(b: &[u8]) -> Result<ImageInfo, String> {
    check_image(b, 0, "bootloader")?;
    let app = app_offset(b)?;
    check_image(b, app, "app")?;
    let d = app + APP_DESC_OFFSET;
    if u32_at(b, d) != Some(APP_DESC_MAGIC) {
        return Err("app: no ESP-IDF app description (esp_app_desc_t)".into());
    }
    let field = |at, len| return text_at(b, d + at, len).ok_or_else(|| return "app: description truncated".to_string());
    let sha = b.get(d + 144..d + 152).ok_or("app: description truncated")?;
    return Ok(ImageInfo {
        version: field(16, 32)?,
        project: field(48, 32)?,
        built: format!("{} {}", field(96, 16)?, field(80, 16)?),
        idf_version: field(112, 32)?,
        build_id: sha.try_into().unwrap(),
        size: b.len(),
    });
}

#[derive(Clone, Copy, Debug)]
pub enum FlashJob {
    Install(&'static [u8]),
    Erase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FlashStage {
    Connecting,
    Erasing,
    Writing,
    Verifying,
    Restarting,
}

pub type FlashProgress<'a> = &'a mut dyn FnMut(FlashStage, Option<f32>);

struct Callbacks<'a> {
    report: FlashProgress<'a>,
    total: usize,
}

impl ProgressCallbacks for Callbacks<'_> {
    fn init(&mut self, _addr: u32, total: usize) {
        self.total = total.max(1);
        (self.report)(FlashStage::Writing, Some(0.0));
    }

    fn update(&mut self, current: usize) {
        (self.report)(FlashStage::Writing, Some(current as f32 / self.total as f32));
    }

    fn verifying(&mut self) {
        (self.report)(FlashStage::Verifying, None);
    }

    fn finish(&mut self, _skipped: bool) {}
}

fn usb_info(port: &str) -> UsbPortInfo {
    return serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .find(|p| return p.port_name.eq_ignore_ascii_case(port))
        .and_then(|p| match p.port_type {
            SerialPortType::UsbPort(info) => return Some(info),
            _ => return None,
        })
        .unwrap_or(UsbPortInfo { vid: 0, pid: 0, serial_number: None, manufacturer: None, product: None });
}

pub fn flash(port: &str, job: FlashJob, report: FlashProgress<'_>) -> Result<String, String> {
    report(FlashStage::Connecting, None);
    let serial = serialport::new(port, 115_200)
        .flow_control(FlowControl::None)
        .open_native()
        .map_err(|e| format!("{port}: can't open the port ({e})"))?;
    let connection = Connection::new(
        serial,
        usb_info(port),
        ResetAfterOperation::HardReset,
        ResetBeforeOperation::DefaultReset,
        115_200,
    );
    let mut flasher = match Flasher::connect(connection, true, true, false, None, Some(FLASH_BAUD)) {
        Ok(flasher) => flasher,
        Err(e) => {
            restart(port);
            return Err(format!(
                "{port}: no answer from the ESP bootloader ({e}). Is this the “COM” port of an ESP32-S3 board?"
            ));
        }
    };

    let result = checked_job(&mut flasher, port, job, report);
    report(FlashStage::Restarting, None);
    if matches!((job, &result), (FlashJob::Install(_), Ok(_))) {
        return result;
    }
    let chip = flasher.chip();
    let restarted = flasher.connection().reset_after(true, chip);
    match (result, restarted) {
        (Ok(summary), Ok(())) => return Ok(summary),
        (Ok(summary), Err(e)) => return Ok(format!("{summary}; couldn't restart the board ({e}) — press RST")),
        (Err(e), _) => return Err(e),
    }
}

fn restart(port: &str) {
    if let Ok(serial) = serialport::new(port, 115_200).flow_control(FlowControl::None).open_native() {
        let (after, before) = (ResetAfterOperation::HardReset, ResetBeforeOperation::NoReset);
        let _ = Connection::new(serial, usb_info(port), after, before, 115_200).reset();
    }
}

fn checked_job(flasher: &mut Flasher, port: &str, job: FlashJob, report: FlashProgress<'_>) -> Result<String, String> {
    let chip = flasher.chip();
    if chip != Chip::Esp32s3 {
        return Err(format!(
            "{port}: the chip is {chip}, but kvmate firmware is for the ESP32-S3 — nothing was changed"
        ));
    }
    let security = flasher.security_info().map_err(|e| format!("{port}: {e}"))?;
    let secure_boot = security.flags & 1 != 0;
    let encrypted = security.flash_crypt_cnt.count_ones() % 2 == 1;
    if secure_boot || encrypted {
        return Err(format!(
            "{port}: the board has Secure Boot or flash encryption enabled — kvmate doesn't support such boards, nothing was changed"
        ));
    }
    let info = flasher.device_info().map_err(|e| format!("{port}: {e}"))?;
    let flash = format!("{}", info.flash_size);
    match job {
        FlashJob::Install(image) => {
            if matches!(info.flash_size, FlashSize::_1Mb | FlashSize::_2Mb) {
                return Err(format!("{port}: flash too small ({flash}), at least 4 MB is needed"));
            }
            let mut callbacks = Callbacks { report, total: 0 };
            flasher
                .write_bin_to_flash(0, image, &mut callbacks)
                .map_err(|e| format!("{port}: writing failed ({e}) — you can try again"))?;
            return Ok(format!(
                "{port}: firmware installed ({} kB) on the ESP32-S3 (flash {flash})",
                image.len() / 1024
            ));
        }
        FlashJob::Erase => {
            report(FlashStage::Erasing, None);
            flasher.erase_flash().map_err(|e| format!("{port}: erasing failed ({e})"))?;
            return Ok(format!(
                "{port}: ESP32-S3 flash erased (flash {flash}) — the board is ready for other projects"
            ));
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn fake_image(chip_id: u16, version: &str) -> Vec<u8> {
        let mut b = vec![0xFF; 0x10000 + 0x200];
        let header = |b: &mut Vec<u8>, at: usize| {
            b[at] = IMAGE_MAGIC;
            b[at + 12..at + 14].copy_from_slice(&chip_id.to_le_bytes());
        };
        header(&mut b, 0);
        let mut nvs = [0u8; 32];
        nvs[..2].copy_from_slice(&PARTITION_ENTRY_MAGIC);
        nvs[2] = 0x01;
        nvs[4..8].copy_from_slice(&0x9000u32.to_le_bytes());
        b[0x8000..0x8020].copy_from_slice(&nvs);
        let mut app = [0u8; 32];
        app[..2].copy_from_slice(&PARTITION_ENTRY_MAGIC);
        app[4..8].copy_from_slice(&0x10000u32.to_le_bytes());
        b[0x8020..0x8040].copy_from_slice(&app);
        header(&mut b, 0x10000);
        let d = 0x10000 + APP_DESC_OFFSET;
        b[d..d + 4].copy_from_slice(&APP_DESC_MAGIC.to_le_bytes());
        b[d + 16..d + 256].fill(0);
        b[d + 16..d + 16 + version.len()].copy_from_slice(version.as_bytes());
        b[d + 48..d + 63].copy_from_slice(b"kvmate-firmware");
        b[d + 80..d + 88].copy_from_slice(b"03:52:15");
        b[d + 96..d + 107].copy_from_slice(b"Oct 11 2026");
        b[d + 112..d + 118].copy_from_slice(b"v5.5.2");
        b[d + 144..d + 152].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        return b;
    }

    #[test]
    fn reads_app_description() {
        let info = parse(&fake_image(CHIP_ID_ESP32S3, "0.1.0")).unwrap();
        assert_eq!(info.version, "0.1.0");
        assert_eq!(info.project, "kvmate-firmware");
        assert_eq!(info.built, "Oct 11 2026 03:52:15");
        assert_eq!(crate::encoding::hex(&info.build_id), "0102030405060708");
    }

    #[test]
    fn rejects_other_chips_and_garbage() {
        assert!(parse(&fake_image(5, "0.1.0")).unwrap_err().contains("another chip"));
        assert!(parse(&[0u8; 100]).is_err());
        let mut no_app = fake_image(CHIP_ID_ESP32S3, "0.1.0");
        no_app[0x8022] = 0x01;
        assert!(parse(&no_app).unwrap_err().contains("partition"));
    }

    #[test]
    fn parses_real_build_if_present() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../firmware/build");
        let (Ok(boot), Ok(table), Ok(app)) = (
            std::fs::read(dir.join("bootloader/bootloader.bin")),
            std::fs::read(dir.join("partition_table/partition-table.bin")),
            std::fs::read(dir.join("kvmate-firmware.bin")),
        ) else {
            return;
        };
        let mut merged = vec![0xFF; 0x10000 + app.len()];
        merged[..boot.len()].copy_from_slice(&boot);
        merged[0x8000..0x8000 + table.len()].copy_from_slice(&table);
        merged[0x10000..].copy_from_slice(&app);
        let info = parse(&merged).unwrap();
        assert_eq!(info.project, "kvmate-firmware");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn missing_port_is_reported() {
        let mut stages = Vec::new();
        let err = flash("COM-does-not-exist", FlashJob::Erase, &mut |s, _| stages.push(s)).unwrap_err();
        assert!(err.contains("can't open the port"), "{err}");
        assert_eq!(stages, [FlashStage::Connecting]);
    }
}
