import { invoke } from "@tauri-apps/api/core";
import { runningInTauri } from "./window";
import type { CatalogApp } from "./types";

export const WEEK2_IDS = ["chatgpt", "claude", "gemini", "kimi"] as const;

export interface PartitionBounds {
  appId: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface ProbeSample {
  appId: string;
  identifier: string;
  localStorage: string;
  cookie: string;
}

export interface ProbeReport {
  isolated: boolean;
  backend: string;
  detail: string;
  hints: string[];
  samples: ProbeSample[];
}

export function isWeek2Web(app: CatalogApp): app is CatalogApp & { webUrl: string } {
  return (
    app.kind === "web" &&
    (WEEK2_IDS as readonly string[]).includes(app.id) &&
    typeof app.webUrl === "string"
  );
}

export function errorText(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "打不开这个分区";
}

export async function showPartition(input: PartitionBounds & { url: string }): Promise<void> {
  if (!runningInTauri()) {
    return;
  }
  await invoke("show_partition", {
    appId: input.appId,
    url: input.url,
    x: input.x,
    y: input.y,
    width: input.width,
    height: input.height,
  });
}

export async function setPartitionBounds(bounds: PartitionBounds): Promise<void> {
  if (!runningInTauri()) {
    return;
  }
  await invoke("set_partition_bounds", {
    appId: bounds.appId,
    x: bounds.x,
    y: bounds.y,
    width: bounds.width,
    height: bounds.height,
  });
}

export async function hidePartitions(): Promise<void> {
  if (!runningInTauri()) {
    return;
  }
  await invoke("hide_partitions");
}

export async function probePartitions(): Promise<ProbeReport> {
  return invoke<ProbeReport>("probe_partitions");
}
