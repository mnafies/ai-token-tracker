# AI Token Tracker

Aplikasi macOS (Tauri v2: Rust + React/TypeScript) yang melacak pemakaian token dan estimasi biaya dari agen AI di komputer Anda. Semua data dibaca dari file lokal agen secara read-only; tidak ada isi percakapan yang dikirim ke mana pun.

## Fitur

- Ikon menu bar dengan biaya hari ini, plus jendela dashboard (ringkasan per periode, per agen, per model, per proyek, detail sesi, ekspor).
- Menghitung token input, output, cache read, dan cache write per sesi, lalu mengestimasi biaya dari harga OpenRouter.
- Pemindaian awal otomatis dan pemantauan file secara berkelanjutan.
- Deteksi proses agen yang sedang berjalan.
- Harga model bisa dikoreksi manual (map, abaikan, atau harga manual).

## Agen yang didukung

Dilacak penuh (token dan biaya):

- Claude Code (`~/.claude`)
- Codex (`~/.codex`)
- opencode (`~/.local/share/opencode`)
- Gemini CLI (`~/.gemini`)
- Copilot CLI (`~/.copilot`)

Hanya terdeteksi keberadaannya (tanpa data token):

- Claude Desktop (`~/Library/Application Support/Claude`)
- VS Code (`~/Library/Application Support/Code`)

## Instalasi

1. Buka file `AI Token Tracker_0.1.0_aarch64.dmg` (Apple Silicon, macOS 13 atau lebih baru).
2. Seret `AI Token Tracker` ke folder `Applications`.
3. Aplikasi belum ditandatangani (unsigned), jadi Gatekeeper akan menolak pembukaan biasa. Buka pertama kali dengan klik kanan pada aplikasi, pilih **Open**, lalu konfirmasi **Open**.

## Saat pertama kali dibuka

- Opsi "Jalankan saat login" aktif secara default (autostart didaftarkan).
- Aplikasi memindai folder di rumah Anda untuk menemukan data agen. macOS akan menampilkan permintaan izin akses untuk Desktop, Documents, dan Downloads; izin ini hanya dipakai untuk pemindaian tersebut.
- Harga model diambil dari OpenRouter (butuh koneksi internet). Tanpa itu, biaya model ditampilkan sebagai tidak diketahui.

## Lokasi database

`~/Library/Application Support/AI Token Tracker/tracker.db`

## Mematikan "jalankan saat login"

Buka halaman **Pengaturan** di aplikasi dan hilangkan centang **Jalankan saat login**, lalu simpan.

## Build dari source

Prasyarat: Node.js, Rust (target `aarch64-apple-darwin`), dan Xcode Command Line Tools.

```sh
npm install
npm run tauri build -- --bundles dmg
```

Dari sesi headless/SSH, awali perintah dengan `CI=true` (melewati penataan jendela dmg via Finder AppleScript yang gagal tanpa GUI).

Hasil: `src-tauri/target/release/bundle/dmg/` (atau `src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/` bila memakai `--target aarch64-apple-darwin`).

Tes:

```sh
cd src-tauri && cargo test
npm test
```

Pemeriksaan data asli tanpa GUI (hanya mencetak agregat, tanpa isi percakapan):

```sh
cd src-tauri && cargo run --release --example real_data_check
```
