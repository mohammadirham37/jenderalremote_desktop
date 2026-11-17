import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { hostEnd, hostRespond, hostStart } from "../api";
import type { EngineBoot } from "../App";
import type { HostEventDto, MetricsSnapshot } from "../types";
import { Button, StatChip } from "./ui";

interface PendingRequest {
  session_id: string;
  controller_device_id: string;
  mode: "attended" | "unattended";
  permissions: string[];
}

const PERMISSION_LABEL: Record<string, string> = {
  view_screen: "Melihat layar",
  control_input: "Mengendalikan mouse & keyboard",
  clipboard: "Membaca clipboard",
  audio: "Mendengar audio",
};

export default function HostView({
  boot,
  notify,
  onExit,
}: {
  boot: EngineBoot;
  notify: (tone: "ok" | "warn" | "info", message: string) => void;
  onExit: () => void;
}) {
  const [status, setStatus] = useState("idle");
  const [error, setError] = useState<string | null>(null);
  const [request, setRequest] = useState<PendingRequest | null>(null);
  const [granted, setGranted] = useState<string[]>([]);
  const [stats, setStats] = useState<MetricsSnapshot | null>(null);
  const [sessionActive, setSessionActive] = useState(false);

  useEffect(() => {
    let mounted = true;
    let unlisten: UnlistenFn | null = null;

    listen<HostEventDto>("host://event", (event) => {
      if (!mounted) return;
      const payload = event.payload;
      switch (payload.kind) {
        case "token_issued":
          break;
        case "session_request":
          setRequest({
            session_id: payload.session_id,
            controller_device_id: payload.controller_device_id,
            mode: payload.mode,
            permissions: payload.permissions,
          });
          setGranted(payload.permissions);
          notify("info", "Permintaan sesi masuk — menunggu keputusan Anda");
          break;
        case "state_changed":
          setStatus(payload.status);
          setSessionActive(payload.status === "active");
          break;
        case "stats":
          setStats(payload.snapshot);
          break;
        case "session_ended":
          setSessionActive(false);
          setRequest(null);
          notify("warn", `Sesi berakhir: ${payload.reason ?? "selesai"}`);
          break;
        case "error":
          setError(payload.message);
          break;
      }
    }).then((fn) => {
      unlisten = fn;
    });

    hostStart(boot.serverUrl).catch((e) => setError(String(e)));

    return () => {
      mounted = false;
      unlisten?.();
      hostEnd("window closed").catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const decide = (approved: boolean) => {
    if (!request) return;
    hostRespond(request.session_id, approved, granted).catch((e) =>
      setError(String(e)),
    );
    notify(
      approved ? "ok" : "warn",
      approved ? "Sesi disetujui, menegosiasikan koneksi" : "Permintaan ditolak",
    );
    setRequest(null);
  };

  const togglePermission = (name: string) => {
    setGranted((prev) =>
      prev.includes(name) ? prev.filter((p) => p !== name) : [...prev, name],
    );
  };

  return (
    <div className="relative h-full overflow-y-auto">
      {/* Modal permintaan sesi — "permohonan izin" */}
      {request && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
          role="dialog"
          aria-modal="true"
          aria-label="Permintaan sesi masuk"
        >
          <div className="w-full max-w-md rounded-xl border border-brass/50 bg-panel p-6 shadow-2xl shadow-black/60">
            <div className="mb-4 flex items-center gap-2.5">
              <span className="h-2.5 w-2.5 rounded-full bg-brass" />
              <h3 className="text-[15px] font-semibold text-brass">
                Permintaan akses masuk
              </h3>
            </div>
            <p className="mb-1 text-[13px] text-muted">Perangkat pengendali:</p>
            <code className="mb-4 block select-all rounded-lg border border-line bg-base px-3 py-2.5 font-mono text-sm text-brass">
              {request.controller_device_id}
            </code>
            <p className="mb-3 text-[13px] text-muted">
              Meminta izin ({request.mode === "attended" ? "attended" : "unattended"})
              untuk:
            </p>
            <div className="mb-5 flex flex-col gap-2">
              {request.permissions.map((permission) => {
                const locked = permission === "view_screen";
                const checked = granted.includes(permission);
                return (
                  <label
                    key={permission}
                    className={`flex items-center gap-3 rounded-lg border px-3.5 py-2.5 text-sm transition ${
                      checked
                        ? "border-phosphor-dim/60 bg-base"
                        : "border-line bg-base/50"
                    } ${locked ? "cursor-default" : "cursor-pointer"}`}
                  >
                    <input
                      type="checkbox"
                      className="h-4 w-4 accent-[#7be382]"
                      checked={checked}
                      disabled={locked}
                      onChange={() => togglePermission(permission)}
                    />
                    <span className="flex-1">
                      {PERMISSION_LABEL[permission] ?? permission}
                    </span>
                    {locked && (
                      <span className="font-mono text-[10px] text-muted">
                        wajib
                      </span>
                    )}
                  </label>
                );
              })}
            </div>
            <p className="mb-5 text-[11px] leading-relaxed text-muted">
              Anda hanya bisa mempersempit izin, tidak memperluas. Sesi bisa
              Anda hentikan kapan pun tanpa alasan.
            </p>
            <div className="flex justify-end gap-3">
              <Button variant="danger" onClick={() => decide(false)}>
                Tolak
              </Button>
              <Button onClick={() => decide(true)}>Izinkan</Button>
            </div>
          </div>
        </div>
      )}

      <div className="mx-auto flex max-w-2xl flex-col gap-5 px-6 py-8">
        {/* Indikator siaran — merah berdenyut saat sesi berjalan */}
        <section
          className={`rounded-xl border px-6 py-5 transition-colors ${
            sessionActive
              ? "border-danger/50 bg-danger-dim/25"
              : "border-line bg-panel"
          }`}
        >
          <div className="mb-2 flex items-center gap-3">
            <span
              className={`h-3 w-3 rounded-full ${
                sessionActive ? "live-dot bg-danger" : "bg-phosphor"
              }`}
            />
            <h2 className="text-lg font-semibold">
              {sessionActive
                ? "Layar Anda sedang dibagikan"
                : status === "idle"
                  ? "Mendaftarkan ke signaling…"
                  : "Host siap — menunggu permintaan"}
            </h2>
          </div>
          <p className="text-[13px] leading-relaxed text-muted">
            {sessionActive
              ? "Indikator ini tetap tampil selama sesi aktif. Tutup kapan pun lewat tombol di bawah."
              : "Perangkat terdaftar dan online. Permintaan sesi akan muncul sebagai dialog persetujuan."}
          </p>
          {status !== "idle" && !sessionActive && (
            <p className="mt-2 font-mono text-[11px] text-phosphor">
              status: {status}
            </p>
          )}
        </section>

        {stats && (
          <section className="rounded-xl border border-line bg-panel px-6 py-5">
            <h3 className="mb-3 text-sm font-semibold text-muted">
              Statistik host
            </h3>
            <div className="grid grid-cols-2 gap-2.5 md:grid-cols-4">
              <StatChip label="FPS" value={stats.fps.toFixed(1)} tone="live" />
              <StatChip label="Encode" value={`${stats.encode_ms.toFixed(1)} ms`} />
              <StatChip label="Capture" value={`${stats.capture_ms.toFixed(1)} ms`} />
              <StatChip label="Frame dilewati" value={String(stats.frames_skipped)} />
              <StatChip label="Frame dikirim" value={String(stats.frames_encoded)} />
              <StatChip
                label="Input diterapkan"
                value={String(stats.input_events_applied)}
                tone="live"
              />
              <StatChip label="Input ditolak" value={String(stats.input_events_rejected)} />
              <StatChip
                label="Data keluar"
                value={`${(stats.bytes_sent / 1024).toFixed(0)} KiB`}
              />
            </div>
          </section>
        )}

        {error && (
          <div className="rounded-xl border border-danger/40 bg-danger-dim/40 px-5 py-4 text-sm text-red-200">
            <p className="mb-1.5 font-semibold">Terjadi kendala</p>
            <p className="leading-relaxed">{error}</p>
            <p className="mt-2 text-[12px] leading-relaxed text-red-200/70">
              Bila ini soal izin OS: buka System Settings → Privacy &amp;
              Security, izinkan Screen Recording dan Accessibility untuk
              aplikasi ini, lalu jalankan ulang.
            </p>
          </div>
        )}

        <div className="mt-auto flex justify-end pb-2">
          <Button
            variant="ghost"
            onClick={async () => {
              await hostEnd("host berhenti").catch(() => {});
              notify("info", "Host berhenti mendengarkan");
              onExit();
            }}
          >
            Berhenti mendengarkan
          </Button>
        </div>
      </div>
    </div>
  );
}
