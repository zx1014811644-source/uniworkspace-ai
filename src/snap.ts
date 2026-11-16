import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/// Must match `SNAP_TARGET_ID` in `src-tauri/src/snap.rs`.
export const SNAP_TARGET_ID = "claude-desktop";

export type SnapMode = "inactive" | "embedded" | "sideBySide";

export interface SnapFrame {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface SnapStatus {
  targetAppId: string;
  targetName: string;
  targetBundleId: string;
  active: boolean;
  mode: SnapMode;
  message: string | null;
}

export function framesDiffer(left: SnapFrame, right: SnapFrame): boolean {
  return (
    Math.abs(left.x - right.x) > 0.5 ||
    Math.abs(left.y - right.y) > 0.5 ||
    Math.abs(left.width - right.width) > 0.5 ||
    Math.abs(left.height - right.height) > 0.5
  );
}

export async function snapAttach(appId: string, frame: SnapFrame): Promise<SnapStatus> {
  return invoke<SnapStatus>("snap_attach", { appId, frame });
}

export async function snapUpdate(frame: SnapFrame): Promise<SnapStatus> {
  return invoke<SnapStatus>("snap_update", { frame });
}

export async function snapDetach(): Promise<SnapStatus> {
  return invoke<SnapStatus>("snap_detach");
}

export async function listenForSnapEnded(
  onEnded: (status: SnapStatus) => void,
): Promise<UnlistenFn> {
  return listen<SnapStatus>("snap-ended", (event) => {
    onEnded(event.payload);
  });
}
