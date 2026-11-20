# AI Token Tracker — Desain

Tanggal: 2026-11-21
Status: disetujui (2026-11-21)

## 1. Tujuan dan alasan

Aplikasi macOS (`.dmg`) yang otomatis:

1. **Mendeteksi agent AI** di laptop ini — yang sedang berjalan (proses aktif) dan yang pernah berjalan (ada riwayat sesi di disk).
2. **Melacak pemakaian token sedetail mungkin** — per agent, per folder project, per sesi, per model, sampai per respons.
3. **Menghitung biaya** dari token × harga per token, dengan harga diambil dari OpenRouter.
4. **Analisis mingguan dan bulanan** — total, tren, dan perbandingan dengan periode sebelumnya.

Alasan: pemilik ingin melihat biaya pemakaian agent AI dan menganalisis polanya per minggu/bulan. Patokan biaya adalah harga per token (bukan harga langganan).

## 2. Goals, non-goals, anti-goals

**Goals**
- Deteksi otomatis tanpa konfigurasi: buka aplikasi → langsung muncul agent, project, sesi, dan biaya dari seluruh riwayat yang ada di disk.
- Update otomatis saat agent dipakai (tanpa perlu refresh manual).
- Rincian: agent → folder project → sesi → respons; juga potongan per model.
- Tampilan mingguan & bulanan, dengan perbandingan ke periode sebelumnya.
- Harga dari OpenRouter, ter-cache lokal, bisa di-override manual.

**Non-goals (versi pertama)**
- Sinkronisasi cloud, multi-perangkat, atau tampilan tim.
- Mengontrol/menghentikan agent.
- Mencocokkan dengan tagihan resmi provider (invoice).
- Melacak chat di web (claude.ai, chatgpt.com) — datanya tidak ada di laptop.

**Anti-goals (gagal walau "jalan")**
- Aplikasi membuat laptop lambat atau boros baterai.
- Isi percakapan dikirim keluar laptop.
- Aplikasi mengubah/menghapus file milik agent.
- Biaya salah secara diam-diam — model tanpa harga harus terlihat jelas sebagai "harga tidak diketahui", bukan dihitung $0.

## 3. Constraints

- Platform: macOS, Apple Silicon (arm64). Laptop ini tidak punya Xcode penuh; toolchain tersedia: Rust (cargo 1.100), Node 22, Swift CLI.
- `.dmg` tidak ditandatangani (tanpa akun Apple Developer) → pengguna klik kanan → Open saat pertama kali.
- Hanya-baca terhadap data agent.
- Satu-satunya koneksi keluar: mengambil daftar harga dari `https://openrouter.ai/api/v1/models` (publik, tanpa API key, tanpa mengirim data pemakaian).

## 4. Pendekatan yang dipertimbangkan

| Pendekatan | Kelebihan | Kekurangan |
|---|---|---|
| **Tauri (Rust + UI web)** — **dipilih** | `.dmg` ~10–15 MB, ringan di RAM, Rust cepat parsing JSONL/SQLite besar, `.dmg` bisa dibuat dengan toolchain yang ada | Dua bahasa (Rust + TypeScript) |
| Electron (semua JS) | Paling cepat dibuat | `.dmg` 150 MB+, boros RAM untuk aplikasi yang jalan terus |
| SwiftUI native | Paling "Mac", menu bar paling rapi | Butuh install Xcode (belasan GB); tanpa itu bundling & `.dmg` rawan masalah |

Pemilik memilih **Tauri** karena ringan, cepat untuk riwayat besar, dan bisa dibangun di laptop ini.

## 5. Sumber data per agent (hasil pengecekan laptop ini)

