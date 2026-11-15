import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

export function runningInTauri(): boolean {
  return isTauri();
}

export async function minimizeWindow(): Promise<void> {
  if (!isTauri()) {
    return;
  }
  await getCurrentWindow().minimize();
}

export async function toggleMaximizeWindow(): Promise<void> {
  if (!isTauri()) {
    return;
  }
  await getCurrentWindow().toggleMaximize();
}

export async function closeWindow(): Promise<void> {
  if (!isTauri()) {
    return;
  }
  await getCurrentWindow().close();
}
