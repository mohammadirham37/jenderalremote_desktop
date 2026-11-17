# ADR-0001: Video lewat DataChannel SCTP untuk fase 0/1

- Status: diterima (fase 0–1)
- Tanggal: 2026-11-17
- Konteks: rancangan teknis §7.3/§7.4 menargetkan video track RTP + codec
  teruji; §12 menuntut *satu jalur video end-to-end* terbukti dulu.

## Keputusan

Video MVP berjalan sebagai JPEG ber-chunk di DataChannel SCTP `video`
(unordered, maxRetransmits 0), bukan RTP/VP8.

## Alasan

1. **Deterministik lintas CI**: encoder JPEG pure-Rust (crate `image`)
   kompilasi di ketiga OS tanpa system codec (libvpx/VA-API/VideoToolbox),
   sehingga jalur “compile di GitHub” untuk tiga OS tetap hijau.
2. **webrtc-rs tidak membundel decoder**: jalur RTP menuntut depacketizer +
   dekoder VP8 (libvpx) yang berat di Windows dan butuh lib sistem di Linux.
3. SCTP/DTLS/ICE tetap 100% asli — yang diuji (NAT, relay, keamanan
   transport) tidak berkurang; hanya payload yang diganti.

## Konsekuensi

- Bandwidth lebih besar dari VP8 pada gerakan kompleks; mitigasi: kualitas
  JPEG adaptif (konfigurasi), backpressure buffer 2 MiB, frame usang dibuang
  di reassembler.
- Input TIDAK terdampak: kanal `control` ordered+reliable terpisah (§7.3).
- Kriteria keluar (fase 1 lanjut): integrasi `TrackLocalStaticSample` +
  VP8/software encoder pertama, lalu hardware (VideoToolbox/VA-API/MF),
  lalu benchmark LAN/WAN sesuai §13.3 sebelum default diganti.

## Pemulihan

Interface `ScreenCapture` dan framing sudah dipisah dari transport; migrasi
ke RTP menyentuh `remote-webrtc::{host,controller}` saja.
