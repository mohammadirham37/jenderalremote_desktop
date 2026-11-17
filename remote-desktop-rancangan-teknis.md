# Rancangan Teknis Aplikasi Remote Desktop

## Engine Rust + WebRTC \| Windows, macOS, Linux

**Status dokumen:** Rancangan teknis awal\
**Tujuan:** Menjadi acuan implementasi aplikasi remote desktop yang
mendukung attended access dan unattended access terotorisasi.\
**Prinsip utama:** keamanan sejak desain, pemisahan control plane dan
media plane, engine yang tidak terikat UI, serta implementasi native per
sistem operasi.

------------------------------------------------------------------------

## 1. Ringkasan Eksekutif

Aplikasi ini dirancang sebagai produk remote desktop lintas platform
dengan komponen:

-   **Desktop UI:** Tauri 2, React, TypeScript, Vite, dan Tailwind CSS.
-   **Remote engine:** Rust, Tokio, dan modul native untuk screen
    capture serta input control.
-   **Media real-time:** WebRTC, dengan koneksi peer-to-peer bila
    memungkinkan dan TURN relay sebagai fallback.
-   **Signaling/control plane:** Rust + Axum + WebSocket Secure.
-   **Data:** PostgreSQL untuk data persisten dan Redis untuk presence,
    cache, serta rate limiting.
-   **Relay:** Coturn.
-   **Keamanan:** identitas perangkat, autentikasi akun, otorisasi sesi,
    persetujuan host, kredensial yang dapat dicabut, audit log, dan
    update bertanda tangan.

### Tujuan MVP

1.  Menghubungkan dua perangkat melalui Device ID.
2.  Menampilkan layar host secara real-time.
3.  Mengendalikan mouse dan keyboard jika diizinkan.
4.  Mendukung attended access dengan persetujuan pengguna host.
5.  Mendukung unattended access yang diaktifkan secara eksplisit dan
    dapat dicabut.
6.  Menggunakan P2P dan fallback TURN.
7.  Menyediakan build awal untuk Windows, macOS, dan Linux.
8.  Mencatat peristiwa keamanan dan sesi.

### Di luar cakupan MVP

-   Akses administratif atau eksekusi perintah shell jarak jauh.
-   Remote wake-on-LAN.
-   File transfer besar.
-   Rekaman sesi.
-   Kolaborasi multi-controller.
-   Deployment enterprise skala besar.
-   Dukungan semua compositor Linux dan semua konfigurasi GPU sejak
    rilis pertama.

Fitur di luar cakupan dapat ditambahkan setelah arsitektur inti stabil.

------------------------------------------------------------------------

## 2. Sasaran Produk dan Non-Functional Requirements

Target di bawah adalah sasaran awal untuk divalidasi dengan benchmark,
bukan jaminan performa.

  -----------------------------------------------------------------------
  Area                                Sasaran awal
  ----------------------------------- -----------------------------------
  Platform                            Windows 10/11, macOS, Linux

  Resolusi                            Hingga 1080p sebagai target MVP

  Frame rate                          Target hingga 30 FPS pada perangkat
                                      dan jaringan yang memadai

  Transport                           WebRTC dengan ICE; TURN sebagai
                                      fallback

  Keamanan transport                  TLS untuk API/signaling dan
                                      mekanisme keamanan WebRTC

  Mode akses                          Attended dan unattended
                                      terotorisasi

  Observability                       Log terstruktur, metrik sesi, error
                                      dan kualitas jaringan

  Distribusi                          Installer per OS, signature, dan
                                      proses update yang diverifikasi

  Kompatibilitas                      Versi protokol dan kemampuan fitur
                                      dinegosiasikan
  -----------------------------------------------------------------------

### Prinsip produk

-   Device ID bukan rahasia dan tidak boleh dianggap sebagai bukti
    otorisasi.
-   Host memiliki kontrol akhir atas izin capture dan input.
-   Pengguna dapat melihat sesi aktif dan menghentikannya.
-   Kegagalan izin OS harus dijelaskan, bukan dilewati.
-   Media tidak dikirim melalui REST API atau database.
-   Koneksi langsung diprioritaskan; relay hanya digunakan bila
    diperlukan atau dipilih kebijakan.
-   Semua fitur berisiko tinggi harus memiliki permission terpisah.

------------------------------------------------------------------------

## 3. Arsitektur Tingkat Tinggi

Sistem dibagi menjadi dua bidang utama.

### 3.1 Control plane

Bertanggung jawab atas:

-   Login dan autentikasi.
-   Registrasi dan identitas perangkat.
-   Device discovery dan presence.
-   Permintaan, persetujuan, serta pencabutan sesi.
-   Signaling WebRTC: SDP offer/answer dan ICE candidates.
-   Kebijakan akses dan izin.
-   Audit log dan rate limiting.

Komponen yang disarankan:

