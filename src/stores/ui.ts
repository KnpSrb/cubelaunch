import { create } from "zustand";

interface UiState {
  newInstanceOpen: boolean;
  /** Файл модпака, с которого открыт диалог (перетаскивание). */
  importPath: string | null;
  openNewInstance: () => void;
  openImport: (path: string) => void;
  closeNewInstance: () => void;
  accountsOpen: boolean;
  setAccountsOpen: (open: boolean) => void;
  selectedInstance: string | null;
  selectInstance: (id: string | null) => void;
}

export const useUi = create<UiState>((set) => ({
  newInstanceOpen: false,
  importPath: null,
  openNewInstance: () => set({ newInstanceOpen: true, importPath: null }),
  openImport: (path) => set({ newInstanceOpen: true, importPath: path }),
  closeNewInstance: () => set({ newInstanceOpen: false }),
  accountsOpen: false,
  setAccountsOpen: (open) => set({ accountsOpen: open }),
  selectedInstance: null,
  selectInstance: (id) => set({ selectedInstance: id }),
}));
