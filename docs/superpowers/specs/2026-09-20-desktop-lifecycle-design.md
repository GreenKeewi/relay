# Relay Desktop Lifecycle and Branding Design

## Goal

Make Relay behave like a polished Windows application: it has a branded taskbar presence, offers an explicit full-app close action, can start quietly with Windows, reveals itself when a supported coding agent becomes active, and preserves the existing four-wall dial snapping behavior.

## User Experience

- The main Relay window appears in the Windows taskbar whenever it is visible. The compact dock window stays out of the taskbar.
- Relay's existing octopus artwork is used for the packaged Windows icon and as a small mark beside the `Relay` wordmark.
- The title bar contains separate minimize-to-dock and close controls. Close exits the complete Relay process, including both native windows.
- Settings contains an operating-system-backed `Launch Relay at startup` toggle. When enabled, Windows launches Relay with a background argument.
- A background launch creates no visible window. When Claude Code becomes newly active, Relay reveals and focuses the main window.
- After the user closes Relay, agent monitoring stops until Relay is opened manually or Windows launches it at the next login.
- Launching Relay while a background instance is already running focuses that existing instance instead of creating a duplicate.

## Native Architecture

Tauri's official autostart plugin owns the Windows startup entry and passes `--background` on automatic launches. Tauri's single-instance plugin is registered first and forwards later launches to the existing instance, where Relay reveals and focuses its main window.

The configured main window starts hidden. Native setup positions both windows, detects whether the process received `--background`, and reveals the main window only for a normal launch. The dock remains hidden until the user collapses Relay.

A shared native reveal operation is used by normal launch, second-instance activation, agent activation, and the existing expand action. A `quit_relay` command calls the application-level exit API so the hidden dock cannot keep the process alive.

## Supported-Agent Detection

Relay adds a small native provider registry. Each provider supplies an identifier and a read-only activity matcher. Version one registers only Claude Code; later providers add registry entries without changing the watcher lifecycle.

The watcher periodically samples local processes and detects inactive-to-active transitions. Claude matching checks process executable and command-line evidence because Claude Code may run through a Node or command-wrapper process. It does not store, log, or send command lines. A transition reveals Relay once; continued activity does not repeatedly steal focus. The transition resets only after Claude activity disappears.

The watcher runs while Relay is alive, including background startup. Polling is deliberately low-frequency to keep overhead negligible. Detection failures are ignored for that sample and do not terminate Relay.

## Frontend and Settings

The app header gains a reusable Relay brand mark and an accessible `Close Relay` button. The window-action group preserves the existing collapse-to-dock control.

The startup toggle queries the real operating-system state through the Tauri plugin. While a change is pending it is disabled. After enable or disable completes, the UI re-reads the OS state. Failures leave the previous value intact and show a concise settings error rather than pretending the change succeeded.

Browser-only development receives safe fallbacks so the React surface remains usable without native APIs.

## Branding and Packaging

The approved octopus concept is converted into Tauri's standard multi-size icon set, including a multi-resolution Windows ICO. Bundle configuration references the generated assets explicitly. A web-sized copy is bundled through Vite for the header mark.

The main window has taskbar participation enabled; the dock window remains excluded. Build hooks are aligned with the repository's declared pnpm package manager.

## Existing Dial Behavior

The committed implementation already snaps the dial to the closest of the top, right, bottom, or left monitor walls after a drag. This work preserves that implementation and runs its frontend and Rust coverage. No unrelated geometry redesign is included.

## Testing and Acceptance

Automated coverage will verify:

- startup toggle state, pending behavior, success, and failure;
- app-header logo and close action;
- background argument parsing and reveal decisions;
- Claude process matching and one-shot inactive-to-active transitions;
- application-level quit command registration;
- the existing dock drag and all-four-wall snap behavior.

Manual packaged-app checks on Windows will verify:

- the octopus icon appears in the executable, taskbar, and installed shortcut;
- normal launches show Relay and background startup does not flash a window;
- enabling and disabling startup updates Windows behavior;
- opening Claude Code reveals the background Relay instance;
- a second Relay launch focuses the first instance;
- Close exits Relay completely.

The final verification includes frontend tests, TypeScript checks, production web build, Rust tests, and a Tauri package build.
