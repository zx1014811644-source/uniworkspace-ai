import { create } from "zustand";
import { errorText, focusPartition } from "./partitions";
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
  boot: () => Promise<void>;
  setLayout: (layout: LayoutMode) => Promise<void>;
  focusPane: (index: number) => Promise<void>;
  assignApp: (appId: string) => Promise<void>;
  clearPane: (index: number) => Promise<void>;
  clearAllPanes: () => Promise<void>;
  addCustomUrl: (raw: string) => Promise<void>;
  removeCustom: (id: string) => Promise<void>;
  inspect: (id: string | null) => void;
  setSyncError: (message: string | null) => void;
  notePane: (appId: string, message: string) => void;
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
let revision = 0;

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
    },
    setLayout: (layout) =>
      commit((file) => {
        const focusedPane = file.focusedPane >= visibleCount(layout) ? 0 : file.focusedPane;
        return { ...file, layout, focusedPane };
      }),
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
    clearPane: (index) =>
      commit((file) => {
        if (!file.slots[index]) {
          return file;
        }
        return {
          ...file,
          slots: file.slots.map((slot, slotIndex) => (slotIndex === index ? null : slot)),
        };
      }),
    clearAllPanes: () =>
      commit((file) => ({
        ...file,
        slots: [null, null, null, null],
      })),
    addCustomUrl: async (raw) => {
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
      const name = new URL(url).hostname;
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
  };
});
