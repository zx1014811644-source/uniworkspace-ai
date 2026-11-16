import { invoke } from "@tauri-apps/api/core";
import { catalog } from "./catalog";
import type { CatalogApp } from "./types";
import { runningInTauri } from "./window";

export type LayoutMode = "single" | "split" | "grid";

export interface CustomApp {
  id: string;
  name: string;
  url: string;
}

export interface WorkspaceFile {
  version: 1;
  layout: LayoutMode;
  slots: (string | null)[];
  focusedPane: number;
  customApps: CustomApp[];
}

const STORAGE_KEY = "uniworkspace.workspace.v1";
const CUSTOM_ID =
  /^custom-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function visibleCount(layout: LayoutMode): number {
  if (layout === "single") {
    return 1;
  }
  if (layout === "split") {
    return 2;
  }
  return 4;
}

export function defaultWorkspace(): WorkspaceFile {
  return {
    version: 1,
    layout: "single",
    slots: [null, null, null, null],
    focusedPane: 0,
    customApps: [],
  };
}

export function isCustomId(id: string): boolean {
  return CUSTOM_ID.test(id);
}

export function officialWeb(app: CatalogApp): app is CatalogApp & { webUrl: string } {
  return app.kind === "web" && app.webStatus === "official" && typeof app.webUrl === "string";
}

export function pendingWeb(app: CatalogApp): boolean {
  return app.kind === "web" && !officialWeb(app);
}

export function normalizeHttpUrl(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed || trimmed.length > 2048) {
    throw new Error("链接为空或过长");
  }
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    throw new Error("链接格式不对");
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error("只接受 http 或 https 链接");
  }
  if (!url.hostname) {
    throw new Error("链接缺少主机名");
  }
  if (url.username || url.password) {
    throw new Error("链接里不要带账号密码");
  }
  return url.href;
}

export function embedTarget(
  appId: string,
  customApps: CustomApp[],
): { name: string; url: string } | null {
  const official = catalog.apps.find((app) => app.id === appId && officialWeb(app));
  if (official && officialWeb(official)) {
    return { name: official.name, url: official.webUrl };
  }
  const custom = customApps.find((app) => app.id === appId);
  if (!custom) {
    return null;
  }
  return { name: custom.name, url: custom.url };
}

export function normalizeWorkspace(input: unknown): WorkspaceFile {
  if (!input || typeof input !== "object") {
    throw new Error("布局配置无法解析");
  }
  const record = input as Partial<WorkspaceFile>;
  if (record.version !== 1) {
    throw new Error("布局配置版本无法识别");
  }
  if (record.layout !== "single" && record.layout !== "split" && record.layout !== "grid") {
    throw new Error("布局只能是单窗、1:1 或田字格");
  }
  if (!Array.isArray(record.slots) || record.slots.length !== 4) {
    throw new Error("布局需要记住 4 个格子");
  }
  const layout = record.layout;
  if (
    typeof record.focusedPane !== "number" ||
    record.focusedPane < 0 ||
    record.focusedPane >= visibleCount(layout)
  ) {
    throw new Error("当前布局里没有这一格");
  }
  if (!Array.isArray(record.customApps) || record.customApps.length > 40) {
    throw new Error("自定义链接太多");
  }

  const customApps: CustomApp[] = [];
  const customIds = new Set<string>();
  const customUrls = new Set<string>();
  for (const entry of record.customApps) {
    if (!entry || typeof entry.id !== "string" || !isCustomId(entry.id.trim())) {
      throw new Error("自定义链接的分区 id 无效");
    }
    const id = entry.id.trim();
    if (customIds.has(id)) {
      throw new Error("自定义链接的分区 id 重复");
    }
    customIds.add(id);
    const url = normalizeHttpUrl(typeof entry.url === "string" ? entry.url : "");
    if (customUrls.has(url)) {
      throw new Error("这个自定义链接已经添加过");
    }
    customUrls.add(url);
    const name = typeof entry.name === "string" ? entry.name.trim() : "";
    if (!name || [...name].length > 80) {
      throw new Error("自定义链接的名称无效");
    }
    customApps.push({ id, name, url });
  }

  const slots = record.slots.map((slot) => {
    if (slot === null) {
      return null;
    }
    if (typeof slot !== "string") {
      throw new Error("格子里的应用无法打开");
    }
    return slot;
  });
  const seen = new Set<string>();
  for (const slot of slots) {
    if (!slot) {
      continue;
    }
    if (seen.has(slot)) {
      throw new Error("同一个网页不能放进两格");
    }
    seen.add(slot);
    const known =
      catalog.apps.some((app) => app.id === slot && officialWeb(app)) ||
      customApps.some((app) => app.id === slot);
    if (!known) {
      const pending = catalog.apps.find((app) => app.id === slot && pendingWeb(app));
      throw new Error(pending ? "待确认的应用不能放进分屏" : `格子里的应用无法打开: ${slot}`);
    }
  }

  return {
    version: 1,
    layout,
    slots,
    focusedPane: record.focusedPane,
    customApps,
  };
}

export async function loadWorkspace(): Promise<WorkspaceFile> {
  if (!runningInTauri()) {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return defaultWorkspace();
    }
    try {
      return normalizeWorkspace(JSON.parse(raw) as unknown);
    } catch {
      localStorage.removeItem(STORAGE_KEY);
      return defaultWorkspace();
    }
  }
  return invoke<WorkspaceFile>("load_workspace");
}

export async function saveWorkspace(file: WorkspaceFile): Promise<WorkspaceFile> {
  const normalized = normalizeWorkspace(file);
  if (!runningInTauri()) {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(normalized));
    return normalized;
  }
  return invoke<WorkspaceFile>("save_workspace", { workspace: normalized });
}