-   Rust + Axum.
-   WebSocket Secure.
-   PostgreSQL.
-   Redis.
-   OIDC/OAuth atau sistem akun dengan MFA.

### 3.2 Media/data plane

Bertanggung jawab atas:

-   Screen capture.
-   Encoding dan decoding video.
-   Audio opsional.
-   Event mouse dan keyboard.
-   Clipboard.
-   File transfer pada fase berikutnya.

Komponen:

-   Remote engine Rust.
-   WebRTC PeerConnection.
-   RTP/SRTP untuk media.
-   WebRTC DataChannel untuk event kontrol dan data kecil.
-   Coturn untuk STUN/TURN.

### 3.3 Diagram logis

``` text
+--------------------------+             +--------------------------+
| Controller Desktop       |             | Host Desktop             |
| Tauri + React + Rust     |             | Tauri/Host Agent + Rust   |
|                          |             |                          |
| Remote Client Engine     |             | Permission Manager       |
| Video Renderer           |             | Screen Capture           |
| Input Event Generator    |             | Input Controller         |
+-------------+------------+             +-------------+------------+
              |                                          |
              | HTTPS/WSS: auth, signaling, policy       |
              +------------------+-----------------------+
                                 |
                    +------------v-------------+
                    | Control Plane             |
                    | Axum + WebSocket          |
                    | Auth / Device / Sessions  |
                    | PostgreSQL + Redis        |
                    +------------+-------------+
                                 |
                         SDP / ICE negotiation
                                 |
                     +-----------v-----------+
                     | WebRTC Connectivity   |
                     | ICE / STUN / TURN     |
                     +-----------+-----------+
                                 |
                    +------------+-------------+
                    |                          |
             +------v------+            +------v------+
             | Direct P2P  |            | TURN Relay  |
             | Preferred   |            | Fallback    |
             +-------------+            +-------------+
```

Control plane tidak perlu meneruskan semua frame video. Ia mengatur
identitas, otorisasi, dan negosiasi koneksi. Media berjalan melalui
WebRTC secara langsung jika kondisi jaringan memungkinkan.

------------------------------------------------------------------------

## 4. Tech Stack dan Library

Versi library harus ditetapkan saat implementasi dimulai, lalu dikunci
dengan lockfile dan diperbarui melalui proses dependency review.

  -----------------------------------------------------------------------
  Lapisan                 Pilihan                 Tanggung jawab
  ----------------------- ----------------------- -----------------------
  Desktop shell           Tauri 2                 Window, packaging,
                                                  bridge UI-native

  UI                      React + TypeScript +    UI koneksi dan remote
                          Vite                    session

  Styling                 Tailwind CSS            Komponen UI

  Runtime async           Tokio                   Task dan asynchronous
                                                  I/O

  WebRTC                  `webrtc-rs`             PeerConnection, ICE,
                                                  media, DataChannel

  Signaling               Axum + WebSocket        Signaling dan API
                                                  control plane

  Serialization           Serde + JSON            Pesan protokol awal

  Database                PostgreSQL + SQLx       Akun, perangkat, sesi,
                                                  audit

  Cache/presence          Redis                   Presence, cache, rate
                                                  limiting

  Logging                 `tracing`               Log terstruktur dan
                                                  tracing

  Metrics                 Prometheus-compatible   Kualitas sesi dan
                          metrics                 kesehatan server

  TURN                    Coturn                  STUN/TURN relay

  Secret storage          OS credential/keychain  Kredensial perangkat
                          APIs                    dan token

  CI                      GitHub Actions atau     Build, test, lint,
                          setara                  dependency audit

  Deployment              Docker untuk backend    Signaling/API dan
                                                  observability
  -----------------------------------------------------------------------

### 4.1 WebRTC

WebRTC merupakan pilihan untuk media real-time dan DataChannel. Evaluasi
`webrtc-rs` berdasarkan:

-   API yang tersedia pada versi yang dipilih.
-   Dukungan codec yang diperlukan.
-   Integrasi dengan sumber video native.
-   Kebutuhan hardware encoding/decoding.
-   Interoperabilitas lintas OS.
-   Kondisi proyek, lisensi, isu terbuka, dan jadwal rilis.

Jangan mengasumsikan dukungan hardware acceleration otomatis tersedia
hanya karena menggunakan WebRTC. Jalur frame dan encoder native mungkin
membutuhkan integrasi tambahan.

### 4.2 Signaling server

Rekomendasi awal adalah Rust + Axum agar bahasa dan tipe data konsisten.
Go juga merupakan alternatif yang baik jika tim lebih menguasainya.
Hindari menjalankan dua backend berbeda tanpa alasan operasional yang
jelas.

### 4.3 Database

Gunakan PostgreSQL untuk data persisten. Redis dapat digunakan untuk
presence, cache, rate limiting, dan state sementara, tetapi jangan
menjadikan Redis sebagai satu-satunya sumber kebenaran untuk izin atau
kepemilikan perangkat.

