import { useEffect, useState } from "react";
import { deviceInfo } from "../api";
import type { DeviceInfo } from "../types";
import { Button } from "./ui";

interface Props {
  notify: (tone: "ok" | "warn" | "info", message: string) => void;
  onHost: (serverUrl: string) => void;
  onConnect: (serverUrl: string, target: string, permissions: string[]) => void;
}

export default function HomeView({ notify, onHost, onConnect }: Props) {
  const [info, setInfo] = useState<DeviceInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [serverUrl, setServerUrl] = useState("http://localhost:8080");
  const [target, setTarget] = useState("");
  const [viewScreen, setViewScreen] = useState(true);
  const [controlInput, setControlInput] = useState(true);

  useEffect(() => {
    deviceInfo()
      .then(setInfo)
      .catch((e) => setError(String(e)));
  }, []);

  const permissions = [
    ...(viewScreen ? ["view_screen"] : []),
    ...(controlInput ? ["control_input"] : []),
  ];

  const deviceId = info?.device_id ?? "";
  // Tampilkan ID perangkat terpotong rapi: dev-1234abcd5678ef90
  const prettyId = deviceId.length > 12 ? deviceId.slice(0, 13) : deviceId;

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-3xl flex-col gap-6 px-6 py-8">
        {error && (
          <div className="rounded-lg border border-danger/40 bg-danger-dim/40 px-4 py-3 text-sm text-red-200">
            Gagal memuat identitas perangkat: {error}
          </div>
        )}

        {/* Pelat identitas — elemen utama halaman */}
        <section className="scanlines overflow-hidden rounded-xl border border-line-strong bg-panel">
          <div className="flex flex-wrap items-center gap-x-8 gap-y-4 px-6 py-5">
            <div className="min-w-0 flex-1">
              <p className="mb-1.5 text-[13px] text-muted">
                Device ID perangkat ini — bagikan ke pengendali untuk
                menghubungkan
              </p>
              <div className="flex flex-wrap items-center gap-3">
                <code className="select-all font-mono text-2xl font-medium tracking-tight text-brass">
                  {prettyId || "memuat…"}
                </code>
                <button
                  className="rounded-md border border-line-strong px-2.5 py-1 text-xs text-muted transition hover:border-brass/60 hover:text-ink"
                  onClick={() => {
                    if (!deviceId) return;
                    navigator.clipboard.writeText(deviceId);
                    notify("ok", "Device ID tersalin ke papan klip");
                  }}
                >
                  Salin
                </button>
              </div>
              <p className="mt-2 text-[11px] leading-relaxed text-muted">
                ID ini publik dan bukan bukti otorisasi. Kunci identitas
                tersimpan di keychain OS, token akses tidak pernah ditulis ke
                localStorage.
              </p>
            </div>
            <div className="flex flex-col gap-1.5 font-mono text-[11px] text-muted">
              <span className="rounded border border-line px-2 py-1 uppercase">
                {info?.platform ?? "…"}
              </span>
              <span
                className={`rounded border px-2 py-1 ${
                  info?.has_saved_token
                    ? "border-phosphor-dim text-phosphor"
                    : "border-line"
                }`}
              >
                token: {info?.has_saved_token ? "tersimpan" : "belum ada"}
              </span>
            </div>
          </div>
        </section>

        {/* Server signaling */}
        <section className="rounded-xl border border-line bg-panel px-6 py-5">
          <label className="mb-2 block text-sm font-medium" htmlFor="server">
            Server signaling
          </label>
          <input
            id="server"
            className="w-full rounded-lg border border-line bg-base px-4 py-2.5 font-mono text-sm text-ink outline-none transition focus:border-brass"
            value={serverUrl}
            onChange={(e) => setServerUrl(e.target.value)}
            placeholder="http://localhost:8080"
            spellCheck={false}
          />
          <p className="mt-2 text-[11px] text-muted">
            Jalankan <code className="font-mono text-brass">cargo run -p jenderalremote-signaling</code>{" "}
            untuk server lokal.
          </p>
        </section>

        {/* Dua tindakan utama */}
        <div className="grid gap-5 md:grid-cols-2">
          <section className="flex flex-col rounded-xl border border-line bg-panel p-6">
            <div className="mb-3 flex h-10 w-10 items-center justify-center rounded-lg border border-phosphor-dim bg-base">
              {/* Ikon monitor sederhana */}
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" aria-hidden>
                <rect x="3" y="4" width="18" height="13" rx="2" stroke="#7be382" strokeWidth="1.6" />
                <path d="M9 21h6M12 17v4" stroke="#7be382" strokeWidth="1.6" strokeLinecap="round" />
              </svg>
            </div>
            <h2 className="mb-1 text-[15px] font-semibold">Bagikan layar ini</h2>
            <p className="mb-5 flex-1 text-[13px] leading-relaxed text-muted">
              Dengarkan permintaan masuk. Setiap sesi attended menunggu
              persetujuan Anda; unattended nonaktif bawaan.
            </p>
            <Button onClick={() => onHost(serverUrl)}>Mulai mendengarkan</Button>
          </section>

          <section className="flex flex-col rounded-xl border border-line bg-panel p-6">
            <div className="mb-3 flex h-10 w-10 items-center justify-center rounded-lg border border-brass/40 bg-base">
              {/* Ikon bidik */}
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" aria-hidden>
                <circle cx="12" cy="12" r="7" stroke="#e3a857" strokeWidth="1.6" />
                <path d="M12 2v4M12 18v4M2 12h4M18 12h4" stroke="#e3a857" strokeWidth="1.6" strokeLinecap="round" />
                <circle cx="12" cy="12" r="1.6" fill="#e3a857" />
              </svg>
            </div>
            <h2 className="mb-1 text-[15px] font-semibold">
              Kendalikan perangkat lain
            </h2>
            <p className="mb-3 text-[13px] leading-relaxed text-muted">
              Masukkan Device ID tujuan, lalu minta izin kepada pengguna host.
            </p>
            <input
              className="mb-3 w-full rounded-lg border border-line bg-base px-4 py-2.5 font-mono text-sm text-ink outline-none transition placeholder:text-muted/50 placeholder:font-display focus:border-brass"
              value={target}
              onChange={(e) => setTarget(e.target.value)}
              placeholder="dev-…"
              spellCheck={false}
            />
            <div className="mb-4 flex flex-wrap gap-4 text-[13px]">
              <label className="flex cursor-pointer items-center gap-2">
                <input
                  type="checkbox"
                  checked={viewScreen}
                  onChange={(e) => setViewScreen(e.target.checked)}
                  className="h-4 w-4 accent-[#e3a857]"
                />
                Lihat layar
              </label>
              <label className="flex cursor-pointer items-center gap-2">
                <input
                  type="checkbox"
                  checked={controlInput}
                  onChange={(e) => setControlInput(e.target.checked)}
                  className="h-4 w-4 accent-[#e3a857]"
                />
                Kendalikan input
              </label>
            </div>
            <Button
              disabled={!target.trim() || !viewScreen}
              className="mt-auto"
              onClick={() => {
                onConnect(serverUrl, target.trim(), permissions);
                notify("info", `Meminta sesi ke ${target.trim().slice(0, 16)}…`);
              }}
            >
              Sambungkan
            </Button>
          </section>
        </div>

        <p className="pb-2 text-center text-[11px] leading-relaxed text-muted">
          Video berjalan lewat DataChannel SCTP (ADR-001), input hanya aktif
          dengan izin control_input. Bila izin Screen Recording / Accessibility
          OS belum diberikan, aplikasi menjelaskan — bukan memaksa.
        </p>
      </div>
    </div>
  );
}
