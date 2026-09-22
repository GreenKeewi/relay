# Contributing to Relay

Thanks for considering a contribution. Relay is a small, local-first Windows widget, and the codebase is intentionally minimal — please keep changes scoped and avoid speculative abstractions.

## Setup

```sh
pnpm install
pnpm tauri dev   # full desktop app
pnpm dev         # Vite-only browser surface, no native commands
```

Requirements: [pnpm](https://pnpm.io/) 11.19+, Node.js 22+, Rust, and the [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/).

## Before opening a PR

```sh
pnpm test        # Vitest (jsdom)
pnpm typecheck   # tsc -b
cd src-tauri && cargo test
```

Add or update tests when you change parsing, validation, or lifecycle logic — Rust tests in particular cover privacy and input-validation boundaries (`src-tauri/src/lib.rs`, `src-tauri/src/claude.rs`) and must keep passing.

## Guidelines

- No telemetry, no uploads, no new fields that carry prompt/response/tool content, session IDs, paths, or credentials into the usage snapshot or session model. See the "Security invariants" section of `CLAUDE.md`/`AGENTS.md`.
- New native commands need: a `#[tauri::command]` in `lib.rs`, registration in `generate_handler!`, a capability entry under `src-tauri/capabilities/`, and a typed wrapper in `src/claude.ts` or `src/components/tauriBridge.ts`.
- Keep PRs focused — one behavior change per PR is easier to review than a bundle of unrelated fixes.

## Reporting issues

Open a GitHub issue with repro steps. For anything touching the security invariants above (input validation, credential handling, telemetry), please flag it clearly in the title.
