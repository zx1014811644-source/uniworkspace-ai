import { invoke } from "@tauri-apps/api/core";

export const COMFYUI_URL = "http://127.0.0.1:8188/";

export type A11yChoice = "pending" | "granted" | "webOnly";

export interface DetectedDesktop {
  appId: string;
  name: string;
  bundleId: string;
  processName: string;
  pid: number;
}

export interface ListedWindow {
  windowId: number;
  pid: number;
  ownerName: string;
  title: string;
  bundleId: string | null;
  catalogAppId: string | null;
  label: string;
}

export interface ServiceStatus {
  id: string;
  name: string;
  port: number;
  address: string;
  running: boolean;
  openUrl: string | null;
}

export interface DesktopScan {
  intervalMs: number;
  platform: string;
  trusted: boolean;
  apps: DetectedDesktop[];
  services: ServiceStatus[];
  windows: ListedWindow[];
  windowNote: string | null;
}

export interface A11yStatus {
  trusted: boolean;
  choice: A11yChoice;
  platform: string;
  blocksWorkbench: boolean;
}

export function scanWait(intervalMs: number): number {
  if (intervalMs >= 250 && intervalMs <= 1000) {
    return intervalMs;
  }
  return 1000;
}

export function comfyOpenUrl(service: ServiceStatus): string | null {
  if (!service.running || service.openUrl !== COMFYUI_URL) {
    return null;
  }
  return COMFYUI_URL;
}

export async function desktopScan(): Promise<DesktopScan> {
  return invoke<DesktopScan>("desktop_scan");
}

export async function a11yStatus(): Promise<A11yStatus> {
  return invoke<A11yStatus>("a11y_status");
}

export async function a11yRequest(): Promise<A11yStatus> {
  return invoke<A11yStatus>("a11y_request");
}

export async function a11yOpenSettings(): Promise<void> {
  await invoke("a11y_open_settings");
}

export async function a11yChoose(choice: A11yChoice): Promise<A11yStatus> {
  return invoke<A11yStatus>("a11y_choose", { choice });
}
