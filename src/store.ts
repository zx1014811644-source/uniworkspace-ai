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

export const useShellStore = create<ShellState>((set, get) => {
  const commit = (next: WorkspaceFile, extra?: Partial<ShellState>) => {
    const task = async () => {
      try {
        const saved = await saveWorkspace(next);
        set({
          layout: saved.layout,
          slots: saved.slots,
          focusedPane: saved.focusedPane,
          customApps: saved.customApps,
          loadError: null,
          ...extra,
        });
      } catch (reason: unknown) {
        set({ loadError: errorText(reason) });
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
    setLayout: (layout) => {
      const state = get();
      const focusedPane =
        state.focusedPane >= visibleCount(layout) ? 0 : state.focusedPane;
      return commit({ ...snapshot(state), layout, focusedPane });
    },
    focusPane: async (index) => {
      const state = get();
      if (index < 0 || index >= visibleCount(state.layout)) {
        return;
      }
      if (index === state.focusedPane) {
        await focusSlot(index);
        return;
      }
      await commit({ ...snapshot(state), focusedPane: index });
      await focusSlot(index);
    },
    assignApp: async (appId) => {
      const state = get();
      const slots = [...state.slots];
      const existing = slots.findIndex((slot) => slot === appId);
      if (existing === state.focusedPane) {
        set({ inspectedId: null });
        await focusSlot(state.focusedPane);
        return;
      }
      if (existing >= 0) {
        slots[existing] = null;
      }
      slots[state.focusedPane] = appId;
      await commit({ ...snapshot(state), slots }, { inspectedId: null });
      await focusSlot(get().focusedPane);
    },
    clearPane: (index) => {
      const state = get();
      if (!state.slots[index]) {
        return Promise.resolve();
      }
      const slots = state.slots.map((slot, slotIndex) => (slotIndex === index ? null : slot));
      return commit({ ...snapshot(state), slots });
    },
    clearAllPanes: () => {
      const state = get();
      return commit({
        ...snapshot(state),
        slots: [null, null, null, null],
      });
    },
    addCustomUrl: async (raw) => {
      const state = get();
      let url: string;
      try {
        url = normalizeHttpUrl(raw);
      } catch (reason: unknown) {
        set({ loadError: errorText(reason) });
        return;
      }
      const existing = state.customApps.find((app) => app.url === url);
      if (existing) {
        await get().assignApp(existing.id);
        return;
      }
      const id = `custom-${crypto.randomUUID()}`;
      const customApps = [...state.customApps, { id, name: new URL(url).hostname, url }];
      const slots = [...state.slots];
      const previous = slots.findIndex((slot) => slot === id);
      if (previous >= 0) {
        slots[previous] = null;
      }
      slots[state.focusedPane] = id;
      await commit({ ...snapshot(state), customApps, slots }, { inspectedId: null });
      await focusSlot(get().focusedPane);
    },
    removeCustom: (id) => {
      const state = get();
      return commit({
        ...snapshot(state),
        customApps: state.customApps.filter((app) => app.id !== id),
        slots: state.slots.map((slot) => (slot === id ? null : slot)),
      });
    },
    inspect: (id) => set({ inspectedId: id }),
    setSyncError: (message) => set({ syncError: message }),
    notePane: (appId, message) =>
      set((state) => ({
        paneNotes: { ...state.paneNotes, [appId]: message },
      })),
  };
});