### 4.4 UI dan native permissions

Frontend tidak boleh mendapatkan akses native yang terlalu luas. Gunakan
capability Tauri secara minimal dan expose command yang spesifik,
tervalidasi, serta memiliki pemeriksaan otorisasi pada sisi host engine.

------------------------------------------------------------------------

## 5. Desain Engine Rust

Engine harus bebas dari ketergantungan pada React dan Tauri sejauh
memungkinkan.

### 5.1 Modul utama

-   `remote-core`: state machine sesi, domain types, lifecycle.
-   `remote-protocol`: tipe pesan dan versi protokol.
-   `remote-webrtc`: PeerConnection, ICE, media, DataChannel.
-   `remote-capture`: interface screen capture.
-   `remote-input`: interface input controller.
-   `remote-security`: identitas, kebijakan, validasi otorisasi.
-   `remote-clipboard`: clipboard pada fase berikutnya.
-   `remote-telemetry`: log, metrik, diagnostik.
-   Implementasi native khusus OS untuk capture dan input.

### 5.2 Interface konseptual

Berikut ilustrasi kontrak, bukan kode siap kompilasi:

``` rust
pub trait ScreenCapture {
    async fn start(&mut self) -> Result<(), CaptureError>;
    async fn next_frame(&mut self) -> Result<VideoFrame, CaptureError>;
    async fn stop(&mut self) -> Result<(), CaptureError>;
}

pub trait InputController {
    async fn send_mouse(
        &self,
        event: MouseEvent,
    ) -> Result<(), InputError>;

    async fn send_keyboard(
        &self,
        event: KeyboardEvent,
    ) -> Result<(), InputError>;
}
```

Implementasi produksi perlu memutuskan penggunaan trait async, object
safety, thread safety, cancellation, backpressure, dan pengelolaan
buffer.

### 5.3 State machine sesi

State yang disarankan:

``` text
Idle
  -> Requesting
  -> AwaitingApproval
  -> Negotiating
  -> Connecting
  -> Active
  -> Ending
  -> Ended

Jalur alternatif:
Requesting -> Rejected
Negotiating -> Failed
Connecting -> Failed
Active -> Revoked
Active -> Disconnected -> Reconnecting -> Active
```

Setiap transisi harus divalidasi. Misalnya, input tidak boleh aktif pada
state `Negotiating` atau `AwaitingApproval`.

------------------------------------------------------------------------

## 6. Implementasi Native per Sistem Operasi

Dukungan lintas platform sejak awal berarti interface dan build pipeline
tersedia untuk ketiga OS, tetapi kemampuan aktual tetap perlu diuji per
OS.

### 6.1 Windows

Pertimbangkan:

-   Windows Graphics Capture untuk screen capture.
-   Windows APIs yang sesuai untuk input mouse dan keyboard.
-   Media Foundation atau API media Windows untuk encoding/decoding.
-   MSI atau installer bertanda tangan.

Kasus khusus yang perlu diuji:

-   Permission dan sesi pengguna.
-   UAC dan Secure Desktop.
-   Lock/unlock dan pergantian pengguna.
-   Multi-monitor dan perubahan resolusi.
-   GPU berbeda dan driver.
-   High DPI scaling.

Jangan mencoba melewati Secure Desktop atau batas keamanan sistem.
Dokumentasikan batasan remote control untuk layar UAC atau login.

### 6.2 macOS

Pertimbangkan:

-   ScreenCaptureKit untuk capture.
-   Core Graphics dan API Accessibility yang sesuai untuk input.
-   VideoToolbox untuk hardware encoding/decoding.
-   Signed application dan notarization untuk distribusi.

Kasus khusus:

-   Screen Recording permission.
-   Accessibility permission.
-   Perubahan izin ketika aplikasi berjalan.
-   Pergantian display dan resolusi.
-   Lock screen dan pergantian user.
-   Notarization serta update aplikasi.

Aplikasi harus meminta pengguna memberikan izin melalui alur OS yang
sah; jangan mengklaim atau mengupayakan aktivasi izin privasi secara
diam-diam.

### 6.3 Linux

Mulai dengan matriks yang dinyatakan secara eksplisit, misalnya:

-   GNOME/Wayland.
-   KDE Plasma/Wayland.
-   X11 pada distro yang dipilih.

Pertimbangkan:

-   XDG Desktop Portal dan PipeWire untuk capture Wayland.
-   Portal RemoteDesktop jika compositor mendukung input control.
-   XTest untuk jalur X11 bila sesuai.
-   VA-API atau GStreamer untuk jalur media yang dibutuhkan.

Kasus khusus:

