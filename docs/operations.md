# Operasional (§15)

## Lingkungan

### Development

```bash
cargo run -p jenderalremote-signaling        # ws://127.0.0.1:8080/ws
cd apps/desktop && npm run tauri dev         # aplikasi desktop
docker compose -f infra/compose/docker-compose.yml up --build   # + PG/Redis/Coturn/Prometheus
```

Endpoint signaling: `GET /health`, `GET /metrics` (Prometheus),
`GET /api/devices` (perangkat online, tanpa secret), `GET /ws`.

### Staging/produksi (rencana konkrit)

- Jalankan signaling di belakang reverse proxy TLS (Caddy/nginx) —
  `wss://` ke klien, header `Authorization` diteruskan apa adanya.
- `LOG_JSON=1` + kirim ke agregator log; scrape `/metrics`.
- Coturn produksi: matikan user statis, pakai `use-auth-secret` dengan
  `static-auth-secret` dari secret manager; buka 3478/udp+tcp dan rentang
  `min-port..max-port` di firewall.
- PostgreSQL/Redis aktif saat fitur persistence hadir (fase 2) — skema
  konseptual ada di §10 rancangan; Redis tidak pernah sumber kebenaran izin.
- Backup PostgreSQL harian + retensi audit terpisah (§10.6).

## Metrik yang tersedia

`signaling_ws_connections_total`, `signaling_messages_total`,
`signaling_messages_rejected_total`, `signaling_registered_devices_total`,
`signaling_sessions_created_total`, `signaling_sessions_ended_total`.
Engine host menampilkan fps/encode/capture/skip/input applied-rejected dan
RTT (ping/pong 2 detik) di UI.

## Runbook singkat

| Gejala | Periksa |
| --- | --- |
| Klien tak bisa connect | `/health`, log `connect` di engine, token di keychain |
| Controller menunggu approval terus | host online? lihat `GET /api/devices`; audit `session.request` |
| Video tidak tampil | izin Screen Recording host; `HostEvent::Error`; fps=0 di stats |
| Input tidak jalan | izin Accessibility host; `input_events_rejected` naik → permission tidak diberikan |
| Sesi langsung putus | `connection_lost` → NAT/TURN (fase 4), cek `rtc.*` rejection di log server |

## Kill switch

- Host: tombol “Berhenti mendengarkan” / End sesi (selalu tersedia, §8.1).
- Server: hentikan proses / blokir `device.register` di proxy saat insiden.