| Agent | Lokasi | Data token | Folder project | Tingkat detail |
|---|---|---|---|---|
| Claude Code (CLI, VS Code, desktop) | `~/.claude/projects/<folder-ter-encode>/<session>.jsonl` (±53 folder, ±79 sesi) | Ya: per pesan assistant, `message.usage` (input, output, cache_creation, cache_read) + `message.model` | Ya: field `cwd` per baris | **Penuh** |
| Codex (CLI/Desktop/VS Code) | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` (+ `archived_sessions`) | Ya: event `token_count` (input, cached_input, output, reasoning_output, total) + model di `turn_context` | Ya: `cwd` di `session_meta` | **Penuh** |
| opencode | `~/.local/share/opencode/opencode.db` (SQLite; tabel `session_v2`, `session_message`, `project`, `project_directory`) | Ya: di JSON `data` (`tokens.input/output/reasoning/cache.read/write`, `modelID`, `providerID`; field `cost` = 0 sehingga dihitung ulang) | Ya: via `project`/`project_directory` | **Penuh** |
| Gemini CLI | `~/.gemini/tmp/<hash>/chats/session-*.jsonl`, `.project_root` | **Tidak tercatat** di file lokal yang ada | Ya: `.project_root` | Sesi & folder saja |
| Copilot CLI | `~/.copilot/session-state/<uuid>/`, `~/.copilot/logs/` | **Tidak tercatat** | Belum pasti | Deteksi saja |
| Claude desktop, VS Code | `~/Library/Application Support/Claude`, `.../Code/User/globalStorage` | Mode Claude Code-nya sudah tercatat di `~/.claude`; chat desktop biasa tidak tersimpan lokal | — | Deteksi saja |

Agent tanpa data token ditampilkan dengan label jelas "token tidak tercatat lokal" — tetap muncul di daftar agent dan sesi, tapi tidak dihitung biayanya.

## 6. Desain

### 6.1 Arsitektur

```
┌─────────────── Tauri app ───────────────┐
│  UI (React + TypeScript)                │
│   Dashboard · Project · Sesi · Model ·  │
│   Agent · Harga · Pengaturan            │
│            ▲ Tauri commands / events     │
│  Core (Rust)                            │
│   ├ Adapter per agent (parser)          │
│   ├ Watcher file (FSEvents)             │
│   ├ Pendeteksi proses (tiap 5 dtk)      │
│   ├ Pricing (OpenRouter + override)     │
│   └ Database lokal (SQLite)             │
└─────────────────────────────────────────┘
  Baca-saja: ~/.claude, ~/.codex, ~/.gemini, ~/.copilot, opencode.db
  Keluar:    openrouter.ai/api/v1/models (harga saja)