-   Perbedaan compositor.
-   Izin portal.
-   Flatpak/sandbox jika didistribusikan dengan format tersebut.
-   Multi-monitor.
-   Sesi terkunci.
-   Driver GPU dan codec.

Wayland memiliki pembatasan keamanan yang disengaja. Jika compositor
tidak mendukung kemampuan yang diperlukan, aplikasi harus menjelaskan
keterbatasannya alih-alih mencoba melewati pembatasan tersebut.

------------------------------------------------------------------------

## 7. Protokol Komunikasi

### 7.1 Signaling transport

Gunakan WebSocket Secure (`wss://`) di atas TLS untuk signaling. API
biasa dapat menggunakan HTTPS.

Format awal dapat menggunakan JSON karena mudah didiagnosis.
Pertimbangkan CBOR atau format biner lain setelah kebutuhan performa dan
kompatibilitas terukur.

Setiap pesan sebaiknya memiliki:

-   `version`: versi protokol.
-   `type`: jenis pesan.
-   `request_id`: identifier untuk korelasi dan idempotensi.
-   `session_id`: jika terkait sesi.
-   `payload`: data sesuai schema.
-   Timestamp atau expiry jika relevan, dengan validasi server.

Contoh:

``` json
{
  "version": 1,
  "type": "session.request",
  "request_id": "req_example_001",
  "payload": {
    "target_device_id": "device_example",
    "mode": "attended",
    "permissions": [
      "view_screen",
      "control_input"
    ]
  }
}
```

Contoh ini tidak berisi token dan bukan bukti otorisasi. Server harus
memperoleh identitas peminta dari sesi autentikasi yang tervalidasi,
bukan mempercayai field identitas yang dikirim client.

### 7.2 Jenis pesan minimum

  Pesan                 Fungsi
  --------------------- -------------------------------
  `device.register`     Registrasi perangkat
  `device.heartbeat`    Presence perangkat
  `session.request`     Permintaan sesi
  `session.approve`     Respons persetujuan host
  `session.reject`      Penolakan
  `session.revoke`      Pencabutan sesi
  `rtc.offer`           SDP offer
  `rtc.answer`          SDP answer
  `rtc.ice_candidate`   ICE candidate
  `session.state`       Perubahan status
  `session.end`         Mengakhiri sesi
  `device.remove`       Menghapus perangkat tepercaya

Server perlu memvalidasi schema, ukuran pesan, identitas pengirim,
otorisasi, expiry, dan state transition. Pesan duplikat harus ditangani
secara idempotent jika sesuai.

### 7.3 WebRTC media dan DataChannel

  -----------------------------------------------------------------------
  Kanal                   Penggunaan              Kebijakan awal
  ----------------------- ----------------------- -----------------------
  Video track             Layar host              Prioritas real-time

  Audio track             Audio sistem jika       Opsional dan perlu izin
                          didukung                

  `control`               Mouse dan keyboard      Ordered, reliable

  `clipboard`             Teks clipboard          Izin terpisah

  `file-transfer`         Transfer file pada fase Chunking, checksum,
                          berikutnya              backpressure
  -----------------------------------------------------------------------

Untuk input, mulai dengan ordered dan reliable DataChannel. Optimasi
reliabilitas hanya boleh dilakukan setelah protokol event dan dampaknya
diuji. Event keyboard harus mempertahankan urutan dan memiliki
penanganan untuk tombol yang masih dianggap ditekan ketika koneksi
terputus.

Transfer file besar perlu protokol tersendiri dengan batas ukuran,
chunking, checksum, pembatalan, dan backpressure.

### 7.4 Video pipeline

``` text
Native screen capture
        |
        v
Frame normalization / damage detection
        |
        v
Hardware or software encoder
        |
        v
WebRTC media transport
        |
        v
Decoder on controller
        |
        v
Renderer
```

Rencana optimasi:

-   Kurangi salinan buffer CPU/GPU.
-   Adaptasi bitrate terhadap bandwidth.
-   Adaptasi frame rate dan resolusi.
-   Ukur dropped frames dan waktu encoding.
-   Gunakan hardware encoder jika tersedia.
-   Sediakan software fallback.
-   Uji teks kecil, scrolling, video, dan gerakan cepat.

------------------------------------------------------------------------

## 8. Alur Koneksi dan Model Akses

### 8.1 Attended access

1.  Controller login dan memilih perangkat tujuan.
2.  Backend memverifikasi akun serta hak akses.
3.  Host menerima permintaan dengan identitas peminta dan izin yang
    diminta.
4.  Pengguna host menerima atau menolak.
5.  Signaling menukar SDP dan ICE candidates.
6.  Kedua endpoint membangun WebRTC.
7.  Host mengaktifkan hanya izin yang disetujui.
8.  Host dan controller dapat mengakhiri sesi; host selalu dapat memutus
    koneksi.

### 8.2 Unattended access

