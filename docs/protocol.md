# Protokol

## Envelope signaling (§7.1)

Setiap pesan WebSocket adalah JSON:

```json
{
  "version": 1,
  "type": "session.request",
  "request_id": "req_<hex>",
  "session_id": "sess_<uuid>",
  "payload": { }
}
```

Validasi diterapkan di `Envelope::from_bytes` sebelum routing:
ukuran maksimum **256 KiB**, versi harus didukung, `request_id` wajib
non-kosong, `type` harus dikenal. Payload dicek ulang per-tipe di
`Envelope::payload_typed()`.

## Jenis pesan (§7.2 + tambahan)

| `type` | Arah | Payload |
| --- | --- | --- |
| `device.register` | client → server | `device_id, display_name, platform, public_key` (SPKI base64) |
| `device.registered` | server → client | `device_id, device_token` (rotasi token tiap register) |
| `device.heartbeat` | client → server | `status` |
| `device.remove` | client → server | `device_id` (MVP: hanya diri sendiri) |
| `session.request` | controller → server → host | `target_device_id, controller_device_id (diisi server), mode, permissions` |
| `session.approve` / `session.reject` | host → server → controller | `approved, reason, granted_permissions` |
| `session.revoke` | host → server → controller | `reason` |
| `session.state` | server → client | `status, detail` |
| `session.end` | kedua pihak | `reason` |
| `rtc.offer` / `rtc.answer` | host ↔ controller (relay) | `sdp` |
| `rtc.ice_candidate` | host ↔ controller (relay) | `candidate, sdp_mid, sdp_mline_index` |
| `error` | server → client | `code, message` |

Aturan routing server: identitas pengirim selalu dari token koneksi;
`session.request` yang menyertakan `controller_device_id` dari client
**diabaikan dan ditimpa**; `rtc.*` hanya untuk peserta sesi dan hanya saat
fase negotiating/connecting; keputusan hanya dari device host sesi;
`session.end` dari peserta mana pun.

Kode error server: `unauthorized`, `rate_limited`, `invalid_envelope`,
`unknown_type`, `invalid_payload`, `target_offline`, `invalid_target`,
`invalid_permissions`, `unknown_session`, `missing_session`, `forbidden`,
`invalid_state`, `identity_mismatch`, `register_rejected`.

## Permission (§8.3)

Enum: `view_screen` (wajib setiap sesi), `control_input`, `clipboard`,
`audio`. Validasi request menolak set tanpa `view_screen`. Host boleh
mempersempit saat approve; engine host mengecek ulang `input_allowed()`
untuk setiap event input.

## Media plane (DataChannel, fase 0 — lihat ADR-001)

- `control` (label `"control"`, ordered + reliable), payload JSON text:
  - `{"type":"mouse", "event":{action, button, x, y, scroll_dx, scroll_dy}}`
    — koordinat piksel layar host.
  - `{"type":"keyboard","event":{action, key, modifiers[]}}` — `key` satu
    karakter unicode atau nama kunci (`Enter`, `ArrowLeft`, …).
  - `{"type":"ping","sent_ms":n}` → host membalas `{"type":"pong","sent_ms":n}`
    (RTT).
  - `{"type":"stats",...}` — fps/encode/capture/skipped dari host.
- `video` (label `"video"`, unordered, maxRetransmits 0), biner:
  `[0x01][frame_id u32 BE][chunk_index u16][total_chunks u16][w u16][h u16][jpeg…]`
  — payload chunk maksimum 60.000 byte. Reassembler membuang frame usang
  (frame_id monotonik), sehingga kehilangan chunk hanya membuang satu frame.
- `clipboard` (ordered + reliable) disiapkan untuk fase berikutnya.

## Auth koneksi

WebSocket handshake membawa `Authorization: Bearer <device_token>`. Token
dirotasi setiap `device.register`; server menyimpan hanya SHA-256 hash.
Device ID yang mendaftar ulang dengan public key berbeda ditolak
(`register_rejected`) — mencegah pembajakan ID.
