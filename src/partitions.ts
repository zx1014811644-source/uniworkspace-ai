import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { runningInTauri } from "./window";

export interface PaneSpec {
  appId: string;
  url: string;
  x: number;
  y: number;
  width: number;
  height: number;
  focus: boolean;
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

export interface PaneNotice {
  appId: string;
  message: string;
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

export async function syncPanes(panes: PaneSpec[]): Promise<void> {
  if (!runningInTauri()) {
    return;
  }
  await invoke("sync_panes", { panes });
}

export async function focusPartition(appId: string): Promise<void> {
  if (!runningInTauri()) {
    return;
  }
  await invoke("focus_partition", { appId });
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

export async function listenForPaneNotices(
  onRebuilt: (notice: PaneNotice) => void,
  onFailed: (notice: PaneNotice) => void,
): Promise<UnlistenFn> {
  const rebuilt = await listen<PaneNotice>("pane-rebuilt", (event) => {
    onRebuilt(event.payload);
  });
  const failed = await listen<PaneNotice>("pane-rebuild-failed", (event) => {
    onFailed(event.payload);
  });
  return () => {
    rebuilt();
    failed();
  };
}

export async function listenForPaneActivation(
  onActivate: (appId: string) => void,
): Promise<UnlistenFn> {
  return listen<{ appId: string }>("pane-activated", (event) => {
    if (event.payload.appId) {
      onActivate(event.payload.appId);
    }
  });
}