1.  Pemilik mengaktifkan fitur secara eksplisit pada perangkat.
2.  Pemilik memilih controller/perangkat tepercaya atau kebijakan akses
    yang sesuai.
3.  Controller mengautentikasi diri dan meminta sesi.
4.  Host memvalidasi identitas controller, status perangkat, policy
    lokal, dan expiry.
5.  Host menampilkan indikator sesi aktif.
6.  Pemilik dapat mencabut akses atau menonaktifkan fitur kapan saja.
7.  Semua peristiwa penting masuk ke audit log.

Unattended access bukan akses anonim atau tersembunyi. Gunakan
kredensial yang dapat dicabut, masa berlaku yang sesuai, MFA untuk
tindakan sensitif, dan indikator aktivitas yang jelas.

### 8.3 Permission model

  -----------------------------------------------------------------------
  Izin                                Default
  ----------------------------------- -----------------------------------
  View screen                         Harus disetujui untuk attended;
                                      mengikuti policy untuk unattended

  Mouse/keyboard                      Izin terpisah

  Clipboard                           Nonaktif sampai diizinkan

  File transfer                       Nonaktif pada MVP

  Audio                               Nonaktif sampai diizinkan

  Administrative command              Tidak tersedia pada MVP
  -----------------------------------------------------------------------

UI tidak menjadi sumber otorisasi final. Host engine dan server harus
memvalidasi policy pada batas kepercayaan yang tepat.

------------------------------------------------------------------------

## 9. Struktur Repository

Contoh monorepo:

``` text
remote-desktop/
├── apps/
│   └── desktop/
│       ├── src/
│       ├── src-tauri/
│       │   ├── src/
│       │   ├── capabilities/
│       │   └── tauri.conf.json
│       └── package.json
├── crates/
│   ├── remote-core/
│   ├── remote-protocol/
│   ├── remote-webrtc/
│   ├── remote-capture/
│   ├── capture-windows/
│   ├── capture-macos/
│   ├── capture-linux/
│   ├── remote-input/
│   ├── input-windows/
│   ├── input-macos/
│   ├── input-linux/
│   ├── remote-security/
│   ├── remote-clipboard/
│   └── remote-telemetry/
├── services/
│   ├── signaling/
│   └── api/
├── infra/
│   ├── docker/
│   ├── compose/
│   ├── coturn/
│   ├── monitoring/
│   └── deployment/
├── tests/
│   ├── integration/
│   ├── network/
│   ├── security/
│   └── performance/
├── docs/
│   ├── architecture.md
│   ├── protocol.md
│   ├── threat-model.md
│   ├── operations.md
│   └── adr/
├── Cargo.toml
├── Cargo.lock
└── README.md
```

Struktur ini adalah target akhir yang dapat tumbuh bertahap. Jangan
membuat seluruh crate kosong di awal; mulai dari modul inti dan pecah
saat batas tanggung jawab sudah jelas.

### Aturan dependensi

-   `remote-core` tidak bergantung pada Tauri.
-   `remote-protocol` tidak bergantung pada OS.
-   `remote-capture` mendefinisikan interface; crate platform
    menyediakan implementasi.
-   `remote-security` mengatur identitas dan policy.
-   `remote-webrtc` menangani transport, bukan keputusan izin.
-   `apps/desktop` mengorkestrasi komponen melalui API yang tervalidasi.
-   Jangan membiarkan UI memanggil fungsi input native tanpa pemeriksaan
    policy sesi.

------------------------------------------------------------------------

## 10. Desain Data Awal

Skema berikut bersifat konseptual dan perlu disesuaikan dengan kebutuhan
autentikasi dan retensi.

### 10.1 `users`

-   `id`
-   `email`
-   `display_name`
-   `identity_provider`
-   `created_at`
-   `updated_at`
-   `status`

Jangan menyimpan password plaintext. Jika autentikasi didelegasikan ke
OIDC provider, simpan identifier provider yang diperlukan.

### 10.2 `devices`

-   `id`
-   `owner_user_id`
-   `display_name`
-   `platform`
-   `public_key`
-   `last_seen_at`
-   `created_at`
-   `revoked_at`
-   `status`

Jangan menyimpan private key perangkat dalam tabel ini.

### 10.3 `trusted_controllers`

-   `id`
-   `device_id`
-   `controller_identity`
-   `granted_by`
-   `permissions`
-   `created_at`
-   `expires_at`
-   `revoked_at`

### 10.4 `sessions`

-   `id`
-   `controller_device_id`
-   `host_device_id`
-   `mode`
-   `status`
-   `requested_at`
-   `approved_at`
-   `started_at`
-   `ended_at`
-   `end_reason`

### 10.5 `session_permissions`

-   `session_id`
-   `permission`
-   `granted`
-   `granted_at`
-   `revoked_at`

### 10.6 `audit_events`