```

### 6.2 Adapter agent

Setiap agent punya adapter dengan antarmuka yang sama:
- `detect()` → apakah agent terpasang/pernah dipakai (cek direktori data).
- `scan(since)` → baca sesi baru/berubah, keluarkan *usage event* yang dinormalisasi.
- `process_signatures()` → pola nama proses untuk deteksi "sedang berjalan".

Usage event yang dinormalisasi:
`agent, session_id, project_path, timestamp, model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens, source_ref (file+offset/row id)`

Hal yang wajib ditangani builder:
- **Claude Code:** satu respons bisa muncul di beberapa baris JSONL (streaming/potongan konten) dengan `message.id` yang sama → dedupe berdasarkan `message.id` (+ `requestId`). Baris sub-agent (`isSidechain`) tetap dihitung dan masuk ke sesi induknya.
- **Codex:** event `token_count` membawa total kumulatif dan pemakaian giliran terakhir → pakai yang per-giliran (atau selisih kumulatif), jangan dijumlah kumulatifnya. Model diambil dari `turn_context` terdekat.
- **opencode:** buka database dalam mode read-only (WAL aktif), jangan pernah menulis.
- Parsing inkremental: simpan offset byte per file JSONL dan `time_updated` terakhir per database, supaya pemindaian ulang hanya membaca yang baru.

Adapter baru (agent lain di masa depan) bisa ditambahkan tanpa mengubah bagian lain.

### 6.2a Definisi "project"

- **Kunci project = root repo git** dari folder kerja sesi (cari `.git` ke atas). Subfolder dan git worktree (termasuk `.claude/worktrees/...` dan worktree di luar repo, dikenali lewat file `.git` yang menunjuk ke `gitdir`) digabung ke repo utamanya.
- Folder yang bukan repo git → foldernya sendiri menjadi project.
- Folder kerja persis (`cwd`) tetap disimpan per event dan ditampilkan di detail sesi.
- Project ditentukan **per event**, jadi jika `cwd` berubah di tengah sesi lalu pindah repo, event-nya masuk ke project masing-masing. Sesi dicantumkan di project tempat mayoritas tokennya berada.
- Jika folder sudah tidak ada di disk (repo dihapus/dipindah), pakai hasil resolusi terakhir yang tersimpan; jika belum pernah diresolusi, pakai `cwd` apa adanya.

### 6.3 Deteksi agent

- **Pernah berjalan:** ada direktori data + minimal satu sesi. Ditampilkan dengan tanggal sesi pertama & terakhir.
- **Sedang berjalan:** daftar proses dicocokkan dengan tanda tangan proses tiap agent (misal `claude`, `codex`, `gemini`, `copilot`, `opencode`). Untuk tiap proses: PID, folder kerja (cwd), waktu mulai. Jika bisa, dihubungkan ke sesi aktif berdasarkan cwd + file sesi yang paling baru diubah.

### 6.3a Pemindaian direktori (discovery)

Dua jenis pemindaian, keduanya berjalan sekali saat pertama dibuka (di background, dengan progress) dan bisa diulang lewat tombol **"Scan ulang"** di halaman Agent. Lokasi yang disisir default: home folder; bisa ditambah/dikurangi di Pengaturan.

**A. Mencari data agent yang belum dikenal**
- Cari file `*.jsonl`, `*.json`, `*.db`/`*.sqlite` yang "berbentuk" data agent: berisi kunci seperti `input_tokens`, `output_tokens`, `prompt_tokens`, `completion_tokens`, `usage`, `model`, `cache_read`.
- Cocokkan juga pola folder agent yang dikenal di lokasi tidak standar (misal salinan kedua `~/.claude-*`, folder dari `CLAUDE_CONFIG_DIR`, `CODEX_HOME`).
- Hanya membaca nama file dan potongan awal isi (maks. beberapa KB per file) untuk deteksi; isi percakapan tidak disimpan.
- Hasil muncul di halaman Agent sebagai **kandidat**: lokasi, perkiraan agent, jumlah file, apakah formatnya bisa dibaca. Pengguna mengonfirmasi "lacak" atau "abaikan".
- Kandidat dengan format yang cocok dengan adapter yang ada (misal salinan folder Claude Code) langsung dibaca dengan adapter itu. Format yang tidak dikenal ditandai "terdeteksi, format belum didukung" — tidak ditebak angkanya.

**B. Mencari jejak agent di folder project**
- Cari penanda: `.claude/`, `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.gemini/`, `.cursor/`, `.cursorrules`, `.github/copilot-instructions.md`, `.codex/`, `.opencode/`, `.aider*`, `.windsurf*`, `.continue/`.
- Hasil: peta **project → agent yang dipakai/dikonfigurasi**, ditampilkan di halaman Project (badge agent) dan halaman Agent. Ini melengkapi agent yang tidak mencatat token (Gemini, Copilot, Cursor), tanpa menambah data biaya.

**Aturan pemindaian**
- Lewati: `node_modules`, `.git`, `Library/Caches`, `Library/Containers`, `.Trash`, `vendor`, `target`, `dist`, `build`, `.venv`, dan folder yang tidak bisa dibaca (tanpa memunculkan permintaan izin macOS). Folder yang dilindungi macOS (Desktop, Documents, Downloads) hanya disisir jika pengguna sudah memberi izin; jika belum, tampilkan keterangan, jangan memaksa.
- Pemindaian berprioritas rendah, bisa dibatalkan, dan tidak mengikuti symlink.
- Hasil pemindaian disimpan, jadi scan berikutnya hanya memeriksa yang berubah.

### 6.4 Harga

- Ambil `https://openrouter.ai/api/v1/models` saat aplikasi dibuka dan sekali sehari; simpan ke database lokal dengan tanggal ambil. Offline → pakai cache terakhir.
- Komponen harga: `prompt` (input), `completion` (output), `input_cache_read`, `input_cache_write`. Token reasoning dihitung dengan harga output.
- **Pencocokan nama model:** tabel pemetaan bawaan (misal `claude-opus-5-5` → `anthropic/claude-opus-5.5`, `gpt-5.6-luna` → `openai/gpt-5.6-luna`) + pencocokan otomatis yang menormalkan tanda hubung/titik dan prefix provider.
- Model yang tidak cocok → masuk daftar "harga tidak diketahui" di halaman Harga; pengguna bisa memetakan ke model OpenRouter atau mengisi harga manual. Biaya ditampilkan sebagai "—" + peringatan, bukan $0.
- Model internal yang tidak ditagih (misal `codex-auto-review`) bisa ditandai "abaikan".
- Biaya dihitung saat ditampilkan dari token × harga saat ini (harga historis tidak disimpan per event) — *keputusan saya, lihat §8*.

