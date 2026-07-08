import { create } from "zustand";
import {
  cancelTranscription,
  deleteSession,
  getSession,
  getSessions,
  renameSession,
} from "../lib/api";
import type { Session } from "../lib/types";

interface SessionStore {
  sessions: Session[];
  selected: Session | null;
  loading: boolean;
  savingId: string | null;
  deletingId: string | null;
  cancelingId: string | null;
  error: string | null;
  load: () => Promise<void>;
  loadOne: (id: string) => Promise<Session | null>;
  applyStatus: (id: string, status: Session["status"], message?: string | null) => void;
  rename: (id: string, title: string) => Promise<void>;
  cancel: (id: string) => Promise<void>;
  delete: (id: string) => Promise<void>;
}

export const useSessionStore = create<SessionStore>((set, get) => ({
  sessions: [],
  selected: null,
  loading: false,
  savingId: null,
  deletingId: null,
  cancelingId: null,
  error: null,

  async load() {
    set({ loading: true, error: null });
    try {
      const sessions = await getSessions();
      set({ sessions, loading: false });
    } catch (error) {
      set({ error: errorMessage(error), loading: false });
    }
  },

  async loadOne(id) {
    set({ loading: true, error: null });
    try {
      const session = await getSession(id);
      set({ selected: session, loading: false });
      return session;
    } catch (error) {
      set({ error: errorMessage(error), loading: false, selected: null });
      return null;
    }
  },

  applyStatus(id, status, message) {
    const apply = (session: Session) =>
      session.id === id
        ? {
            ...session,
            status,
            errorMessage: status === "error" ? (message ?? session.errorMessage) : null,
          }
        : session;
    set({
      sessions: get().sessions.map(apply),
      selected: get().selected?.id === id ? apply(get().selected) : get().selected,
    });
  },

  async rename(id, title) {
    const previous = get().sessions;
    set({ savingId: id, error: null });
    try {
      const session = await renameSession(id, title);
      set({
        sessions: get().sessions.map((item) => (item.id === id ? session : item)),
        selected: get().selected?.id === id ? session : get().selected,
        savingId: null,
      });
    } catch (error) {
      set({ sessions: previous, error: errorMessage(error), savingId: null });
    }
  },

  async cancel(id) {
    set({ cancelingId: id, error: null });
    try {
      const session = await cancelTranscription(id);
      set({
        sessions: get().sessions.map((item) => (item.id === id ? session : item)),
        selected: get().selected?.id === id ? session : get().selected,
        cancelingId: null,
      });
    } catch (error) {
      set({ error: errorMessage(error), cancelingId: null });
    }
  },

  async delete(id) {
    const previous = get().sessions;
    set({
      deletingId: id,
      sessions: previous.filter((session) => session.id !== id),
      error: null,
    });
    try {
      await deleteSession(id);
      if (get().selected?.id === id) {
        set({ selected: null });
      }
      set({ deletingId: null });
    } catch (error) {
      set({ sessions: previous, error: errorMessage(error), deletingId: null });
    }
  },
}));

function errorMessage(error: unknown) {
  if (error && typeof error === "object" && "message" in error) {
    return String(error.message);
  }
  return String(error);
}