-   `id`
-   `actor_id`
-   `device_id`
-   `session_id`
-   `event_type`
-   `result`
-   `created_at`
-   `metadata_redacted`

Audit log tidak boleh menyimpan password, token sesi, private key, atau
payload clipboard/file. Tetapkan kebijakan retensi dan akses audit yang
terpisah.

------------------------------------------------------------------------

## 11. Keamanan dan Threat Model

### 11.1 Identitas dan autentikasi

Gunakan lapisan identitas berikut:

1.  Identitas akun.
2.  Identitas perangkat berbasis key pair.
3.  Identitas sesi yang memiliki expiry.
4.  Identitas controller dan izin yang berlaku.

Gunakan MFA untuk akun dan tindakan sensitif. Simpan secret di OS
credential vault bila memungkinkan. Jangan menaruh token jangka panjang
dalam `localStorage`, konfigurasi plaintext, atau log.

### 11.2 Proteksi transport

-   TLS untuk API dan signaling.
-   WebRTC menggunakan mekanisme keamanan standar seperti DTLS-SRTP.
-   Validasi fingerprint dan identitas peer sesuai desain.
-   Jangan menonaktifkan pemeriksaan sertifikat.
-   Jangan mempercayai Device ID sebagai autentikasi.
-   Gunakan expiry, nonce/request ID, dan validasi state untuk mencegah
    replay.
-   Lindungi TURN credentials dengan masa berlaku dan scope terbatas.

### 11.3 Ancaman dan mitigasi

  -----------------------------------------------------------------------
  Ancaman                             Mitigasi
  ----------------------------------- -----------------------------------
  Penyerang menebak Device ID         Device ID bukan bukti otorisasi;
                                      wajib autentikasi

  Session hijacking                   Token terbatas, identitas peer,
                                      expiry, validasi sesi

  Replay pesan                        Nonce, request ID, expiry, state
                                      machine

  Penyalahgunaan unattended access    Opt-in, MFA, trusted controller,
                                      revocation

  Input tanpa izin                    Validasi policy pada host sebelum
                                      event diterapkan

  Kebocoran secret                    OS vault, rotasi, redaksi log

  Serangan terhadap signaling         Rate limit, schema validation,
                                      batas ukuran pesan

  Penyalahgunaan TURN                 Kredensial sementara, kuota,
                                      monitoring, firewall

  Update berbahaya                    Signed binaries, signature
                                      verification, secure update

  Dependency rentan                   Audit dependency, lockfile, patch
                                      policy
  -----------------------------------------------------------------------

### 11.4 Respons insiden

Sediakan mekanisme untuk:

-   Mencabut perangkat.
-   Mencabut semua sesi aktif milik perangkat.
-   Menonaktifkan unattended access.
-   Merotasi kredensial.
-   Menandai versi client rentan.
-   Menghentikan distribusi update bermasalah.
-   Meninjau audit log tanpa membuka konten sensitif.

------------------------------------------------------------------------

## 12. Roadmap Pengembangan

Estimasi berikut untuk perencanaan tim kecil yang sudah memiliki
pengalaman Rust dan desktop development. Integrasi native lintas
platform dapat memperpanjang jadwal.

  --------------------------------------------------------------------------
  Fase                                        Estimasi Hasil
  ----------------------- ---------------------------- ---------------------
  0\. Technical spike                      1--2 minggu Bukti capture, input,
                                                       WebRTC, TURN pada OS
                                                       target

  1\. Engine dasar                         2--3 minggu Video pipeline,
                                                       renderer, input,
                                                       lifecycle

  2\. Control plane                        2--3 minggu Signaling, device
                                                       registry, login,
                                                       attended access

  3\. Security/unattended                  2--3 minggu Trusted device,
                                                       policy,
                                                       MFA/revocation, audit

  4\.                                      2--4 minggu Installer, signing,
  Stabilitas/distribusi                                test jaringan dan
                                                       benchmark
  --------------------------------------------------------------------------

Total perencanaan awal: sekitar 9--15 minggu, dengan buffer untuk risiko
native integration. Jangan menjanjikan dukungan penuh semua fitur OS
berdasarkan jadwal ini tanpa hasil technical spike.

### Fase 0 --- Technical spike

Buktikan hal berikut sebelum mengembangkan dashboard besar:

-   Capture layar pada Windows, macOS, dan Linux.
-   Input control yang sah pada masing-masing OS.
-   Satu jalur WebRTC video end-to-end.
-   P2P dan TURN relay.
-   Pengukuran latensi dan bandwidth.
-   Penanganan izin yang ditolak.

### Fase 1 --- Engine

-   Implementasi interface capture.
-   Frame pipeline dan renderer.
-   WebRTC PeerConnection.
-   DataChannel untuk input.
-   Lifecycle dan reconnect dasar.
-   Telemetry performa.

### Fase 2 --- Control plane

