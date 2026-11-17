import { useCallback, useRef, useState } from "react";
import HomeView from "./components/HomeView";
import HostView from "./components/HostView";
import ControllerView from "./components/ControllerView";

type View = "home" | "host" | "controller";

export interface EngineBoot {
  serverUrl: string;
  targetDeviceId?: string;
  permissions?: string[];
}

export interface Toast {
  id: number;
  tone: "ok" | "warn" | "info";
  message: string;
}

const MODE_LABEL: Record<View, string> = {
  home: "Siaga",
  host: "Host — layar dibagikan",
  controller: "Controller — mengendalikan",
};

export default function App() {
  const [view, setView] = useState<View>("home");
  const [boot, setBoot] = useState<EngineBoot>({ serverUrl: "http://localhost:8080" });
  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastSeq = useRef(0);

  const notify = useCallback((tone: Toast["tone"], message: string) => {
    const id = ++toastSeq.current;
    setToasts((prev) => [...prev.slice(-2), { id, tone, message }]);
    setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, 4200);
  }, []);

  return (
    <div className="flex h-full flex-col bg-base font-display text-ink">
      <header className="flex items-center gap-3 border-b border-line px-5 py-3">
        <div className="flex h-9 w-9 items-center justify-center rounded-md bg-brass font-mono text-lg font-bold text-brass-ink shadow-[0_2px_12px_rgba(227,168,87,0.35)]">
          J
        </div>
        <div className="leading-tight">
          <h1 className="text-[15px] font-semibold tracking-tight">
            JenderalRemote <span className="font-normal text-muted">Desktop</span>
          </h1>
          <p className="text-[11px] text-muted">
            Kendali jarak jauh Rust + WebRTC, dengan izin yang jelas
          </p>
        </div>
        <div
          className={`ml-auto rounded-full border px-3 py-1 font-mono text-[11px] ${
            view === "home"
              ? "border-line text-muted"
              : "border-line-strong text-phosphor"
          }`}
        >
          {MODE_LABEL[view]}
        </div>
      </header>

      <main className="min-h-0 flex-1">
        {view === "home" && (
          <HomeView
            notify={notify}
            onHost={(serverUrl) => {
              setBoot({ serverUrl });
              setView("host");
            }}
            onConnect={(serverUrl, target, permissions) => {
              setBoot({ serverUrl, targetDeviceId: target, permissions });
              setView("controller");
            }}
          />
        )}
        {view === "host" && (
          <HostView boot={boot} notify={notify} onExit={() => setView("home")} />
        )}
        {view === "controller" && (
          <ControllerView boot={boot} notify={notify} onExit={() => setView("home")} />
        )}
      </main>

      {/* Toast — umpan balik aksi, maksimal tiga */}
      <div className="pointer-events-none fixed bottom-5 left-1/2 z-[60] flex w-full max-w-md -translate-x-1/2 flex-col items-center gap-2 px-4">
        {toasts.map((toast) => (
          <div
            key={toast.id}
            className={`pointer-events-auto w-full rounded-lg border px-4 py-2.5 text-sm shadow-xl shadow-black/40 backdrop-blur ${
              toast.tone === "ok"
                ? "border-phosphor-dim bg-panel/95 text-phosphor"
                : toast.tone === "warn"
                  ? "border-danger/40 bg-panel/95 text-danger"
                  : "border-line-strong bg-panel/95 text-ink"
            }`}
          >
            {toast.message}
          </div>
        ))}
      </div>
    </div>
  );
}
