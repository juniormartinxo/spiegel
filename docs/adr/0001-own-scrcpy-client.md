# Spiegel is its own scrcpy client, not a wrapper around scrcpy.exe

Spiegel pushes the official `scrcpy-server` to the Device over adb and speaks the scrcpy client/server protocol itself: it decodes the video, renders it inside Spiegel's own window, forwards keyboard and mouse input, and feeds the same frames to the Virtual webcam. We chose this over launching `scrcpy.exe` because the two headline requirements, the phone's image embedded in Spiegel's window and a Virtual webcam on Windows, both need the decoded frames in Spiegel's process, and `scrcpy.exe` exposes neither (it renders into its own SDL window, and its only webcam output is V4L2 on Linux).

## Consequences

- The protocol is internal to scrcpy and the server refuses a client of any other version (`doc/develop.md`, "Protocol"). Spiegel bundles one `scrcpy-server` build, pinned to the version it implements (v4.1 at the start), and upgrading scrcpy means porting protocol changes.
- A user-installed scrcpy is never used. Only adb is configurable: bundled by default, with an optional custom path.

## Considered options

- **Wrap `scrcpy.exe`**: cheapest, but embedding its SDL window needs per-OS window-reparenting hacks and gives no access to frames for the Virtual webcam.
- **Hybrid** (wrap for mirroring, own client for the webcam): two implementations of the same job.