-   Registrasi perangkat.
-   Autentikasi.
-   Signaling.
-   Attended session.
-   Session expiry dan audit event.
-   Device presence.

### Fase 3 --- Unattended access

-   Aktivasi eksplisit.
-   Trusted controller.
-   Kebijakan izin per fitur.
-   MFA untuk tindakan sensitif.
-   Revocation dan session termination.
-   Security review.

### Fase 4 --- Stabilisasi

-   Uji NAT dan TURN.
-   Installer serta signed update.
-   Uji lintas OS dan GPU.
-   Benchmark LAN/WAN.
-   Dokumentasi deployment dan incident response.

------------------------------------------------------------------------

## 13. Strategi Testing

### 13.1 Unit test

-   Schema dan parsing protokol.
-   State machine sesi.
-   Permission policy.
-   Expiry dan revocation.
-   Normalisasi event mouse/keyboard.
-   Penanganan reconnect.
-   Pengelolaan error capture.

### 13.2 Integration test

-   Dua endpoint melalui signaling.
-   P2P berhasil.
-   TURN fallback berhasil.
-   Host menolak permintaan.
-   Controller tak berizin ditolak.
-   Sesi dicabut saat aktif.
-   Koneksi terputus dan pulih.
-   Host menghentikan input saat sesi berakhir.

### 13.3 Performance test

Ukur:

-   Waktu dari permintaan hingga sesi siap.
-   Latensi input-to-visible-change.
-   FPS aktual.
-   Dropped frames.
-   Bandwidth rata-rata dan puncak.
-   CPU, GPU, RAM, dan baterai.
-   Waktu pemulihan setelah packet loss.
-   Performa pada resolusi dan refresh rate berbeda.

Jangan hanya menggunakan ping sebagai ukuran latensi remote desktop.
Ukur alur dari input controller sampai perubahan tampilan terlihat
kembali.

### 13.4 Network test

Uji:

-   LAN dengan latensi rendah.
-   WAN dengan RTT yang berbeda.
-   Packet loss dan jitter.
-   Bandwidth terbatas.
-   NAT simetris atau jaringan yang memerlukan TURN.
-   Pergantian Wi-Fi ke Ethernet.
-   Putus-sambung jaringan.
-   TURN server tidak tersedia.

### 13.5 Security test

-   Fuzzing parser pesan.
-   Manipulasi dan replay pesan.
-   Akses lintas akun/perangkat.
-   Brute-force resistance dan rate limiting.
-   Pemeriksaan token di log.
-   Uji pencabutan perangkat dan sesi.
-   Dependency audit dan pemeriksaan signature update.
-   Verifikasi bahwa penolakan izin OS tidak dilewati.

### 13.6 Matriks OS

  -------------------------------------------------------------------------------
  Pengujian          Windows           macOS                  Linux
  ------------------ ----------------- ---------------------- -------------------
  Screen capture     Wajib             Wajib                  X11 dan Wayland

  Mouse/keyboard     Wajib             Accessibility          Portal/compositor
                                       permission             dan X11

  Multi-monitor      Wajib             Wajib                  Bergantung
                                                              compositor

  Lock/unlock        Wajib             Wajib                  Per desktop
                                                              environment

  Reconnect          Wajib             Wajib                  Wajib

  Hardware encoding  GPU yang dipilih  VideoToolbox           VA-API sesuai
                                                              perangkat

  Installer/update   Signed installer  Signing/notarization   Paket yang
                                                              ditargetkan
  -------------------------------------------------------------------------------

CI dapat menjalankan linting, unit test, audit dependency, dan build.
Pengujian native harus melibatkan mesin atau runner yang memiliki
display session dan izin yang benar. Linux Wayland tidak cukup diuji
hanya dalam CI headless.

------------------------------------------------------------------------

## 14. Kriteria Penerimaan MVP

MVP dapat dinyatakan memenuhi target awal bila:

-   [ ] Ketiga OS dapat melakukan screen capture sesuai matriks
    dukungan.
-   [ ] Attended access memerlukan persetujuan host.
-   [ ] Unattended access hanya berjalan setelah diaktifkan pemilik.
-   [ ] Input mouse/keyboard hanya aktif ketika diotorisasi.
-   [ ] P2P dan TURN fallback telah diuji.
-   [ ] Permintaan tidak sah ditolak.
-   [ ] Host dapat menghentikan sesi secara lokal.
-   [ ] Revocation menghentikan akses yang sudah berjalan.
-   [ ] Audit log merekam kejadian penting tanpa menyimpan secret.
-   [ ] Installer/update diverifikasi.
-   [ ] Benchmark LAN dan WAN tersedia.
-   [ ] Keterbatasan fitur per OS didokumentasikan.

------------------------------------------------------------------------

## 15. Deployment dan Operasional

### Komponen server

