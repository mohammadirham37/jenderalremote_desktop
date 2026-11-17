import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  controllerConnect,
  controllerEnd,
  controllerKeyboard,
  controllerMouse,
} from "../api";
import type { EngineBoot } from "../App";
import type { ControllerEventDto } from "../types";
import { Button, ConnectionSteps } from "./ui";

export default function ControllerView({
  boot,
  notify,
  onExit,
}: {
  boot: EngineBoot;
  notify: (tone: "ok" | "warn" | "info", message: string) => void;
  onExit: () => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const bitmapRef = useRef<ImageBitmap | null>(null);
  const frameShownRef = useRef(false);
  const lastMoveRef = useRef(0);

  const [status, setStatus] = useState("requesting");
  const [error, setError] = useState<string | null>(null);
  const [stats, setStats] = useState<{
    host_fps: number;
    host_encode_ms: number;
    rtt_ms: number;
  } | null>(null);
  const [hasFrame, setHasFrame] = useState(false);
  const [ended, setEnded] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    let unlisten: UnlistenFn | null = null;

    listen<ControllerEventDto>("controller://event", (event) => {
      if (!mounted) return;
      const payload = event.payload;
      switch (payload.kind) {
        case "token_issued":
          break;
        case "state_changed":
          setStatus(payload.status);
          if (payload.status === "active") {
            notify("ok", "Sesi aktif — video mengalir");
          }
          break;
        case "frame": {
          const canvas = canvasRef.current;
          if (!canvas) return;
          const bytes = base64ToBytes(payload.jpeg_base64);
          createImageBitmap(new Blob([bytes], { type: "image/jpeg" }))
            .then((bitmap) => {
              bitmapRef.current?.close();
              bitmapRef.current = bitmap;
              if (canvas.width !== payload.width) canvas.width = payload.width;
              if (canvas.height !== payload.height)
                canvas.height = payload.height;
              const ctx = canvas.getContext("2d");
              if (ctx && bitmapRef.current) {
                ctx.drawImage(bitmapRef.current, 0, 0);
              }
              if (!frameShownRef.current) {
                frameShownRef.current = true;
                setHasFrame(true);
              }
            })
            .catch(() => {});
          break;
        }
        case "stats":
          setStats({
            host_fps: payload.host_fps,
            host_encode_ms: payload.host_encode_ms,
            rtt_ms: payload.rtt_ms,
          });
          break;
        case "ended":
          setEnded(payload.reason ?? "berakhir");
          notify("warn", `Sesi berakhir: ${payload.reason ?? "selesai"}`);
          break;
        case "error":
          setError(payload.message);
          break;
      }
    }).then((fn) => {
      unlisten = fn;
    });

    controllerConnect(
      boot.serverUrl,
      boot.targetDeviceId ?? "",
      "attended",
      boot.permissions && boot.permissions.length > 0
        ? boot.permissions
        : ["view_screen", "control_input"],
    ).catch((e) => setError(String(e)));

    return () => {
      mounted = false;
      unlisten?.();
      bitmapRef.current?.close();
      controllerEnd("window closed").catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---- Input: koordinat ternormalisasi, move dibatasi ~30 Hz ----

  const normalized = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current!;
    const rect = canvas.getBoundingClientRect();
    return {
      nx: (e.clientX - rect.left) / Math.max(rect.width, 1),
      ny: (e.clientY - rect.top) / Math.max(rect.height, 1),
    };
  };

  const onMouseDown = (e: React.MouseEvent<HTMLCanvasElement>) => {
    e.preventDefault();
    canvasRef.current?.focus();
    const { nx, ny } = normalized(e);
    controllerMouse(nx, ny, "press", buttonOf(e.button), 0, 0).catch(() => {});
  };

  const onMouseUp = (e: React.MouseEvent<HTMLCanvasElement>) => {
    e.preventDefault();
    const { nx, ny } = normalized(e);
    controllerMouse(nx, ny, "release", buttonOf(e.button), 0, 0).catch(() => {});
  };

  const onMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const now = performance.now();
    if (now - lastMoveRef.current < 33) return;
    lastMoveRef.current = now;
    const { nx, ny } = normalized(e);
    controllerMouse(nx, ny, "move", "left", 0, 0).catch(() => {});
  };

  const onWheel = (e: React.WheelEvent<HTMLCanvasElement>) => {
    e.preventDefault();
    const { nx, ny } = normalized(
      e as unknown as React.MouseEvent<HTMLCanvasElement>,
    );
    controllerMouse(nx, ny, "scroll", "left", Math.sign(e.deltaX), Math.sign(e.deltaY)).catch(
      () => {},
    );
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.repeat) return;
    e.preventDefault();
    controllerKeyboard("press", e.key, modifiersOf(e)).catch(() => {});
  };

  const onKeyUp = (e: React.KeyboardEvent<HTMLDivElement>) => {
    e.preventDefault();
    controllerKeyboard("release", e.key, modifiersOf(e)).catch(() => {});
  };

  const showCanvas = hasFrame && !ended;

  return (
    <div className="flex h-full flex-col">
      {/* Bar status mengambang di atas video */}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-line bg-panel/80 px-5 py-2.5 backdrop-blur">
        <ConnectionSteps status={ended ? "ended" : status} />
        {stats && (
          <span className="font-mono text-[11px] text-muted">
            {stats.host_fps.toFixed(1)} fps · enc {stats.host_encode_ms.toFixed(0)} ms
            {stats.rtt_ms > 0 && ` · rtt ${stats.rtt_ms} ms`}
          </span>
        )}
        <Button
          variant="ghost"
          className="ml-auto !px-3 !py-1.5 !text-xs"
          onClick={async () => {
            await controllerEnd("controller menutup").catch(() => {});
            notify("info", "Sesi diputus");
            onExit();
          }}
        >
          Putuskan
        </Button>
      </div>

      {error && (
        <div className="border-b border-danger/40 bg-danger-dim/50 px-5 py-2.5 text-[13px] text-red-200">
          {error}
        </div>
      )}

      <div
        className="min-h-0 flex-1 outline-none"
        tabIndex={0}
        onKeyDown={onKeyDown}
        onKeyUp={onKeyUp}
      >
        {showCanvas ? (
          <div className="flex h-full items-center justify-center bg-black p-3">
            <canvas
              ref={canvasRef}
              className="max-h-full max-w-full rounded-lg border border-line shadow-2xl shadow-black/60 cursor-crosshair"
              onMouseDown={onMouseDown}
              onMouseUp={onMouseUp}
              onMouseMove={onMouseMove}
              onWheel={onWheel}
              tabIndex={0}
              aria-label="Layar host yang dikendalikan"
            />
          </div>
        ) : (
          <div className="flex h-full flex-col items-center justify-center gap-4 px-6 text-center">
            <div
              className="flex h-14 w-14 items-center justify-center rounded-full border border-line-strong"
              aria-hidden
            >
              <span
                className={`h-3 w-3 rounded-full ${
                  ended
                    ? "bg-danger"
                    : status === "awaiting_approval"
                      ? "animate-pulse bg-brass"
                      : "animate-pulse bg-phosphor"
                }`}
              />
            </div>
            <div>
              <p className="text-[15px] font-semibold">
                {ended
                  ? `Sesi berakhir — ${ended}`
                  : status === "awaiting_approval"
                    ? "Menunggu persetujuan di sisi host…"
                    : "Menyiapkan koneksi…"}
              </p>
              <p className="mt-1 max-w-sm text-[13px] leading-relaxed text-muted">
                {ended
                  ? "Kembali ke beranda untuk menyambung kembali."
                  : "Video tampil otomatis begitu host mengizinkan dan WebRTC tersambung. Klik kanvas untuk mulai mengendalikan mouse dan keyboard."}
              </p>
            </div>
            {ended && (
              <Button variant="ghost" onClick={onExit}>
                Kembali ke beranda
              </Button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function buttonOf(button: number): "left" | "right" | "middle" {
  if (button === 2) return "right";
  if (button === 1) return "middle";
  return "left";
}

function modifiersOf(e: React.KeyboardEvent): string[] {
  const modifiers: string[] = [];
  if (e.ctrlKey) modifiers.push("ctrl");
  if (e.altKey) modifiers.push("alt");
  if (e.shiftKey) modifiers.push("shift");
  if (e.metaKey) modifiers.push("meta");
  return modifiers;
}

function base64ToBytes(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}