### 6.5 Mata uang

USD sebagai utama. Opsi tampilan Rupiah dengan kurs yang diisi manual di Pengaturan.

### 6.6 Tampilan

**Jendela utama**, dengan sidebar:

1. **Dashboard** — pilih periode (Minggu ini / Bulan ini / Minggu lalu / Bulan lalu / custom). Total biaya + token, persentase vs periode sebelumnya, grafik batang harian (bertumpuk per agent), top 5 project, top 5 sesi termahal, agent yang sedang berjalan sekarang.
2. **Mingguan / Bulanan** — grafik tren per minggu (12 minggu terakhir) dan per bulan (12 bulan terakhir), bisa dipecah per agent/model/project; tabel ringkasan per periode.
3. **Project** — daftar folder project dengan biaya, token, jumlah sesi, agent yang dipakai; klik → daftar sesi project itu.
4. **Sesi** — daftar semua sesi (agent, project, mulai, durasi, model, token, biaya), bisa difilter & diurutkan; klik → detail sesi: timeline per respons (token & biaya tiap giliran) dan pecahan input/output/cache.
5. **Model** — biaya & token per model, harga yang dipakai.
6. **Agent** — daftar agent terdeteksi: status (sedang jalan/pernah jalan/tidak ada data token), lokasi data, jumlah sesi, proses aktif beserta folder kerjanya.
7. **Harga** — sumber harga, waktu update terakhir, model tanpa harga, override manual.
8. **Pengaturan** — mata uang & kurs, jalankan saat login, interval refresh, ekspor CSV.

**Ikon menu bar** — menampilkan biaya hari ini; klik → ringkasan singkat (hari ini, minggu ini, bulan ini, agent yang sedang jalan) dan tombol buka jendela utama.

```
Menu bar:  ◉ $4.12
┌────────────────────────────┐
│ Hari ini     $4.12         │
│ Minggu ini  $31.80  ▲12%   │
│ Bulan ini  $142.30  ▲18%   │
│ Sedang jalan:              │
│  • Claude Code  backoffice-be │
│  • Codex        another    │
│ [Buka dashboard]           │
└────────────────────────────┘
```

**Ekspor:** CSV per sesi dan per hari, untuk periode yang dipilih.

### 6.7 Penyimpanan

SQLite di `~/Library/Application Support/AI Token Tracker/tracker.db`, berisi: agents, projects, sessions, usage_events, scan_state (offset per file), prices, model_mappings, settings.

