# Agent instructions

This file provides guidance to AI coding agents working with code in this repository. `AGENTS.md` is a symlink to `CLAUDE.md`; edit `CLAUDE.md`.

## Project

Spiegel is a graphical front-end for [scrcpy](https://github.com/Genymobile/scrcpy) (Android screen mirroring/control over adb). The repo is at its initial stage — no source code, build system or tests exist yet. The GUI tech stack has not been decided; update this file with build/run/test commands once it is.

## scrcpy reference

A local clone of scrcpy lives at `D:\apps\community\scrcpy` (tag v4.1). Use it as the source of truth for scrcpy behavior instead of guessing. Useful entry points:

- `app/src/cli.c` — the `options[]` table (~108 long options) with each flag's name, argument and help text. This is the canonical list of what the GUI can expose.
- `app/src/options.h` / `options.c` — the `scrcpy_options` struct and defaults behind those flags.
- `doc/*.md` — user docs per feature area (video, audio, control, keyboard, mouse, camera, virtual-display, recording, connection, tunnels, otg, window, shortcuts, windows).
- `app/data/bash-completion`, `app/data/zsh-completion` — compact option lists, handy for cross-checking.

Facts relevant to wrapping scrcpy:

- scrcpy is a C/SDL client plus a Java server (`scrcpy-server`) that it pushes to `/data/local/tmp/scrcpy-server.jar` on the device and runs via adb.
- The client finds adb via the `ADB` env var and the server file via `SCRCPY_SERVER_PATH` (see `app/src/adb/adb.c`, `app/src/server.c`).
- Discovery options that print info and exit — useful for populating GUI pickers: `--list-displays`, `--list-cameras`, `--list-camera-sizes`, `--list-encoders`, `--list-apps`. Device selection is `--serial` / `-s`; wireless setup is `--tcpip` (default adb port 5555).
- scrcpy renders into its own SDL window; `--window-title` and `--no-window` control that.

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues on juniormartinxo/spiegel, using the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Uses the five default triage labels: needs-triage, needs-info, ready-for-agent, ready-for-human, wontfix. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
