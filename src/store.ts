import { create } from "zustand";
import {
  a11yChoose,
  a11yOpenSettings,
  a11yRequest,
  a11yStatus,
  type A11yChoice,
  type DetectedDesktop,
  type DesktopScan,
  type ListedWindow,
  type ServiceStatus,
} from "./desktop";
import { errorText, focusPartition } from "./partitions";
import {
  SNAP_TARGET_ID,
  snapAttach,
  snapDetach,
  snapUpdate,
  type SnapFrame,
  type SnapMode,
} from "./snap";
import {
  defaultWorkspace,
  loadWorkspace,
  normalizeHttpUrl,
  saveWorkspace,
  visibleCount,
  type CustomApp,
  type LayoutMode,
  type WorkspaceFile,
} from "./workspace";
import { runningInTauri } from "./window";

interface ShellState {
  ready: boolean;
  layout: LayoutMode;
  slots: (string | null)[];
  focusedPane: number;
  customApps: CustomApp[];
  inspectedId: string | null;
  syncError: string | null;
  loadError: string | null;
  paneNotes: Record<string, string>;
  reopenNonce: number;
  detectedApps: DetectedDesktop[];
  services: ServiceStatus[];
  windows: ListedWindow[];
  windowNote: string | null;
  designated: ListedWindow[];
  scanned: boolean;
  scanError: string | null;
  a11yReady: boolean;
  a11yOpen: boolean;
  a11yTrusted: boolean;
  a11yChoice: A11yChoice;
  a11yError: string | null;
  boot: () => Promise<void>;
  setLayout: (layout: LayoutMode) => Promise<void>;
  focusPane: (index: number) => Promise<void>;
  assignApp: (appId: string) => Promise<void>;
  clearPane: (index: number) => Promise<void>;
  clearAllPanes: () => Promise<void>;
  addCustomUrl: (raw: string, displayName?: string) => Promise<void>;
  removeCustom: (id: string) => Promise<void>;
  inspect: (id: string | null) => void;
  setSyncError: (message: string | null) => void;
  notePane: (appId: string, message: string) => void;
  applyScan: (scan: DesktopScan) => void;
  setScanError: (message: string | null) => void;
  designate: (window: ListedWindow) => void;
  undesignate: (windowId: number) => void;
  reopenA11y: () => void;
  hideA11y: () => void;
  requestA11y: () => Promise<void>;
  openA11ySettings: () => Promise<void>;
  refreshA11y: () => Promise<void>;
  chooseA11y: (choice: A11yChoice) => Promise<void>;
  snap: LiveSnap | null;
  snapRequest: number | null;
  snapBusy: boolean;
  snapError: string | null;
  snapNote: string | null;
  askSnap: () => void;
  attachSnap: (paneIndex: number, frame: SnapFrame) => Promise<void>;
  updateSnapFrame: (frame: SnapFrame) => Promise<void>;
  detachSnap: () => Promise<void>;
}

export interface LiveSnap {
  appId: string;
  paneIndex: number;
  mode: Exclude<SnapMode, "inactive">;
  message: string;
}

const A11Y_KEY = "uniworkspace.a11y.v1";

function savedBrowserChoice(): A11yChoice {
  const saved = localStorage.getItem(A11Y_KEY);
  if (saved === "granted" || saved === "webOnly") {
    return saved;
  }
  return "pending";
}

function snapshot(state: ShellState): WorkspaceFile {
  return {
    version: 1,
    layout: state.layout,
    slots: state.slots,
    focusedPane: state.focusedPane,
    customApps: state.customApps,
  };
}

let chain: Promise<void> = Promise.resolve();
let snapChain: Promise<void> = Promise.resolve();
let revision = 0;

function enqueueSnap(task: () => Promise<void>): Promise<void> {
  const run = snapChain.then(task, task);
  snapChain = run.then(
    () => undefined,
    () => undefined,
  );
  return run;
}

