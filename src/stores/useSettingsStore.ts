import { create } from "zustand";
import { getSettings, getSystemInfo, updateSettings } from "../lib/api";
import type { Settings, SystemInfo } from "../lib/types";

interface SettingsState {
  settings: Settings | null;
  systemInfo: SystemInfo | null;
  loading: boolean;
  saving: boolean;
  error: string | null;
  load: () => Promise<void>;
  update: (patch: Partial<Settings>) => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: null,
  systemInfo: null,
  loading: false,
  saving: false,
  error: null,

  async load() {
    set({ loading: true, error: null });
    try {
      const [settings, systemInfo] = await Promise.all([getSettings(), getSystemInfo()]);
      set({ settings, systemInfo, loading: false });
    } catch (error) {
      set({ error: errorMessage(error), loading: false });
    }
  },

  async update(patch) {
    const current = get().settings;
    if (current) {
      set({ settings: { ...current, ...patch }, saving: true, error: null });
    } else {
      set({ saving: true, error: null });
    }

    try {
      const settings = await updateSettings(patch);
      set({ settings, saving: false });
    } catch (error) {
      if (current) {
        set({ settings: current });
      }
      set({ error: errorMessage(error), saving: false });
    }
  },
}));

function errorMessage(error: unknown) {
  if (error && typeof error === "object" && "message" in error) {
    return String(error.message);
  }
  return String(error);
}
