import { NavLink, Navigate, Route, Routes, useLocation } from "react-router-dom";
import { useEffect } from "react";
import Library from "./pages/Library";
import Onboarding from "./pages/Onboarding";
import Record from "./pages/Record";
import SessionDetail from "./pages/SessionDetail";
import Settings from "./pages/Settings";
import { initEventListeners } from "./lib/events";
import { useSettingsStore } from "./stores/useSettingsStore";

const navItems = [
  { to: "/", label: "ライブラリ" },
  { to: "/settings", label: "設定" },
];

function navClassName({ isActive }: { isActive: boolean }) {
  return [
    "rounded-btn px-3 py-2 text-body transition-colors",
    isActive ? "bg-elevate text-ink" : "text-ink-2 hover:bg-surface hover:text-ink",
  ].join(" ");
}

export default function App() {
  const location = useLocation();
  const { settings, loading, load } = useSettingsStore();

  useEffect(() => {
    void initEventListeners();
  }, []);

  useEffect(() => {
    if (!settings && !loading) {
      void load();
    }
  }, [load, loading, settings]);

  const onboardingRequired = settings ? !settings.onboardingDone : false;

  return (
    <div className="flex min-h-screen bg-bg text-ink">
      <aside className="flex w-sidebar shrink-0 flex-col border-r border-line bg-surface-2 px-4 py-5">
        <div className="mb-8 px-2">
          <p className="text-title">Sokki</p>
          <p className="mt-1 text-meta text-ink-2">ローカル文字起こし</p>
        </div>

        <NavLink
          to="/record"
          className="mb-4 rounded-btn bg-accent px-3 py-2 text-center text-body font-semibold text-white shadow-accent hover:bg-accent-hover"
        >
          新規録音
        </NavLink>

        <nav className="grid gap-1">
          {navItems.map((item) => (
            <NavLink key={item.to} to={item.to} className={navClassName} end={item.to === "/"}>
              {item.label}
            </NavLink>
          ))}
        </nav>
      </aside>

      <main className="min-w-0 flex-1">
        {loading && !settings ? (
          <section className="px-8 py-7">
            <h1 className="text-h1">Sokki</h1>
            <p className="mt-2 text-body text-ink-2">読み込み中</p>
          </section>
        ) : onboardingRequired && location.pathname !== "/onboarding" ? (
          <Navigate to="/onboarding" replace />
        ) : !onboardingRequired && location.pathname === "/onboarding" ? (
          <Navigate to="/" replace />
        ) : (
          <Routes>
            <Route path="/onboarding" element={<Onboarding />} />
            <Route path="/" element={<Library />} />
            <Route path="/record" element={<Record />} />
            <Route path="/session/:id" element={<SessionDetail />} />
            <Route path="/settings" element={<Settings />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        )}
      </main>
    </div>
  );
}
