# Relay Desktop Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship Relay as a branded Windows taskbar application that can start quietly with Windows, reveal itself when Claude Code starts, and close completely on demand.

**Architecture:** Tauri owns startup registration, single-instance activation, native window lifecycle, process observation, and whole-app exit. React owns the branded title bar and an OS-backed startup preference UI. A provider-oriented native activity module implements Claude Code now and exposes a narrow boundary for future agent matchers.

**Tech Stack:** Tauri 2, Rust, React 19, TypeScript, Vitest, pnpm, official Tauri autostart and single-instance plugins, sysinfo.

**Spec:** `docs/superpowers/specs/2026-09-20-desktop-lifecycle-design.md`

## Global Constraints

- Preserve the committed four-wall dial snapping implementation and its tests.
- Preserve unrelated uncommitted dock-shape changes in `src/components/DockView.tsx` and `src/styles.css`.
- The dock window never appears in the taskbar.
- Closing Relay exits the complete process and stops agent monitoring until a manual launch or the next enabled Windows startup.
- Process command lines are inspected only in memory and are never persisted, logged, or transmitted.
- Version one auto-detects Claude Code only; future providers extend the registry rather than the watcher lifecycle.
- Browser-only Vite development must remain usable when Tauri APIs are unavailable.

---

### Task 1: Native startup, singleton, agent activity, and whole-app lifecycle

**Files:**
- Create: `src-tauri/src/agent_activity.rs`
- Create: `src/startup.ts`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `package.json`
- Modify: `pnpm-lock.yaml`
- Modify: `src/components/tauriBridge.ts`

**Interfaces:**
- Produces: `readLaunchAtStartup(): Promise<boolean>` and `writeLaunchAtStartup(enabled: boolean): Promise<boolean>` from `src/startup.ts`.
- Produces: `quitRelay(): Promise<void>` from `src/components/tauriBridge.ts`.
- Produces: `agent_activity::start(app: AppHandle)` and pure Claude matching/transition logic covered by Rust unit tests.
- Produces: native `reveal_relay(app: AppHandle)` behavior used by normal launch, single-instance launch, and agent activation.

- [ ] **Step 1: Add failing Rust tests for argument and agent-transition behavior**

Add pure tests proving `is_background_launch(["relay.exe", "--background"])`, Claude command matching for direct `claude`, `claude.cmd`, and Node command lines containing the Claude CLI entrypoint, rejection of unrelated Node processes, and exactly one reveal on an inactive-to-active transition until activity clears.

```rust
#[test]
fn claude_transition_reveals_only_on_rising_edge() {
    let mut state = ActivityState::default();
    assert!(state.observe(true));
    assert!(!state.observe(true));
    assert!(!state.observe(false));
    assert!(state.observe(true));
}
```

- [ ] **Step 2: Run the focused native test and confirm failure**

Run: `cargo test agent_activity --manifest-path src-tauri/Cargo.toml`

Expected: FAIL because `agent_activity` and its matcher/state types do not exist.

- [ ] **Step 3: Implement the provider activity module**

Create a focused module with an `ActivityState { active: bool }`, a pure `matches_claude_process(name: &OsStr, command: &[OsString]) -> bool`, a small provider registry containing Claude Code, and a low-frequency background loop using `sysinfo`. Refresh processes, derive provider activity, and invoke a supplied reveal callback only on a rising edge. Never print or retain process command lines.

```rust
impl ActivityState {
    pub fn observe(&mut self, active: bool) -> bool {
        let reveal = active && !self.active;
        self.active = active;
        reveal
    }
}
```

- [ ] **Step 4: Add official startup and single-instance integrations**

Add `tauri-plugin-autostart`, `tauri-plugin-single-instance`, and `sysinfo` Rust dependencies; add `@tauri-apps/plugin-autostart` to the web package; grant `autostart:allow-enable`, `autostart:allow-disable`, and `autostart:allow-is-enabled`. Register single-instance first, autostart with `--background`, and make later launches call the shared reveal operation.

```rust
.plugin(tauri_plugin_single_instance::init(|app, _, _| {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { let _ = reveal_relay(app).await; });
}))
.plugin(tauri_plugin_autostart::Builder::new().arg("--background").build())
```

- [ ] **Step 5: Implement native reveal and quit behavior**

Factor a shared async reveal operation that expands from dock mode or shows, unminimizes, and focuses the main window in widget/background mode. During setup, show Relay only when `--background` is absent, then start the agent watcher. Register `quit_relay` and exit through `AppHandle::exit(0)`.

