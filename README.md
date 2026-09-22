# Relay

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

Relay is a lightweight, local-first Windows widget for supervising concurrent AI coding sessions. It stays above the desktop as a compact session list and collapses into a small edge-docked status ring.

The current product slice connects to Claude Code. Codex, ChatGPT Work, and Zed remain represented in the provider model and visual assets, but are not active data sources yet.

## Download

Grab the latest Windows installer from the [Releases page](../../releases/latest) — no build tools required. Relay reads your existing local Claude Code metadata; nothing is uploaded anywhere.

## Why it's lightweight

Relay is a native Tauri 2 shell, not an Electron app: there's no bundled Chromium, it renders through the OS's already-installed WebView2 runtime, and the Rust backend does no polling beyond periodically re-reading local session files.

Measured live on a running instance (Windows 11, PowerShell `Get-Process`, timestamped 2026-09-22):

```
Get-Process relay | Select ProcessName, Id,
  @{N='WorkingSetMB';E={[math]::Round($_.WS/1MB,2)}},
  @{N='PrivateMB';E={[math]::Round($_.PrivateMemorySize64/1MB,2)}}

ProcessName   Id WorkingSetMB PrivateMB
-----------   -- ------------ ---------
relay       3824        52.89     16.74
```

The `relay.exe` process itself — Rust backend plus window management, excluding the shared system-wide WebView2 renderer host — holds roughly **15-17 MB of private (non-shared) memory** and ~50 MB of working set. WebView2 rendering happens in `msedgewebview2.exe` host processes that are shared across every WebView2-based app running on the machine (not exclusive to Relay), so they're not counted as Relay's own footprint. Run the command above yourself while Relay is open to reproduce this.

## What works

- Discovers real Claude Code sessions from local metadata under `~/.claude/projects`
- Shows safe session names, projects, agents/subagents, observable activity, and last-update times
- Filters by connected agent and lifecycle state: In progress, To review, Done, and Archived
- Supports review acknowledgement, local status changes, multi-select, archive, restore, and Relay-only deletion
- Resumes a selected Claude session in a visible terminal
- Displays verified five-hour and seven-day Claude account limits when Claude emits status-line rate-limit data
- Preserves the user's existing Claude status line through a privacy-safe forwarding bridge
- Collapses between a draggable main widget and an edge-aware draggable wall widget

Relay does not ship demo sessions in the active interface.

## Stack

- Tauri 2 and Rust for native windows, local discovery, safe process launching, and window transitions
- React 19 and TypeScript for the interface
- Vite for development and production bundling
- Vitest and Rust unit tests for behavior and privacy boundaries

## Prerequisites

- [pnpm](https://pnpm.io/) 11.19 or newer
- Node.js 22 or newer
- Rust and the [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/)
- Claude Code for the current provider integration

```sh
pnpm install
```

## Development

```sh
pnpm dev        # Browser-only Vite surface
pnpm tauri dev  # Complete desktop app
pnpm test       # Frontend and model tests
pnpm typecheck  # TypeScript project references
pnpm build      # Typecheck and production web bundle
```

Run the native test suite separately:

```sh
cd src-tauri
cargo test
```

## Project structure

```text
design/                     Brand exploration and approved visual references
src/
  assets/agent-logos/       Bundled provider marks
  components/               Widget, onboarding, usage, and session UI
  model/                    Provider-neutral session and lifecycle logic
  claude.ts                 Typed Tauri commands for Claude Code
  RelayApp.tsx              Application state and view composition
src-tauri/
  capabilities/             Tauri permissions
  src/claude.rs             Claude discovery, usage, and resume integration
  src/lib.rs                Window behavior and command registration
  tauri.conf.json           Desktop window and bundle configuration
```

## Claude usage integration

Claude Code passes documented `rate_limits.five_hour` and `rate_limits.seven_day` values to status-line commands after the first API response. Relay's optional bridge stores only the usage percentages, reset timestamps, and capture time, then forwards the original stdin bytes to the pre-existing status-line command unchanged.

The bridge:

- keeps the previous status-line command, including ccstatusline
- backs up the pre-Relay Claude settings once
- never stores the raw status-line payload
- never stores session IDs, prompts, responses, paths, transcripts, or credentials
- hides stale values instead of estimating account usage

## Privacy and safety

Relay reads local Claude metadata and keeps its own state on the device. It does not upload session data or add telemetry. The Claude transcript parser deliberately excludes prompt text, assistant responses, tool inputs, and tool outputs from its data model.

Session-opening inputs are validated before Tauri launches them. URLs must be credential-free HTTPS targets, local paths must be absolute existing directories, and Claude resume IDs must be canonical UUIDs passed as process arguments rather than interpolated into shell source.

## Contributing

Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for how to set up a dev environment and submit changes.

## License

Relay is licensed under the [Apache License 2.0](LICENSE).
