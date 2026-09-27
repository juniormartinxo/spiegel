# Rust + Tauri 2, with the preview decoded in the webview and the Virtual webcam decoded natively

Spiegel is a Tauri 2 app: a Rust core owns adb, the scrcpy protocol and the Sessions, and a TypeScript web UI renders everything the user sees. The video path is split so no raw frames ever cross the Tauri IPC. The core forwards the still-encoded stream (~1-2 MB/s) to the UI, which decodes it with WebCodecs for the in-window preview. When the Virtual webcam is fed, the core also decodes the same stream with FFmpeg and writes the frames to shared memory for the webcam DLL. We chose this because it keeps a web UI (the richest ecosystem for a polished interface) while staying portable and free of bottlenecks. WebCodecs is on by default in WebView2, WKWebView (Safari 16.4+) and WebKitGTK 2.44+, and raw 1080p30 frames (~90 MB/s) exceed what Tauri IPC was measured to carry on Windows (~50 MB/s).

## Consequences

- While the Virtual webcam is fed and the preview is visible, each frame is decoded twice (webview and core). Hardware decoders absorb this.
- The default video codec is H.264 or H.265: WKWebView does not enable AV1 decoding by default.
- The Virtual webcam DLL stays C++ (Windows COM), fed by the Rust core over shared memory.

## Considered options

- **Qt 6 (C++/QML)**: a single native frame path, but a weaker UI toolkit and the protocol written from scratch in C++.
- **Electron or Tauri with Tango (TypeScript scrcpy client) doing all the decoding in the webview**: the Virtual webcam would need raw frames copied out of the webview over IPC. Tango's scrcpy 4.x support was also still beta.
- **Native wgpu rendering under a transparent webview**: does not work on Linux Wayland.
- **Media Source Extensions with fragmented MP4**: no evidence of sub-100 ms latency.