```rust
#[tauri::command]
fn quit_relay(app: AppHandle) {
    app.exit(0);
}
```

- [ ] **Step 6: Implement the TypeScript startup and exit bridges**

Use `isTauri()` to return `false` safely in browser development. In Tauri, call the official plugin and always re-read the OS state after a mutation.

```ts
export async function writeLaunchAtStartup(enabled: boolean) {
  if (!isTauri()) return false;
  await (enabled ? enable() : disable());
  return isEnabled();
}
```

- [ ] **Step 7: Run native and TypeScript checks**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`

Expected: PASS, including matcher and transition tests.

Run: `pnpm typecheck`

Expected: PASS with startup and quit bridge exports available to the frontend task.

- [ ] **Step 8: Commit the native lifecycle slice**

```bash
git add src-tauri/src/agent_activity.rs src-tauri/src/lib.rs src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/capabilities/default.json src/startup.ts src/components/tauriBridge.ts package.json pnpm-lock.yaml
git commit -m "feat: add Relay startup and agent activation lifecycle"
```

---

### Task 2: Branded window controls and startup settings

**Files:**
- Create: `src/components/RelayTitleBar.tsx`
- Create: `src/components/RelayTitleBar.test.tsx`
- Modify: `src/RelayApp.tsx`
- Modify: `src/components/SecondaryViews.tsx`
- Modify: `src/components/SecondaryViews.test.tsx`
- Modify: `src/styles.css`

**Interfaces:**
- Consumes: `readLaunchAtStartup()` and `writeLaunchAtStartup(enabled)` from `src/startup.ts`.
- Consumes: `quitRelay()` and existing `invokeWindowCommand(...)` from `src/components/tauriBridge.ts`.
- Consumes: `src/assets/relay-logo.png`, produced by Task 3.
- Produces: `RelayTitleBar` with accessible `Collapse Relay to dock` and `Close Relay` controls.
- Produces: startup props on `SettingsView`: `launchAtStartup`, `isStartupPending`, `startupError`, and `onLaunchAtStartupChange`.

- [ ] **Step 1: Write failing title-bar and settings tests**

Test that the title bar renders the octopus mark beside `Relay`, invokes its dock handler, and invokes its close handler. Extend Settings tests to assert the startup button's `aria-pressed`, pending disabled state, callback value, and visible error message.

```tsx
fireEvent.click(screen.getByRole("button", { name: "Close Relay" }));
expect(onClose).toHaveBeenCalledOnce();
```

- [ ] **Step 2: Run focused frontend tests and confirm failure**

Run: `pnpm test -- src/components/RelayTitleBar.test.tsx src/components/SecondaryViews.test.tsx`

Expected: FAIL because the title bar and startup settings props do not exist.

- [ ] **Step 3: Implement the title bar and integrate it into RelayApp**

Move the existing wordmark and collapse button into `RelayTitleBar`, add the image mark and a Phosphor `X` close button, and pass callbacks from RelayApp. The close callback invokes the native whole-app command; its browser fallback remains harmless.

```tsx
<RelayTitleBar
  onDock={() => invokeWindowCommand("collapse_to_dock", "relay:collapse-to-dock")}
  onClose={() => { void quitRelay(); }}
