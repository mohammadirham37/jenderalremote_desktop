# JenderalRemote Desktop

Aplikasi remote desktop lintas platform (Windows, macOS, Linux) dengan engine
**Rust**, media real-time **WebRTC**, dan UI desktop **Tauri 2 + React**.
Implementasi mengikuti `remote-desktop-rancangan-teknis.md`: keamanan sejak
desain, pemisahan control plane dan media plane, serta attended access dan
unattended access terotorisasi.

> Status: **MVP fase 0–1** — jalur video end-to-end, input control dengan
> policy gate, signaling server, dan audit event sudah berjalan dan lulus
> [test integrasi e2e](services/signaling/tests/e2e.rs). Lihat
> [docs/architecture.md](docs/architecture.md) untuk batasan dan fase
> berikutnya.

## Ringkasan komponen

| Komponen | Teknologi | Peran |
| --- | --- | --- |
| `apps/desktop` | Tauri 2, React 19, TypeScript, Vite, Tailwind 4 | UI host & controller |
| `crates/remote-protocol` | Serde JSON | Envelope signaling, framing media, validasi versi/ukuran |
| `crates/remote-core` | — | State machine sesi + permission gating (tanpa Tauri/OS) |
| `crates/remote-security` | Ed25519, keyring | Identitas perangkat, token, policy unattended, audit |
| `crates/remote-webrtc` | webrtc-rs, tokio-tungstenite | Host/controller engine, DataChannel, signaling client |
| `crates/remote-capture` | xcap (feature) | Interface capture + stub deterministik + backend native |
| `crates/remote-input` | enigo (feature) | Interface input + stub + injeksi native (CGEvent/SendInput/XTest) |
| `crates/remote-telemetry` | tracing | Log terstruktur + metrik sesi |
| `services/signaling` | Axum, WebSocket | Registry perangkat, sesi, relay SDP/ICE, rate limit, audit |
| `infra/` | Docker, Coturn, Prometheus | Lingkungan dev lengkap |

## Menjalankan (development)

Prasyarat: Rust stable (1.85+), Node 22+; untuk Linux tambahkan
libwebkit2gtk-4.1, libgtk-3, librsvg, libX11, libXtst (lihat
[ci.yml](.github/workflows/ci.yml) untuk daftar paket persis).

```bash
# 1. Server signaling (terminal 1)
cargo run -p jenderalremote-signaling
# mendengarkan di ws://127.0.0.1:8080/ws

# 2. Aplikasi desktop (terminal 2)
cd apps/desktop
npm install
npm run tauri dev
```

Dua instance di dua perangkat (atau dua mesin berbeda) dapat saling
menyambung: satu memilih “Bagikan layar ini”, satu lagi memasukkan Device ID
host. Untuk lingkungan server lengkap (PostgreSQL, Redis, Coturn, Prometheus):

```bash
docker compose -f infra/compose/docker-compose.yml up --build
```

### Izin OS (tidak bisa dilewati, by design)

- **macOS**: Screen Recording untuk capture, Accessibility untuk input —
  berikan di System Settings → Privacy & Security, lalu jalankan ulang.
- **Windows**: jalankan dalam sesi pengguna interaktif.
- **Linux**: X11 untuk MVP ini; Wayland menunguh backend portal PipeWire
  (lihat [ADR-002](docs/adr/0002-capture-input-phased.md)).

## Menjalankan via GitHub (tanpa compile lokal)

Repositori ini membangun semuanya di GitHub Actions:

- **[ci.yml](.github/workflows/ci.yml)** — setiap push/PR: `rustfmt`,
  `clippy -D warnings`, `cargo test --workspace` pada Ubuntu, Windows, dan
  macOS, build frontend (tsc + vite), serta audit dependency.
- **[build-desktop.yml](.github/workflows/build-desktop.yml)** — manual
  (tab Actions → *Build Desktop* → Run workflow) atau saat tag `v*`:
  menghasilkan `.dmg` (Apple Silicon & Intel), `.msi/.exe` Windows,
  `.deb`/`.AppImage` Linux, plus binary signaling Linux. Artifact muncul di
  halaman run untuk diunduh.

Artinya Anda tidak perlu toolchain lokal sama sekali untuk mendapatkan
aplikasi jadi — cukup push, tunggu workflow selesai, unduh artifact.

## Testing

```bash
cargo test --workspace        # unit + integrasi + e2e (server in-proc + WebRTC loopback)
cargo clippy --workspace --all-targets
cd apps/desktop && npm run build   # type-check + bundle frontend
```

Test penting: `services/signaling/tests/e2e.rs` — dua endpoint lengkap
melalui signaling server nyata: request → approval → ICE/DTLS/SCTP → video
frame diterima controller → input diterapkan host → teardown.

## Struktur repository

```
├── apps/desktop/          # Tauri 2 (src-tauri) + React/TS
├── crates/
│   ├── remote-protocol/   # tipe pesan + versi protokol (tanpa OS dep)
│   ├── remote-core/       # state machine sesi (tanpa Tauri)
│   ├── remote-security/   # identitas, token, policy, audit
│   ├── remote-telemetry/  # tracing + metrik
│   ├── remote-capture/    # trait capture + stub + xcap
│   ├── remote-input/      # trait input + stub + enigo
│   └── remote-webrtc/     # engine host/controller + signaling client
├── services/signaling/    # Axum + WebSocket control plane
├── infra/                 # compose, coturn, monitoring, Dockerfile
├── docs/                  # arsitektur, protokol, threat model, ops, ADR
├── scripts/               # utilitas (generator ikon)
└── .github/workflows/     # CI + build artifacts per OS
```

## Keamanan — apa yang sudah ada dan apa yang belum

Sudah: identitas perangkat Ed25519 (private key di OS keychain), device token
hash-only di server, envelope ber-version+request_id+validasi ukuran,
otorisasi identitas hanya dari koneksi terautentikasi (field identitas client
diabaikan), policy gate input di host (state machine + permission),
deny-by-default unattended, rate limiting, audit log tanpa secret, host hanya
bisa mempersempit izin, release-all keys saat sesi berakhir.

Belum (fase berikutnya, lihat [docs/threat-model.md](docs/threat-model.md)):
persistensi PostgreSQL, MFA, kredensial TURN sementara, signed update, dan
bundle app yang ditandatangani. **Server dev menerima registrasi terbuka —
jangan ekspos ke internet publik tanpa reverse proxy TLS + autentikasi.**

## Lisensi

MIT — lihat [LICENSE](LICENSE).
