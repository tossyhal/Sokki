import { create } from "zustand";
import {
  cancelDownload,
  deleteModel,
  downloadModel,
  getModels,
  verifyModel,
} from "../lib/api";
import type { ModelErrorEvent, ModelInfo, ModelProgressEvent } from "../lib/types";

type ModelAction = "download" | "cancel" | "delete" | "verify";

interface ModelProgress {
  downloadedBytes: number;
  totalBytes: number | null;
}

interface ModelStore {
  models: ModelInfo[];
  progressByName: Record<string, ModelProgress>;
  actionByName: Record<string, ModelAction>;
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
  download: (name: string) => Promise<void>;
  cancel: (name: string) => Promise<void>;
  delete: (name: string) => Promise<void>;
  verify: (name: string) => Promise<void>;
  applyProgress: (event: ModelProgressEvent) => void;
  applyDone: (name: string) => Promise<void>;
  applyError: (event: ModelErrorEvent) => void;
}

export const useModelStore = create<ModelStore>((set, get) => ({
  models: [],
  progressByName: {},
  actionByName: {},
  loading: false,
  error: null,

  async load() {
    set({ loading: true, error: null });
    try {
      const models = await getModels();
      set({ models, loading: false });
    } catch (error) {
      set({ error: errorMessage(error), loading: false });
    }
  },

  async download(name) {
    setAction(set, name, "download");
    try {
      await downloadModel(name);
      await get().load();
      clearAction(set, name);
    } catch (error) {
      set({ error: errorMessage(error) });
      clearAction(set, name);
    }
  },

  async cancel(name) {
    setAction(set, name, "cancel");
    try {
      await cancelDownload(name);
    } catch (error) {
      set({ error: errorMessage(error) });
    } finally {
      clearAction(set, name);
      clearProgress(set, name);
    }
  },

  async delete(name) {
    setAction(set, name, "delete");
    try {
      const model = await deleteModel(name);
      replaceModel(set, model);
      clearProgress(set, name);
      clearAction(set, name);
    } catch (error) {
      set({ error: errorMessage(error) });
      clearAction(set, name);
    }
  },

  async verify(name) {
    setAction(set, name, "verify");
    try {
      const model = await verifyModel(name);
      replaceModel(set, model);
      clearAction(set, name);
    } catch (error) {
      set({ error: errorMessage(error) });
      await get().load();
      clearAction(set, name);
    }
  },

  applyProgress(event) {
    set((state) => ({
      progressByName: {
        ...state.progressByName,
        [event.name]: {
          downloadedBytes: event.downloadedBytes,
          totalBytes: event.totalBytes,
        },
      },
    }));
  },

  async applyDone(name) {
    clearProgress(set, name);
    clearAction(set, name);
    await get().load();
  },

  applyError(event) {
    set({ error: event.message });
    clearProgress(set, event.name);
    clearAction(set, event.name);
  },
}));

export function usableModels(models: ModelInfo[]) {
  return models.filter((model) => model.usable && !model.corrupted);
}

function setAction(set: typeof useModelStore.setState, name: string, action: ModelAction) {
  set((state) => ({
    error: null,
    actionByName: {
      ...state.actionByName,
      [name]: action,
    },
  }));
}

function clearAction(set: typeof useModelStore.setState, name: string) {
  set((state) => {
    const { [name]: _removed, ...rest } = state.actionByName;
    return { actionByName: rest };
  });
}

function clearProgress(set: typeof useModelStore.setState, name: string) {
  set((state) => {
    const { [name]: _removed, ...rest } = state.progressByName;
    return { progressByName: rest };
  });
}

function replaceModel(set: typeof useModelStore.setState, model: ModelInfo) {
  set((state) => ({
    models: state.models.map((item) => (item.name === model.name ? model : item)),
  }));
}

function errorMessage(error: unknown) {
  if (error && typeof error === "object" && "message" in error) {
    return String(error.message);
  }
  return String(error);
}