/>
```

- [ ] **Step 4: Add OS-backed startup state to RelayApp**

On mount, query `readLaunchAtStartup`. For changes, set pending, clear the prior error, call `writeLaunchAtStartup`, store the returned OS truth, catch a concise error, and clear pending in `finally`. Pass this state to `SettingsView`.

```ts
const changeLaunchAtStartup = async (enabled: boolean) => {
  setIsStartupPending(true);
  setStartupError(undefined);
  try { setLaunchAtStartup(await writeLaunchAtStartup(enabled)); }
  catch { setStartupError("Relay could not update Windows startup settings."); }
  finally { setIsStartupPending(false); }
};
```

- [ ] **Step 5: Add the startup setting and styles**

Add a button labeled `Launch Relay at startup` with copy explaining that Relay starts quietly and opens when a supported agent becomes active. Use the existing toggle visual language and display the error with `role="alert"`. Update the topbar to contain a left brand group, centered tabs, and a right window-action group. Preserve all current uncommitted `.dock-*` CSS.

- [ ] **Step 6: Run focused and complete frontend checks**

Run: `pnpm test -- src/components/RelayTitleBar.test.tsx src/components/SecondaryViews.test.tsx`

Expected: PASS.

Run: `pnpm test`

Expected: PASS, including existing dock snapping tests.

Run: `pnpm typecheck`

Expected: PASS.

- [ ] **Step 7: Commit the frontend slice without swallowing unrelated edits**

Before committing, inspect `git diff -- src/styles.css src/components/DockView.tsx`. Preserve the pre-existing dock-surface changes verbatim; stage the intended title-bar/settings hunks and all owned files.

```bash
git add src/RelayApp.tsx src/components/RelayTitleBar.tsx src/components/RelayTitleBar.test.tsx src/components/SecondaryViews.tsx src/components/SecondaryViews.test.tsx src/styles.css
git commit -m "feat: add branded Relay window controls and startup setting"
```

---

### Task 3: Windows app branding, taskbar configuration, and package validation

**Files:**
- Create: `src/assets/relay-logo.png`
- Modify/Create generated files under: `src-tauri/icons/`
- Modify: `src-tauri/tauri.conf.json`

**Interfaces:**
- Produces: `src/assets/relay-logo.png` consumed by `RelayTitleBar`.
- Produces: Tauri standard icon files including `icons/icon.ico`, `icons/32x32.png`, `icons/128x128.png`, and `icons/128x128@2x.png`.
- Configures: main window `visible: false` and `skipTaskbar: false`; dock window remains `visible: false` and `skipTaskbar: true`.

- [ ] **Step 1: Generate the standard icon set from the approved art**

Run: `pnpm tauri icon design/brand/relay-octopus-concept-v1.png`

Expected: Tauri replaces the placeholder icon set with multi-size branded files. Copy the approved PNG to `src/assets/relay-logo.png` for Vite using a filesystem copy that preserves the source.

- [ ] **Step 2: Update Tauri window and bundle configuration**

Set the main window to start hidden so background autostart never flashes, enable its taskbar presence, retain dock taskbar exclusion, list generated bundle icons explicitly, and change `beforeDevCommand`/`beforeBuildCommand` from Bun to pnpm.

```json
"bundle": {
  "active": true,
  "targets": "all",
  "icon": ["icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png", "icons/icon.ico"]
}
```

- [ ] **Step 3: Validate configuration and the production web build**

Run: `pnpm build`

Expected: PASS and the Vite output contains the branded header asset.

Run: `pnpm tauri build --debug`

Expected: PASS and emits a Windows executable/bundle carrying the Relay icon.

- [ ] **Step 4: Commit the packaging slice**

```bash
git add src/assets/relay-logo.png src-tauri/icons src-tauri/tauri.conf.json
git commit -m "feat: brand Relay Windows packaging"
```

---

### Task 4: Integrated review and acceptance

**Files:**
- Modify only files required to fix verified integration defects from Tasks 1-3.

**Interfaces:**
- Consumes every interface and artifact produced by Tasks 1-3.
- Produces a clean, buildable integrated application without regressing dial snapping or the uncommitted dock-surface work.

- [ ] **Step 1: Review the combined diff against the specification**

Confirm taskbar visibility applies only to `main`; background startup is hidden; all reveal paths converge; close exits the process; startup UI reads OS truth; command lines are never logged; the dock snap implementation remains intact.

- [ ] **Step 2: Run all automated verification**

Run: `pnpm test`

Run: `pnpm typecheck`

Run: `pnpm build`

Run: `cargo test --manifest-path src-tauri/Cargo.toml`

Expected: every command passes.

- [ ] **Step 3: Build the complete desktop app**

Run: `pnpm tauri build --debug`

Expected: PASS with a Windows executable and installer artifacts.

- [ ] **Step 4: Record any environment-only manual checks**

If Windows login restart or external Claude activation cannot be exercised automatically, report those exact remaining manual checks without claiming they passed. All behavior that can be isolated must remain covered by unit tests.

- [ ] **Step 5: Commit integration fixes if any**

```bash
git add src/RelayApp.tsx src/startup.ts src/components/RelayTitleBar.tsx src/components/SecondaryViews.tsx src/components/tauriBridge.ts src/styles.css src-tauri/src/agent_activity.rs src-tauri/src/lib.rs src-tauri/tauri.conf.json src-tauri/capabilities/default.json
git commit -m "fix: complete Relay desktop lifecycle integration"
```