**Database aplikasi adalah catatan permanen**, bukan sekadar cermin file agent. Alasannya, Claude Code menghapus file sesi yang lebih tua dari ~30 hari (di laptop ini sesi tertua tepat 30 hari), dan Codex memindahkan sesi ke `archived_sessions`. Konsekuensinya:
- Setiap usage event menyimpan datanya sendiri secara lengkap (token, model, waktu, cwd, project, session), tidak bergantung pada file sumber.
- File sumber yang terhapus, dipindah, atau mengecil **tidak** menghapus data yang sudah tercatat. Event baru tetap di-dedupe dengan ID stabil (ID pesan/giliran dari agent), supaya file yang dipindah (misalnya ke `archived_sessions`) atau sesi yang di-resume tidak terhitung dua kali.
- "Jalankan saat login" **aktif secara default**, supaya aplikasi sempat membaca sesi sebelum agent menghapusnya. Halaman Agent menampilkan peringatan jika aplikasi lama tidak berjalan dan ada risiko data terlewat.
- Data yang sudah terhapus sebelum aplikasi pertama kali dijalankan tidak bisa dipulihkan; grafik menampilkan sejak kapan data tersedia.
- Pengaturan menyediakan tombol "hapus semua data" dan ekspor CSV sebagai cadangan. Hanya metadata pemakaian — **isi percakapan tidak disimpan**, kecuali label sesi: potongan pesan pertama pengguna (maks. 80 karakter) supaya sesi mudah dikenali (*keputusan saya*). Semua tetap lokal.

### 6.8 Performa

- Pemindaian awal seluruh riwayat di background dengan progress; UI langsung bisa dipakai.
- Setelah itu inkremental via FSEvents (debounce ~2 dtk) + pemindaian proses tiap 5 dtk.
- Target: pemakaian CPU saat idle mendekati 0%, RAM < 150 MB.

### 6.9 Build & distribusi

- Tauri v2, frontend React + TypeScript + Vite, library grafik web (pilihan builder, misal ECharts/Recharts).
- Output: `.dmg` arm64, tidak ditandatangani.
- Opsi "jalankan saat login" (default **aktif**, lihat §6.7).
- Menutup jendela utama tidak menghentikan aplikasi; aplikasi tetap berjalan di menu bar. Keluar penuh lewat menu bar → Quit.

## 7. Yang diputuskan pemilik

- Tujuan: biaya + analisis mingguan & bulanan.
- Biaya berdasarkan harga per token, referensi dari OpenRouter.
- Hasil akhir: `.dmg` untuk macOS.
- Pendekatan: Tauri.
- Riwayat disimpan permanen di database aplikasi (§6.7).
- Project = root repo git; subfolder dan worktree digabung (§6.2a).
- Fitur pemindaian direktori, dua jenis: data agent yang belum dikenal (A) dan jejak agent di folder project (B) (§6.3a).

## 8. Keputusan saya (bisa dikoreksi)

- Agent yang dilacak di versi pertama: Claude Code, Codex, opencode (penuh); Gemini CLI, Copilot CLI, Claude desktop, VS Code (deteksi saja).
- USD utama + opsi Rupiah dengan kurs manual.
- Jendela utama + ikon menu bar dengan biaya hari ini.
- Biaya dihitung ulang dengan harga terbaru (bukan harga saat token dipakai).
- Label sesi dari 80 karakter pertama pesan pengguna, disimpan lokal.
- arm64 saja, tanpa tanda tangan.
- Ekspor CSV.
- Minggu dimulai hari **Senin**; semua pengelompokan harian/mingguan/bulanan memakai zona waktu lokal laptop (WIB), bukan UTC.
- Harga cache write: jika provider punya beberapa tingkat (misal cache 5 menit vs 1 jam di Claude) dan OpenRouter hanya memberi satu harga, pakai harga dari OpenRouter untuk semuanya.
- Menutup jendela → aplikasi tetap jalan di menu bar.
- Pemindaian direktori: lokasi default home folder, jalan sekali di awal + tombol "Scan ulang" (tidak terus-menerus); kandidat dari scan A harus dikonfirmasi sebelum dilacak.

## 9. Diserahkan ke builder

- Skema database detail, struktur kode Rust/TS, library grafik dan styling UI.
- Detail parsing tiap format (dengan aturan dedupe di §6.2).
- Algoritma pencocokan nama model, selama model tak dikenal selalu terlihat.
