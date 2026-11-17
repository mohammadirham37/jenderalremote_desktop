# ADR-0002: Backend capture/input bertahap (feature flag, bukan crate kosong)

- Status: diterima (fase 0–1)
- Tanggal: 2026-11-17
- Konteks: rancangan teknis §5.1 & §9 menggambarkan crate per-OS
  (`capture-macos`, `input-linux`, …) tetapi juga memperingatkan: “jangan
  membuat seluruh crate kosong di awal”.

## Keputusan

1. Interface tetap di `remote-capture`/`remote-input` (trait + error +
   normalisasi).
2. Backend native fase ini: **xcap** (WGC/CoreGraphics/X11) dan **enigo**
   (SendInput/CGEvent/XTest) melalui feature flag — build default tetap
   stub-only sehingga CI tanpa display tetap menguji seluruh pipeline.
3. Crate platform spesifik (`capture-macos` ScreenCaptureKit,
  `capture-linux` portal PipeWire, `input-linux` libei) dibuat saat teknologi
   itu benar-benar digarap — mengikuti aturan “crate tumbuh saat batas
   tanggung jawab jelas”.

## Alasan

- xcap/enigo membungkus API native yang tepat (bukan hack), aktif
  dipelihara, dan menutup tiga OS sekaligus untuk spike.
- Wayland memang butuh portal — diputuskan terpisah saat diimplementasikan,
  bukan disimulasikan.

## Konsekuensi

- macOS: butuh izin Screen Recording & Accessibility (dijelaskan di UI,
  tidak dilewati — sesuai §6.2).
- Linux: X11 saja di fase ini; Wayland ditolak dengan pesan jelas.
- Pergantian backend tidak menyentuh engine: hanya implementasi trait.
