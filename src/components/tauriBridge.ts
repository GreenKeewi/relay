import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getSafeSourceTarget, type RelaySession } from "../model/session";

function prefersReducedMotion() {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export type DockEdge = "top" | "right" | "bottom" | "left";

export type DockPlacement = {
  edge: DockEdge;
  x: number;
  y: number;
};

export function snapDockToNearestEdge() {
  return invoke<DockPlacement>("snap_dock_to_nearest_edge");
}

export async function dragDockToNearestEdge() {
  await invoke("prepare_dock_drag");
  await getCurrentWindow().startDragging();
  return snapDockToNearestEdge();
}

export function openDockMenu() {
  return invoke<DockPlacement>("open_dock_menu");
}

export function closeDockMenu() {
  return invoke<DockPlacement>("close_dock_menu");
}

export function hideDockFor(durationSeconds: 3_600 | 86_400) {
  return invoke("hide_dock_for", { durationSeconds });
}

export function invokeWindowCommand(
  command: "collapse_to_dock" | "expand_from_dock",
  fallbackEvent: "relay:collapse-to-dock" | "relay:expand-from-dock",
) {
  void invoke(command, { reducedMotion: prefersReducedMotion() }).catch(() => {
    window.dispatchEvent(new CustomEvent(fallbackEvent));
  });
}

export function openRelaySession(session: RelaySession) {
  const target = getSafeSourceTarget(session.source);
  if (!target) {
    window.dispatchEvent(new CustomEvent("relay:invalid-session-source", { detail: session }));
    return;
  }

  void invoke("open_session", { source: target.value }).catch(() => {
    window.dispatchEvent(new CustomEvent("relay:open-session", { detail: session }));
  });
}
