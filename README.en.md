<div align="center">

# Spiegel

**Your Android phone, on your desktop: mirror it, control it, and use its camera as a webcam.**

A friendly graphical front-end for [scrcpy](https://github.com/Genymobile/scrcpy). Everything you would do with scrcpy on the command line, done from one good-looking app.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Status: early development](https://img.shields.io/badge/status-early%20development-orange.svg)
![Platform: Windows first](https://img.shields.io/badge/platform-Windows%20first-lightgrey.svg)

[Português (Brasil)](README.md) · **English**

</div>

> [!NOTE]
> Spiegel is in early development. The design is settled and the riskiest piece, the Windows 11 virtual webcam, has been proven by a prototype, but there is no usable app yet. Star or watch the repo to follow along.

*Spiegel* is German for "mirror".

## Why

scrcpy is fast, light and excellent, but it is a command-line tool with about 108 options. Spiegel puts a real interface on top of it and adds what scrcpy cannot do on its own on Windows:

- **📷 Your phone as a webcam.** Use the phone's rear or front camera in OBS, Zoom, Google Meet, Slack, Chrome and any other app that takes a webcam. Record videos for YouTube with a far better camera than your laptop's.
- **🖱️ Full control from your keyboard and mouse.** The phone's screen shows inside Spiegel's window and you drive it from the computer. It is a lifesaver when the phone's touchscreen is cracked or dead.
- **📱 Several devices and sessions at once.** For example, mirror one phone's screen while its rear camera feeds the webcam.
- **💾 Profiles.** Save settings under a name ("Rear camera webcam 1080p", "Remote control") and start a session in one click. Built-in profiles cover the common cases.
- **📶 Wireless setup by QR code.** Pair over Wi-Fi without a cable (Android 11+), or switch a USB-connected phone to Wi-Fi.
- **Nothing to install on the phone.** Like scrcpy, Spiegel only needs USB debugging turned on.

## How it works

Spiegel is its own scrcpy client. It pushes the official `scrcpy-server` to the phone over adb and talks to it directly, instead of launching `scrcpy.exe`. That is what lets it show the phone inside its own window and feed the same video to a virtual webcam.

```
Phone                         Computer
─────                         ──────────────────────────────────────────────────
Camera / Screen               Spiegel (Rust core)
  └─► scrcpy-server ──USB/Wi-Fi──► encoded video
      (encodes H.264/H.265)          ├─► FFmpeg decode ─► shared memory ─► Virtual webcam ─► OBS, Zoom, Meet…
                                     └─► Spiegel window (WebCodecs) ─► live preview + keyboard/mouse control
```

| Piece | Technology |
|---|---|
| App shell and UI | [Tauri 2](https://tauri.app), TypeScript web UI |
| Core: adb, scrcpy protocol, sessions | Rust |
| Video decoding | WebCodecs for the preview, FFmpeg for the webcam |
| Virtual webcam | C++ COM DLL: Media Foundation on Windows 11, DirectShow on Windows 10 |
| Phone side | Official `scrcpy-server`, pinned to v4.1 |

The reasoning behind each choice is recorded in [`docs/adr/`](docs/adr) (in Portuguese).

## Roadmap

- [x] **Design**: domain model and architecture decisions
- [x] **Virtual webcam prototype (Windows 11)**: ~1 ms from Spiegel to the consuming app. It works in OBS, Zoom, Google Meet (including the remote side of a call), Slack, Chrome, Edge and the Windows Camera app.
- [ ] **Screen + remote control over USB**: the phone inside Spiegel's window, driven by keyboard and mouse
- [ ] **Phone camera, recorded and as a webcam**: record straight to the computer's disk with no re-encoding (up to 4K when the phone can), or feed the virtual webcam at 720p/1080p; rear or front camera, fps, zoom, torch
- [ ] **Profiles, QR pairing, audio, installer, system tray**
- [ ] **Languages**: English and Portuguese first, open to more
- [ ] **Linux and macOS**

Later ideas include a virtual microphone, a guided rescue mode for phones whose screen is dead and USB debugging is not yet authorized, virtual displays and gamepad support.

## Requirements (planned)

- **Windows 11** for the full feature set. Windows 10 is supported with a DirectShow webcam, which apps that only use Media Foundation, such as the Windows Camera app, cannot see.
- An Android device with **USB debugging** enabled and this computer authorized. The camera needs Android 12 or later, as in scrcpy.
- Administrator rights **only once**, the first time you turn on the virtual webcam. Everything else installs per user.

Windows allows only one app at a time to use a virtual camera.

## Project docs

The project's docs are written in Brazilian Portuguese:

- [`CONTEXT.md`](CONTEXT.md): the project's vocabulary (Device, Session, Profile, Virtual webcam…)
- [`docs/adr/`](docs/adr): architecture decision records
- [`CLAUDE.md`](CLAUDE.md): notes for AI coding agents working on the repo

## Acknowledgements

Spiegel stands on the shoulders of [scrcpy](https://github.com/Genymobile/scrcpy) by Genymobile and Romain Vimont, licensed under Apache 2.0. Spiegel is an independent project, not affiliated with Genymobile.

## License

[MIT](LICENSE) © 2026 Junior Martins
