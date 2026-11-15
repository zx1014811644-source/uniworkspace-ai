import { create } from "zustand";

interface ShellState {
  selectedId: string | null;
  select: (id: string | null) => void;
}

export const useShellStore = create<ShellState>((set) => ({
  selectedId: null,
  select: (id) => set({ selectedId: id }),
}));