-   API/signaling service.
-   PostgreSQL.
-   Redis.
-   Coturn.
-   Monitoring dan log collection.
-   Reverse proxy untuk HTTPS/WSS.

### Lingkungan

1.  **Development:** seluruh layanan dapat dijalankan lokal menggunakan
    Docker Compose.
2.  **Staging:** konfigurasi mirip produksi dengan TURN publik dan
    sertifikat TLS.
3.  **Production:** secret manager, backup database, monitoring, alert,
    pembatasan firewall, serta proses rotasi kredensial.

### Operasional

-   Pantau koneksi berhasil/gagal, penggunaan TURN, durasi sesi,
    latency, packet loss, dan error rate.
-   Jangan log isi layar, clipboard, file, token, atau private key.
-   Siapkan backup dan prosedur pemulihan PostgreSQL.
-   Terapkan pembaruan keamanan dependency.
-   Pisahkan kredensial staging dan production.
-   Sediakan kill switch untuk mencabut sesi atau menonaktifkan
    unattended access saat insiden.

------------------------------------------------------------------------

## 16. Risiko Teknis Utama

  -----------------------------------------------------------------------
  Risiko                  Dampak                  Mitigasi
  ----------------------- ----------------------- -----------------------
  Screen capture lintas   Tinggi                  Technical spike dan
  OS                                              interface native

  Input control           Tinggi                  Uji
  Wayland/macOS                                   permission/compositor
                                                  sejak awal

  Interoperabilitas codec Tinggi                  Mulai dengan subset
                                                  codec yang teruji

  Latensi video           Tinggi                  Benchmark end-to-end,
                                                  buffer minim, adaptive
                                                  bitrate

  P2P gagal di NAT        Sedang--tinggi          TURN dan pengukuran
                                                  rasio relay

  Secret unattended bocor Tinggi                  OS vault, revocation,
                                                  MFA, audit

  Update berbahaya        Tinggi                  Signature verification
                                                  dan release pipeline

  Scope membesar          Tinggi                  MVP ketat, fitur
                                                  lanjutan setelah stabil
  -----------------------------------------------------------------------

------------------------------------------------------------------------

## 17. Keputusan Arsitektur yang Disarankan

1.  Gunakan Rust untuk engine dan signaling awal.
2.  Gunakan Tauri 2 + React + TypeScript untuk UI.
3.  Gunakan WebRTC untuk media real-time, bukan membangun protokol media
    sendiri.
4.  Pisahkan control plane dan media plane.
5.  Prioritaskan satu jalur video end-to-end pada fase technical spike,
    tetapi buat interface lintas OS sejak awal.
6.  Mulai dengan attended access; unattended access baru diaktifkan
    setelah identitas, policy, dan revocation siap.
7.  Gunakan PostgreSQL untuk data persisten dan Redis hanya untuk data
    sementara/cache.
8.  Gunakan Coturn untuk fallback jaringan.
9.  Jadikan security review, permission handling, signed update, dan
    audit log bagian dari definisi selesai MVP.
10. Tunda file transfer, remote shell, dan fitur enterprise hingga
    koneksi dasar teruji.

------------------------------------------------------------------------

## 18. Referensi Teknis

-   WebRTC: <https://webrtc.org/>
-   WebRTC.rs: <https://github.com/webrtc-rs/webrtc>
-   Tauri 2: <https://v2.tauri.app/>
-   Tauri security capabilities:
    <https://v2.tauri.app/security/capabilities/>
-   Axum: <https://github.com/tokio-rs/axum>
-   Tokio: <https://tokio.rs/>
-   Coturn: <https://github.com/coturn/coturn>
-   RustDesk repository (referensi pembanding):
    <https://github.com/rustdesk/rustdesk>
-   RustDesk Server: <https://github.com/rustdesk/rustdesk-server>
-   RustDesk self-host documentation:
    <https://rustdesk.com/docs/en/self-host/>

Periksa versi, lisensi, status pemeliharaan, dan kompatibilitas
dependency sebelum mengunci pilihan untuk rilis produksi.

------------------------------------------------------------------------

## 19. Langkah Pertama yang Paling Penting

Mulailah dari proof of concept kecil:

1.  Capture layar pada satu OS.
2.  Kirim video melalui WebRTC ke endpoint kedua.
3.  Tambahkan input mouse/keyboard dengan izin yang sah.
4.  Uji P2P dan TURN.
5.  Catat latency, FPS, bandwidth, serta penggunaan CPU/GPU.
6.  Replikasi pendekatan melalui interface yang sama di macOS dan Linux.
7.  Setelah kemampuan native terbukti, lanjutkan ke signaling, akun,
    attended access, lalu unattended access.

Keberhasilan aplikasi ini paling ditentukan oleh kualitas video
pipeline, input control native, dan penanganan keamanan sesi. UI dan
dashboard sebaiknya dikembangkan setelah tiga fondasi tersebut terbukti.
