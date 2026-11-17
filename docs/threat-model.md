# Threat Model (status implementasi)

Tabel dari rancangan teknis §11.3, dipetakan ke status implementasi saat ini.

| Ancaman | Mitigasi dirancang | Status |
| --- | --- | --- |
| Menebak Device ID | ID publik, bukan otorisasi; semua aksi butuh token | ✅ server hanya percaya token koneksi |
| Session hijacking | Token dirotasi tiap register; hash-only di server | ✅ dasar; MFA & expiry sesi = fase 3 |
| Replay pesan | `request_id` + validasi state machine + status sesi di server | ✅ `rtc.*` hanya pada fase valid |
| Penyalahgunaan unattended | Opt-in eksplisit, trusted controller, expiry, revocation | ✅ policy engine deny-by-default; UI opt-in = fase 3 |
| Input tanpa izin | Gate di host sebelum injeksi | ✅ `input_allowed()` (state + permission) per event |
| Kebocoran secret | OS keychain, hash token, redaksi audit | ✅; fallback file 0600 hanya bila keychain absen (diberi peringatan) |
| Serangan signaling | Rate limit + schema + batas ukuran | ✅ token bucket 30 burst/10 rps + validasi envelope |
| Penyalahgunaan TURN | Kredensial sementara + kuota | ⚠️ konfigurasi dev statis (`infra/coturn`) — produksi wajib `use-auth-secret` |
| Update berbahaya | Signed binary + verifikasi | ❌ fase 4 (CI artifact belum ditandatangani) |
| Dependency rentan | Audit + lockfile | ✅ `cargo audit`/`cargo deny` di CI (non-blocking); lockfile dikomit |
| Pembajakan device ID | Public key mismatch ditolak | ✅ `register_rejected` |
| Token di log | Redaksi `«redacted»` + audit tanpa payload sensitif | ✅ |

## Sisa risiko yang diterima sementara (dev build)

1. **Registrasi terbuka**: siapa pun bisa `device.register` di server dev.
   Mitigasi produksi: reverse proxy TLS + autentikasi akun (fase 2) —
   jangan ekspos binary dev ke internet.
2. **Video tidak dienkripsi end-to-end tambahan** — melewati DTLS/SCTP
   bawaan WebRTC (standar), tetapi belum ada verifikasi fingerprint sertifikat
   peer terhadap `public_key` perangkat (rencana: menyertakan fingerprint di
   payload approve).
3. **Server dev tanpa TLS** — `ws://` lokal; staging wajib `wss://` via
   reverse proxy.
4. **Kehilangan frame** pada congestion hanya menurunkan FPS (by design,
   unreliable channel), tidak mengorbankan input (channel reliable).

## Respons insiden (§11.4) — yang bisa dilakukan hari ini

- Cabut perangkat: matikan host/engine (device.remove lokal) — penghapusan
  sisi server menunggu persistence (fase 2).
- Cabut sesi aktif: host End, atau server `session.revoke`.
- Audit: log terstruktur `jenderalremote::audit` di server & host.
