import { create } from "zustand";
import {
  getRecordingState,
  getSettings,
  listAudioDevices,
  pauseRecording,
  resumeRecording,
  startRecording,
  stopRecording,
} from "../lib/api";
import type {
  AudioDevices,
  Language,
  RecordingDropsEvent,
  RecordingElapsedEvent,
  RecordingLevelEvent,
  RecordingState,
  Session,
  Source,
  StartRecordingRequest,
} from "../lib/types";

type RecordingSource = Exclude<Source, "import">;

interface RecordingSetup {
  source: RecordingSource;
  language: Language;
  model: string;
  micDevice: string | null;
  loopbackDevice: string | null;
}

interface RecordingStore {
  devices: AudioDevices | null;
  state: RecordingState;
  levels: RecordingLevelEvent;
  dropCount: number;
  setup: RecordingSetup;
  loading: boolean;
  starting: boolean;
  stopping: boolean;
  error: string | null;
  load: () => Promise<void>;
  setSource: (source: RecordingSource) => void;
  setLanguage: (language: Language) => void;
  setModel: (model: string) => void;
  setMicDevice: (micDevice: string | null) => void;
  setLoopbackDevice: (loopbackDevice: string | null) => void;
  start: () => Promise<void>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  stop: () => Promise<Session | null>;
  applyLevel: (event: RecordingLevelEvent) => void;
  applyElapsed: (event: RecordingElapsedEvent) => void;
  applyDrops: (event: RecordingDropsEvent) => void;
}

const initialState: RecordingState = {
  active: false,
  sessionId: null,
  paused: false,
  elapsedMs: 0,
};

const initialSetup: RecordingSetup = {
  source: "mic",
  language: "ja",
  model: "medium-q5_0",
  micDevice: null,
  loopbackDevice: null,
};

export const useRecordingStore = create<RecordingStore>((set, get) => ({
  devices: null,
  state: initialState,
  levels: { mic: 0, system: 0 },
  dropCount: 0,
  setup: initialSetup,
  loading: false,
  starting: false,
  stopping: false,
  error: null,

  async load() {
    set({ loading: true, error: null });
    try {
      const [settings, devices, state] = await Promise.all([
        getSettings(),
        listAudioDevices(),
        getRecordingState(),
      ]);
      set({
        devices,
        state,
        setup: {
          ...get().setup,
          language: settings.language,
          model: settings.defaultModel,
        },
        loading: false,
      });
    } catch (error) {
      set({ error: errorMessage(error), loading: false });
    }
  },

  setSource(source) {
    set((state) => ({
      setup: {
        ...state.setup,
        source,
      },
    }));
  },

  setLanguage(language) {
    set((state) => ({ setup: { ...state.setup, language } }));
  },

  setModel(model) {
    set((state) => ({ setup: { ...state.setup, model } }));
  },

  setMicDevice(micDevice) {
    set((state) => ({ setup: { ...state.setup, micDevice } }));
  },

  setLoopbackDevice(loopbackDevice) {
    set((state) => ({ setup: { ...state.setup, loopbackDevice } }));
  },

  async start() {
    const setup = get().setup;
    const request: StartRecordingRequest = {
      source: setup.source,
      language: setup.language,
      model: setup.model,
      micDevice: setup.source === "system" ? null : setup.micDevice,
      loopbackDevice: setup.source === "mic" ? null : setup.loopbackDevice,
    };

    set({ starting: true, error: null });
    try {
      const sessionId = await startRecording(request);
      const state = await getRecordingState();
      set({
        state: {
          ...state,
          active: true,
          sessionId: state.sessionId ?? sessionId,
        },
        levels: { mic: 0, system: 0 },
        dropCount: 0,
        starting: false,
      });
    } catch (error) {
      set({ error: errorMessage(error), starting: false });
    }
  },

  async pause() {
    set({ error: null });
    try {
      await pauseRecording();
      const state = await getRecordingState();
      set({ state });
    } catch (error) {
      set({ error: errorMessage(error) });
    }
  },

  async resume() {
    set({ error: null });
    try {
      await resumeRecording();
      const state = await getRecordingState();
      set({ state });
    } catch (error) {
      set({ error: errorMessage(error) });
    }
  },

  async stop() {
    set({ stopping: true, error: null });
    try {
      const session = await stopRecording();
      set({
        state: initialState,
        levels: { mic: 0, system: 0 },
        stopping: false,
      });
      return session;
    } catch (error) {
      set({ error: errorMessage(error), stopping: false });
      return null;
    }
  },

  applyLevel(event) {
    set({ levels: event });
  },

  applyElapsed(event) {
    set((current) => ({
      state: {
        ...current.state,
        elapsedMs: event.elapsedMs,
      },
    }));
  },

  applyDrops(event) {
    set({ dropCount: event.dropCount });
  },
}));

function errorMessage(error: unknown) {
  if (error && typeof error === "object" && "message" in error) {
    return String(error.message);
  }
  return String(error);
}