export const useShellStore = create<ShellState>((set, get) => {
  const commit = (
    recipe: (file: WorkspaceFile) => WorkspaceFile,
    extra?: Partial<ShellState>,
  ) => {
    const current = snapshot(get());
    const next = recipe(current);
    if (next === current) {
      if (extra) {
        set(extra);
      }
      return Promise.resolve();
    }
    const rev = ++revision;
    set({
      layout: next.layout,
      slots: next.slots,
      focusedPane: next.focusedPane,
      customApps: next.customApps,
      loadError: null,
      ...extra,
    });
    const task = async () => {
      if (rev !== revision) {
        return;
      }
      const latest = snapshot(get());
      try {
        const saved = await saveWorkspace(latest);
        if (rev !== revision) {
          return;
        }
        set({
          layout: saved.layout,
          slots: saved.slots,
          focusedPane: saved.focusedPane,
          customApps: saved.customApps,
          loadError: null,
        });
      } catch (reason: unknown) {
        if (rev === revision) {
          set({ loadError: errorText(reason) });
        }
      }
    };
    const run = chain.then(task, task);
    chain = run.then(
      () => undefined,
      () => undefined,
    );
    return run;
  };

  const focusSlot = async (index: number) => {
    const appId = get().slots[index];
    if (appId) {
      await focusPartition(appId);
    }
  };

  return {
    ready: false,
    ...defaultWorkspace(),
    inspectedId: null,
    syncError: null,
    loadError: null,
    paneNotes: {},
    reopenNonce: 0,
    detectedApps: [],
    services: [],
    windows: [],
    windowNote: null,
    designated: [],
    scanned: false,
    scanError: null,
    a11yReady: false,
    a11yOpen: false,
    a11yTrusted: false,
    a11yChoice: "pending",
    a11yError: null,
    snap: null,
    snapRequest: null,
    snapBusy: false,
    snapError: null,
    snapNote: null,
    boot: async () => {
      try {
        const file = await loadWorkspace();
        set({
          layout: file.layout,
          slots: file.slots,
          focusedPane: file.focusedPane,
          customApps: file.customApps,
          ready: true,
          loadError: null,
        });
      } catch (reason: unknown) {
        set({
          ...defaultWorkspace(),
          ready: true,
          loadError: errorText(reason),
        });
      }
      if (!runningInTauri()) {
        const choice = savedBrowserChoice();
        set({
          a11yReady: true,
          a11yChoice: choice,
          a11yTrusted: false,
          a11yOpen: choice === "pending",
          a11yError: null,
        });
        return;
      }
      try {
        const status = await a11yStatus();
        set({
          a11yReady: true,
          a11yChoice: status.choice,
          a11yTrusted: status.trusted,
          a11yOpen: status.choice === "pending",
          a11yError: null,
        });
      } catch (reason: unknown) {
        set({
          a11yReady: true,
          a11yOpen: true,
          a11yError: errorText(reason),
        });
      }
    },
    setLayout: async (layout) => {
      const snap = get().snap;
      if (snap && snap.paneIndex >= visibleCount(layout)) {
        await get().detachSnap();
      }
      await commit((file) => {
        const focusedPane = file.focusedPane >= visibleCount(layout) ? 0 : file.focusedPane;
        return { ...file, layout, focusedPane };
      });
    },
    focusPane: async (index) => {
      if (index < 0 || index >= visibleCount(get().layout)) {
        return;
      }
      await commit((file) => {
        if (index < 0 || index >= visibleCount(file.layout) || index === file.focusedPane) {
          return file;
        }
        return { ...file, focusedPane: index };
      });
      await focusSlot(index);
    },
    assignApp: async (appId) => {
      const snap = get().snap;
      if (snap && snap.paneIndex === get().focusedPane) {
        await get().detachSnap();
      }
      let reopen = false;
      await commit((file) => {
        const slots = [...file.slots];
        const existing = slots.findIndex((slot) => slot === appId);
        if (existing === file.focusedPane) {
          reopen = true;
          return file;
        }
        if (existing >= 0) {
          slots[existing] = null;
        }
        slots[file.focusedPane] = appId;
        return { ...file, slots };
      }, { inspectedId: null });
      if (reopen) {
        set((state) => {
          const paneNotes = { ...state.paneNotes };
          delete paneNotes[appId];
          return {
            inspectedId: null,
            reopenNonce: state.reopenNonce + 1,
            paneNotes,
          };
        });
      }
      await focusSlot(get().focusedPane);
    },
    clearPane: async (index) => {
      if (get().snap?.paneIndex === index) {
        await get().detachSnap();
      }
      await commit((file) => {
        if (!file.slots[index]) {
          return file;
        }
        return {
          ...file,
          slots: file.slots.map((slot, slotIndex) => (slotIndex === index ? null : slot)),
        };
      });
    },
    clearAllPanes: async () => {
      if (get().snap) {
        await get().detachSnap();
      }
      await commit((file) => ({
        ...file,
        slots: [null, null, null, null],
      }));
    },
    addCustomUrl: async (raw, displayName) => {
      let url: string;
      try {
        url = normalizeHttpUrl(raw);
      } catch (reason: unknown) {
        set({ loadError: errorText(reason) });
        return;
      }
      const existing = get().customApps.find((app) => app.url === url);
      if (existing) {
        await get().assignApp(existing.id);
        return;
      }
      const id = `custom-${crypto.randomUUID()}`;
      const requested = displayName?.trim() ?? "";
      const name = requested || new URL(url).hostname;
      if ([...name].length > 80) {
        set({ loadError: "自定义链接的名称无效" });
        return;
      }
      await commit((file) => {
        const slots = [...file.slots];
        slots[file.focusedPane] = id;
        return {
          ...file,
          customApps: [...file.customApps, { id, name, url }],
          slots,
        };
      }, { inspectedId: null });
      await focusSlot(get().focusedPane);
    },
    removeCustom: (id) =>
      commit((file) => ({
        ...file,
        customApps: file.customApps.filter((app) => app.id !== id),
        slots: file.slots.map((slot) => (slot === id ? null : slot)),
      })),
    inspect: (id) => set({ inspectedId: id }),
    setSyncError: (message) => set({ syncError: message }),
    notePane: (appId, message) =>
      set((state) => ({
        paneNotes: { ...state.paneNotes, [appId]: message },
      })),
    applyScan: (scan) =>
      set({
        detectedApps: scan.apps,
        services: scan.services,
        windows: scan.windows,
        windowNote: scan.windowNote,
        a11yTrusted: scan.trusted,
        scanned: true,
        scanError: null,
      }),
    setScanError: (message) => set({ scanError: message }),
    designate: (window) =>
      set((state) => {
        if (state.designated.some((item) => item.windowId === window.windowId)) {
          return state;
        }
        return { designated: [...state.designated, window] };
      }),
    undesignate: (windowId) =>
      set((state) => ({
        designated: state.designated.filter((item) => item.windowId !== windowId),
      })),
    reopenA11y: () => set({ a11yOpen: true }),
    hideA11y: () => set({ a11yOpen: false }),
    requestA11y: async () => {
      if (!runningInTauri()) {
        set({ a11yError: "系统授权要在 macOS 应用里发起。网页工作台可以继续使用。" });
        return;
      }
      try {
        const status = await a11yRequest();
        set({ a11yTrusted: status.trusted, a11yChoice: status.choice, a11yError: null });
      } catch (reason: unknown) {
        set({ a11yError: errorText(reason) });
      }
    },
    openA11ySettings: async () => {
      if (!runningInTauri()) {
        set({ a11yError: "系统设置要在 macOS 应用里打开。网页工作台可以继续使用。" });
        return;
      }
      try {
        await a11yOpenSettings();
        set({ a11yError: null });
      } catch (reason: unknown) {
        set({ a11yError: errorText(reason) });
      }
    },
    refreshA11y: async () => {
      if (!runningInTauri()) {
        set({ a11yError: "辅助功能要在 macOS 应用里检查。网页工作台可以继续使用。" });
        return;
      }
      try {
        const status = await a11yStatus();
        set({
          a11yTrusted: status.trusted,
          a11yChoice: status.choice,
          a11yError: null,
          a11yOpen: status.choice === "pending" ? true : get().a11yOpen,
        });
      } catch (reason: unknown) {
        set({ a11yError: errorText(reason) });
      }
    },
    askSnap: () => {
      set({ snapRequest: get().focusedPane, snapError: null, snapNote: null });
    },
    attachSnap: (paneIndex, frame) =>
      enqueueSnap(async () => {
        set({ snapBusy: true, snapRequest: null, snapError: null });
        try {
          const status = await snapAttach(SNAP_TARGET_ID, frame);
          if (!status.active || status.mode === "inactive") {
            set({
              snap: null,
              snapBusy: false,
              snapError: status.message,
            });
            return;
          }
          set({
            snap: {
              appId: SNAP_TARGET_ID,
              paneIndex,
              mode: status.mode,
              message: status.message ?? "",
            },
            snapBusy: false,
            snapNote: null,
          });
        } catch (reason: unknown) {
          set({
            snap: null,
            snapBusy: false,
            snapRequest: null,
            snapError: errorText(reason),
          });
        }
      }),
    updateSnapFrame: (frame) =>
      enqueueSnap(async () => {
        const current = get().snap;
        if (!current) {
          return;
        }
        try {
          const status = await snapUpdate(frame);
          const latest = get().snap;
          if (!latest || latest.paneIndex !== current.paneIndex) {
            return;
          }
          if (!status.active || status.mode === "inactive") {
            set({ snap: null, snapError: status.message });
            return;
          }
          if (status.mode !== latest.mode || (status.message ?? "") !== latest.message) {
            set({
              snap: {
                ...latest,
                mode: status.mode,
                message: status.message ?? "",
              },
            });
          }
        } catch (reason: unknown) {
          if (get().snap) {
            set({ snapError: errorText(reason) });
          }
        }
      }),
    detachSnap: () =>
      enqueueSnap(async () => {
        set({ snapBusy: true, snapRequest: null });
        try {
          const status = await snapDetach();
          set({
            snap: null,
            snapBusy: false,
            snapError: null,
            snapNote: status.message,
          });
        } catch (reason: unknown) {
          set({ snapBusy: false, snapError: errorText(reason) });
        }
      }),
    chooseA11y: async (choice) => {
      if (!runningInTauri()) {
        if (choice !== "webOnly") {
          set({ a11yError: "辅助功能要在 macOS 应用里完成。网页工作台可以继续使用。" });
          return;
        }
        localStorage.setItem(A11Y_KEY, "webOnly");
        set({ a11yChoice: "webOnly", a11yOpen: false, a11yError: null });
        return;
      }
      try {
        const status = await a11yChoose(choice);
        set({
          a11yTrusted: status.trusted,
          a11yChoice: status.choice,
          a11yOpen: status.choice === "pending",
          a11yError: null,
        });
      } catch (reason: unknown) {
        set({ a11yError: errorText(reason) });
      }
    },
  };
});
