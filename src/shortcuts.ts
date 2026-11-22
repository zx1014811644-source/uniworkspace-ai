import { invoke } from "@tauri-apps/api/core";

export interface ShortcutStatus {
  accelerator: string;
  registered: boolean;
  error: string | null;
  platform: string;
}

export async function shortcutStatus(): Promise<ShortcutStatus> {
  return invoke<ShortcutStatus>("shortcut_status");
}

export async function shortcutSet(accelerator: string): Promise<ShortcutStatus> {
  return invoke<ShortcutStatus>("shortcut_set", { accelerator });
}

export async function shortcutCapture(active: boolean): Promise<ShortcutStatus> {
  return invoke<ShortcutStatus>("shortcut_capture", { active });
}
