# kvmate

One keyboard, mouse, headset and microphone for two computers, with sound and the microphone working on **both at the same time**.

kvmate is a Windows tray app and a small ESP32-S3 board. To the second computer the board is an ordinary USB keyboard, mouse and sound card, so it needs no software or drivers there.

- **Keyboard and mouse** switch with a double tap of Right Ctrl (or Left Ctrl, or Right Shift), from the tray, from the window, or with the button on the board (a short press switches, a long press brings them back to the PC).
- **Sound from both computers** plays in the headset connected to the PC.
- **The microphone works on both computers at once**, e.g. a video call on the second computer while you keep working on the PC.
- **Cable or Wi-Fi**: the board talks to the PC over USB, or over Wi-Fi through your router or its own network, encrypted with AES-256-GCM.
- **Firmware from the app**: the app installs and updates the board's firmware itself, with no flashing tools, and can wipe the board for other projects. The firmware never burns eFuses, so nothing it does is permanent.

Video doesn't go through kvmate: the monitors stay connected to both computers (a laptop e.g. through a dock) and you switch the monitor's input.

Status: first version. The app runs on Windows x64 (developed and tested on Windows 11); the other computer can run any system that supports USB keyboards and USB audio.

## How it works

```
 PC (Windows)                            ESP32-S3 board                          other computer
 ┌───────────────────────────────┐ USB  ┌──────────────────────────┐  USB  ┌─────────────────────────┐
 │ kvmate app                    │─────►│ “COM” port (USB-UART)    │       │ sees:                   │
 │  • takes over the keyboard    │◄─────│   firmware (ESP-IDF)     │       │  • keyboard and mouse   │
 │    and mouse on the shortcut  │      │                          │       │  • “kvmate” headphones  │
 │  • microphone → other computer│      │ “USB” port (native USB)  │──────►│  • “kvmate” microphone  │
 │  • its sound → headphones     │      │   HID + USB Audio        │(dock) │                         │
 └───────────────────────────────┘      └──────────────────────────┘       └─────────────────────────┘
```

- The app captures the keyboard and mouse with low-level hooks and Raw Input. Keys go out as positions (USB HID usages), so the other computer applies its own layout, and mouse movement goes out raw, so it applies its own acceleration.
- The app and the board exchange binary frames with a CRC over the USB-UART bridge, or the same frames in encrypted UDP datagrams over Wi-Fi.
- The board's native USB port is a composite device: HID keyboard, HID mouse with media keys, and a USB Audio Class 1 sound card (48 kHz stereo out, mono in), which every system supports without drivers.
- The PC is the sound hub: the headset stays connected to it. The app plays the other computer's sound on the headset, sends the PC's microphone to the board, and adjusts the playback speed slightly to make up for the difference between the two clocks.

## Hardware

- An **ESP32-S3** board with two USB-C ports: native USB (to the other computer) and a USB-UART bridge (to the PC), e.g. YD-ESP32-S3 (CH343 bridge) or Espressif ESP32-S3-DevKitC-1 (CP210x). The app also recognises CH340 and FTDI bridges.
- Two USB-C cables that carry data.

## Getting started

There are no ready-made downloads yet, so kvmate is built from source on Windows. You need:
- Rust (rustup, stable toolchain);
- Visual Studio Build Tools with “Desktop development with C++”;
- ESP-IDF v5.5.x, for the firmware.

1. Clone this repository.
2. Build the firmware, in a terminal with ESP-IDF loaded (the ESP-IDF terminal in VS Code, or `export.ps1` from the ESP-IDF folder):
   ```
   cd firmware
   idf.py set-target esp32s3
   idf.py build
   ```
3. Build and start the app from the repository folder. It embeds the firmware from step 2, and a tray icon appears.
   ```
   cargo run --release -p kvmate-hub
   ```
4. Connect the board's **“COM”** port to the PC and its **“USB”** port to the other computer (or its dock).
5. Open the window (click the icon). The board's section shows a port without firmware → **“Install kvmate firmware”**. After a dozen or so seconds the board connects.
6. Pick the devices in “Headphones” and “Microphone”, the shortcut in “Keyboard and mouse” and autostart in “This computer”.
7. Optionally set up Wi-Fi in “Board connection”; then the board doesn't need a cable to the PC.

The app is a single file, `target/release/kvmate.exe`, with the window and the firmware inside; it only needs WebView2, which is part of Windows 11. Settings and the log are in `%APPDATA%\dev.pawelduraj.kvmate`. “Remove kvmate firmware” wipes the board so it can be used for something else.

## Limitations

- **Ctrl+Alt+Del and Win+L** always act on the PC: Windows doesn't let any app capture them. While you control the other computer, Ctrl+Alt+End works there as Ctrl+Alt+Del, Win+L locks the PC and brings the keyboard and mouse back to it, and both shortcuts can be sent from the tray menu.
- While the PC shows a UAC prompt, the lock screen or the Ctrl+Alt+Del screen, the keyboard and mouse stay on the PC.
- The app runs on Windows only for now: capturing the keyboard and mouse on Linux and macOS isn't written yet.

## Development

The project has three parts:

1. **`crates/protocol`**: the frame protocol shared by the app and the firmware (Rust, `no_std`). The firmware's C header is generated from it (`cargo xtask gen-header`) and committed, so building the firmware needs no Rust.
2. **`firmware/`**: ESP-IDF builds it into `firmware/build`.
3. **`crates/hub`**: the app (Tauri 2, plain HTML/CSS/JS, no Node.js). Its build script (`crates/hub/build.rs`) merges the firmware's bootloader, partition table and app from `firmware/build` into one image and embeds it in the exe. So the app carries the firmware built last; without one it still compiles but can't install firmware. `KVMATE_FIRMWARE_BIN` points it at a ready image instead.

App and core:

```
cargo test                              # protocol + core, with a simulated board
cargo run -p kvmate-hub -- --demo       # the app with a simulated board, no hardware needed
cargo run -p kvmate-hub                 # the app, finds the board by itself
cargo run -p kvmate-hub -- --port COM5  # a specific port
cargo xtask gen-header                  # after a change in crates/protocol or the version
cargo xtask check-header                # checks that the generated C files are up to date
cargo xtask gen-icons                   # after a change in xtask/src/icons.rs (the app icon)
cargo xtask check-icons                 # checks that crates/hub/icons is up to date
```

`cargo xtask` is an alias (`.cargo/config.toml`) for `cargo run -p xtask --`. Before committing: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.

Firmware, in a terminal with ESP-IDF loaded:

```
cd firmware
idf.py build                   # the app embeds this firmware on its next build
idf.py -p COM5 flash           # flashing without the app (the board's “COM” port)
```

The firmware has no text console, because UART0 carries the protocol. The board's log is shown in the app instead, under “Log” in the board's firmware section.

## Layout

```
crates/protocol/   frame protocol (Rust, no_std): the source of truth, the C header is generated from it
crates/core/       everything but the GUI: link to the board (cable, Wi-Fi), switching, input capture, sound, simulated board
crates/hub/        the app (Tauri): tray icon, panel and window in plain HTML/CSS/JS
firmware/          ESP32-S3 firmware (C, ESP-IDF, TinyUSB)
xtask/             project scripts: generated C files, app icons, release builds
```
