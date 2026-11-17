# Arsitektur

Dokumen ini memetakan implementasi ke rancangan teknis, mencatat keputusan,
dan menandai apa yang belum ada. Rujukan nomor bagian mengacu ke
`remote-desktop-rancangan-teknis.md`.

## Peta lapisan

```
┌──────────────────────────────────────────────────────────────┐
│ apps/desktop (Tauri 2 + React)                               │
│  commands.rs → satu-satunya jembatan UI → engine             │
├──────────────────────────────────────────────────────────────┤
│ remote-webrtc                                                │
│  HostEngine / ControllerEngine                               │
│  ├─ remote-core    : state machine sesi + permission gate    │
│  ├─ remote-security: identitas, policy, audit                │
│  ├─ remote-capture : frame (stub / xcap native)              │
│  ├─ remote-input   : injeksi (stub / enigo native)           │
│  └─ remote-protocol: envelope + framing                      │
├──────────────────────────────────────────────────────────────┤
│ services/signaling (Axum + WSS)                              │
│  registry device, sesi, relay SDP/ICE, rate limit, audit     │
├──────────────────────────────────────────────────────────────┤
│ infra: PostgreSQL (rencana), Redis (rencana), Coturn, Prometheus │
└──────────────────────────────────────────────────────────────┘
```

## Aturan dependensi (dipenuhi)

- `remote-core` tidak tergantung Tauri/OS — hanya `remote-protocol`.
- `remote-protocol` tanpa dependensi OS — serde/thiserror saja.
- `remote-capture`/`remote-input` mendefinisikan interface; backend native
  datang lewat feature (`xcap`, `enigo`) — crate platform terpisah
  (`capture-macos` ScreenCaptureKit, `capture-linux` portal) menyusul
  sesuai [ADR-002](adr/0002-capture-input-phased.md).
- `remote-webrtc` hanya transport; keputusan izin selalu lewat
  `remote-core` + `remote-security`.
- UI tidak pernah memanggil native langsung — hanya command tervalidasi
  (`apps/desktop/src-tauri/src/commands.rs`).

## Alur attended (§8.1) — yang diimplementasikan

1. Controller & host register via `device.register` → server rotasi token →
   `device.registered` (disimpan client di OS keychain).
2. Controller `session.request` → server *menyunting* payload (identitas
   controller dari koneksi, bukan dari client) → diteruskan ke host →
   `session.state awaiting_approval` ke controller.
3. Host UI menampilkan dialog → `session.approve/reject` (host hanya bisa
   mempersempit izin yang diminta).
4. Host membuat PeerConnection (offerer) + DataChannel `control`
   (ordered/reliable) & `video` (unordered, maxRetransmits 0) → `rtc.offer`
   → controller `rtc.answer` + trickle `rtc.ice_candidate` dua arah.
5. State machine host: `Idle→AwaitingApproval→Negotiating→Connecting→Active`;
   capture loop hidup hanya saat `Active`.
6. Input dari controller hanya diterapkan bila
   `SessionStateMachine::input_allowed()` (Active **dan** `control_input`).
7. `session.end`/putus koneksi → stop capture, `release_all` input, audit,
   state `Ended`.

Unattended: policy engine `HostAccessPolicy` sudah memvalidasi
trusted-controller + expiry + subset permission, dan host auto-reject dengan
alasan bila tidak lolos; UI untuk *mengaktifkan* unattended (opt-in pemilik +
MFA) adalah fase 3.

## Media plane (§7.3, §7.4) — keputusan fase 0

Video: capture RGBA → encode JPEG (pure Rust) → chunk ≤60 KiB → SCTP
DataChannel unordered → reassembler drop-stale di controller → base64 ke
canvas. Input/stats/ping-pong: JSON di DataChannel `control`. Alasannya dan
rencana migrasi ke RTP/VP8 + hardware encoding ada di
[ADR-001](adr/0001-video-over-datachannel.md).

## Observability

- Engine: `tracing` + `SessionMetrics` (fps, encode/capture ms EMA, bytes,
  input applied/rejected, RTT ping-pong 2 s).
- Signaling: log JSON (env `LOG_JSON=1`), `/metrics` format Prometheus
  (koneksi, pesan, rejection, sesi), audit event terstruktur.

## Batasan fase ini (jujur)

- Server in-memory; restart menghapus registry (PostgreSQL = fase 2).
- TURN belum dipakai engine (ICE server STUN default; konfigurasi `EngineConfig`
  sudah menerima TURN, uji NAT = fase 4).
- Linux Wayland belum didukung (X11 via xcap).
- Clipboard channel disiapkan (label `clipboard`) tapi belum ada trafik.
- CI headless tidak menguji izin Screen Recording/Accessibility nyata —
  butuh runner dengan display & izin (§13.6).
