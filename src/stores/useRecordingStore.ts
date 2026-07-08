import { create } from "zustand";
import {
  getRecordingState,
  getSettings,
  listAudioDevices,
  pauseRecording,
  resumeRecording,
  runSoundCheck,
  startRecording,
  stopRecording,
} from "../lib/api";
import type {
  AudioDevices,
  Language,
  RecordingDropsEvent,
  RecordingElapsedEvent,
  RecordingLimitEvent,
  RecordingLevelEvent,
  RecordingState,
  Session,
  SoundCheckLevelEvent,
  SoundCheckResult,
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
  soundCheckRunning: boolean;
  soundCheckLevels: SoundCheckLevelEvent;
  soundCheckResult: SoundCheckResult | null;
  setup: RecordingSetup;
  loading: boolean;
  starting: boolean;
  stopping: boolean;
  error: string | null;
  durationLimitWarning: string | null;
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
  runSoundCheck: () => Promise<void>;
  applyLevel: (event: RecordingLevelEvent) => void;
  applyElapsed: (event: RecordingElapsedEvent) => void;
  applyDrops: (event: RecordingDropsEvent) => void;
  applyLimit: (event: RecordingLimitEvent) => Promise<Session | null>;
  applySoundCheckLevel: (event: SoundCheckLevelEvent) => void;
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
  soundCheckRunning: false,
  soundCheckLevels: { mic: 0, system: 0 },
  soundCheckResult: null,
  setup: initialSetup,
  loading: false,
  starting: false,
  stopping: false,
  error: null,
  durationLimitWarning: null,

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
        durationLimitWarning: null,
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
        durationLimitWarning: null,
        stopping: false,
      });
      return session;
    } catch (error) {
      set({ error: errorMessage(error), stopping: false });
      return null;
    }
  },

  async runSoundCheck() {
    const setup = get().setup;
    set({
      soundCheckRunning: true,
      soundCheckLevels: { mic: 0, system: 0 },
      soundCheckResult: null,
      error: null,
    });
    try {
      const result = await runSoundCheck({
        source: setup.source,
        micDevice: setup.source === "system" ? null : setup.micDevice,
        loopbackDevice: setup.source === "mic" ? null : setup.loopbackDevice,
      });
      set({ soundCheckResult: result, soundCheckRunning: false });
    } catch (error) {
      set({ error: errorMessage(error), soundCheckRunning: false });
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

  async applyLimit(event) {
    if (event.kind === "warning") {
      set({
        durationLimitWarning: `録音は最大${formatLimit(event.maxMs)}です。あと約${formatLimit(
          event.maxMs - event.elapsedMs,
        )}で自動停止します。`,
      });
      return null;
    }

    if (!get().state.active || get().stopping) {
      return null;
    }

    set({ durationLimitWarning: "録音が最大時間に達したため自動停止します。" });
    return get().stop();
  },

  applySoundCheckLevel(event) {
    set({ soundCheckLevels: event });
  },
}));

function errorMessage(error: unknown) {
  if (error && typeof error === "object" && "message" in error) {
    return String(error.message);
  }
  return String(error);
}

function formatLimit(ms: number) {
  const minutes = Math.max(1, Math.ceil(ms / 60_000));
  if (minutes >= 60 && minutes % 60 === 0) {
    return `${minutes / 60}時間`;
  }
  return `${minutes}分`;
}
