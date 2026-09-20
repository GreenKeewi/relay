import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getSafeSourceTarget, type RelaySession } from "../model/session";

function prefersReducedMotion() {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export type DockEdge = "left" | "right";

type DockPlacement = {
  edge: DockEdge;
  x: number;
  y: number;
};

export async function snapDockToNearestEdge() {
  const placement = await invoke<DockPlacement>("snap_dock_to_nearest_edge");
  return placement.edge;
}

export async function dragDockToNearestEdge() {
  await getCurrentWindow().startDragging();
  return snapDockToNearestEdge();
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
