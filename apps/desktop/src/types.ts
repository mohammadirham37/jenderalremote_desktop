// Bentuk payload event dari backend (lihat src-tauri/src/state.rs).

export interface DeviceInfo {
  device_id: string;
  platform: string;
  has_saved_token: boolean;
}

export interface MetricsSnapshot {
  frames_captured: number;
  frames_encoded: number;
  frames_skipped: number;
  bytes_sent: number;
  input_events_received: number;
  input_events_applied: number;
  input_events_rejected: number;
  encode_errors: number;
  fps: number;
  encode_ms: number;
  capture_ms: number;
  rtt_ms: number;
}

export type HostEventDto =
  | { kind: "token_issued"; token: string }
  | {
      kind: "session_request";
      session_id: string;
      controller_device_id: string;
      mode: "attended" | "unattended";
      permissions: string[];
    }
  | { kind: "state_changed"; status: string; detail: string | null }
  | { kind: "stats"; snapshot: MetricsSnapshot }
  | { kind: "session_ended"; reason: string | null }
  | { kind: "error"; message: string };

export type ControllerEventDto =
  | { kind: "token_issued"; token: string }
  | { kind: "state_changed"; status: string; detail: string | null }
  | {
      kind: "frame";
      frame_id: number;
      width: number;
      height: number;
      jpeg_base64: string;
    }
  | {
      kind: "stats";
      host_fps: number;
      host_encode_ms: number;
      host_capture_ms: number;
      host_frames_skipped: number;
      rtt_ms: number;
    }
  | { kind: "ended"; reason: string | null }
  | { kind: "error"; message: string };

export const STATUS_LABEL: Record<string, string> = {
  idle: "Siaga",
  requesting: "Meminta sesi…",
  awaiting_approval: "Menunggu persetujuan host…",
  negotiating: "Negosiasi koneksi…",
  connecting: "Menghubungkan…",
  active: "Sesi aktif",
  ending: "Mengakhiri…",
  ended: "Berakhir",
  rejected: "Ditolak host",
  failed: "Gagal",
  revoked: "Dicabut",
  disconnected: "Terputus",
  reconnecting: "Menyambung ulang…",
};
