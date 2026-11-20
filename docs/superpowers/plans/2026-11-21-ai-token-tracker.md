# AI Token Tracker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Aplikasi macOS (Tauri v2, `.dmg`) yang otomatis mendeteksi agent AI di laptop, mencatat pemakaian token per agent/project/sesi/model secara permanen, menghitung biaya dari harga OpenRouter, dan menampilkan analisis mingguan & bulanan.

**Architecture:** Core Rust membaca data agent (read-only) lewat satu *adapter* per agent, menormalkan menjadi `UsageEvent`, dan menyimpannya permanen ke SQLite milik aplikasi. Biaya dihitung saat query dengan join ke tabel `resolved_prices` (hasil pencocokan model ↔ OpenRouter + override). UI React memanggil Tauri commands; ikon menu bar menampilkan biaya hari ini.

**Tech Stack:** Tauri 2.x (feature `tray-icon`), Rust stable (cargo 1.100), `rusqlite` (feature `bundled`), `notify` + `notify-debouncer-mini`, `sysinfo`, `reqwest` (blocking, rustls), `walkdir`, `chrono`, `serde`/`serde_json`, `anyhow`, `tauri-plugin-autostart`, `tauri-plugin-dialog`; frontend React 18 + TypeScript + Vite, `recharts`, `@tauri-apps/api` v2, `vitest`. Dev: `tempfile`.

**Spec:** `docs/superpowers/specs/2026-11-21-ai-token-tracker-design.md`

## Global Constraints

- Root proyek: `/Users/ultravoucher/ai-token-tracker` (repo git baru; Task 1 menjalankan `git init`).
- Target: macOS arm64 (`aarch64-apple-darwin`), `.dmg` tidak ditandatangani.
- Nama aplikasi: `AI Token Tracker`; bundle identifier `com.aitokentracker.app`; versi `0.1.0`.
- Database: `~/Library/Application Support/AI Token Tracker/tracker.db`. Database ini catatan **permanen**: file sumber yang hilang/mengecil tidak pernah menghapus event.
- Data agent **hanya-baca**. SQLite opencode dibuka dengan `OpenFlags::SQLITE_OPEN_READ_ONLY`. Tidak ada kode yang menulis/menghapus di bawah `~/.claude`, `~/.codex`, `~/.gemini`, `~/.copilot`, `~/.local/share/opencode`.
- Satu-satunya koneksi keluar: `GET https://openrouter.ai/api/v1/models`.
- Isi percakapan tidak disimpan. Pengecualian satu-satunya: judul sesi = judul bawaan agent (Claude `ai-title`, opencode `title`) atau, jika tidak ada, 80 karakter pertama pesan pengguna pertama.
- Model tanpa harga → biaya `NULL`/"—" + dihitung sebagai `unpriced_tokens`; **tidak pernah** dihitung $0. Hanya status `ignored` yang bernilai $0.
- Normalisasi token di semua adapter: `input` = input **tanpa** cache; `output` = output **termasuk** reasoning; `reasoning` = bagian dari output (informasi saja, tidak ditagih terpisah); `cache_read`, `cache_write` terpisah.
- Waktu disimpan sebagai epoch ms UTC. Pengelompokan hari/minggu/bulan memakai offset zona waktu lokal (`tz_offset_secs`, di laptop ini +25200 / WIB). Minggu mulai **Senin**.
- Default settings: mata uang USD, `idr_rate` kosong, `launch_at_login = true`, `process_poll_secs = 5`, `scan_roots = [home]`.
- Interval: watcher debounce 2 dtk; poll proses 5 dtk; refresh harga saat start + tiap 24 jam; heartbeat `last_seen_ms` tiap 5 menit; tray update tiap 30 dtk dan setelah ingest.
- Bahasa UI: Indonesia.
- Setiap pesan commit diakhiri baris `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Fixture tes adalah data **sintetis** yang ditulis di plan ini — jangan menyalin file sesi asli ke repo.

## Review Focus

1. **Baris JSONL terakhir yang belum selesai ditulis** (agent sedang streaming) → baris itu dilewati, offset tidak melewatinya, dan terbaca di scan berikutnya. Tes: Task 8 `partial_trailing_line_is_read_next_time`.
2. **Sesi Codex yang sama ada di `sessions/` dan `archived_sessions/`** → dihitung sekali. Tes: Task 8 `codex_archived_copy_counted_once`.
3. **Model tanpa harga** (mis. `gpt-5.6-luna-fast`, `claude-opus-5-5[1m]` yang tak ada di katalog) → total menampilkan biaya yang diketahui + jumlah token tanpa harga, bukan $0. Tes: Task 10 `unpriced_model_excluded_from_cost_and_counted`.
4. **Batas hari zona waktu** — event `2026-11-20T18:30:00Z` = 21 Nov 01:30 WIB → masuk hari 21 dan minggu/bulan yang sesuai. Tes: Task 10 `event_after_utc_evening_lands_on_next_local_day`.
5. **Start pertama tanpa internet dan tanpa cache harga** → aplikasi tetap jalan, semua model berstatus `unknown`, status harga melaporkan offline. Tes: Task 9 `refresh_failure_keeps_old_prices_and_reports_offline`.

---

## File Structure

```
ai-token-tracker/
  package.json, vite.config.ts, tsconfig.json, index.html
  src/
    main.tsx                 entry React
    App.tsx                  sidebar + pemilihan halaman + listener "data-updated"
    api.ts                   wrapper typed untuk semua Tauri command
    types.ts                 tipe TS yang mencerminkan struct Rust (camelCase)
    lib/format.ts            format uang/token/persen  (+ format.test.ts)
    components/PeriodPicker.tsx, StatCard.tsx, CostCell.tsx, DailyStackedBar.tsx, DataTable.tsx
    pages/Dashboard.tsx, Trends.tsx, Projects.tsx, Sessions.tsx, SessionDetail.tsx,
          Models.tsx, Agents.tsx, Pricing.tsx, Settings.tsx
  src-tauri/
    Cargo.toml, tauri.conf.json, capabilities/default.json, icons/
    src/main.rs              panggil ai_token_tracker_lib::run()
    src/lib.rs               setup app, state, thread background, registrasi command
    src/model.rs             AgentKind, UsageEvent, SessionMeta, TokenStatus, ParseOutput
    src/db/mod.rs            Db: open, migrasi, apply_parse, source_state, settings, meta
    src/db/schema.sql        skema v1
    src/project.rs           ProjectResolver (root git / worktree)
    src/adapters/mod.rs      trait Adapter, Source, SourceState, registry
    src/adapters/jsonl.rs    read_complete_lines
    src/adapters/claude_code.rs, codex.rs, opencode.rs, gemini.rs, copilot.rs, presence.rs
    src/ingest.rs            Ingestor (inkremental, transaksi per source)
    src/watcher.rs           FSEvents → ingest
    src/pricing/mod.rs       rebuild_resolved, status harga
    src/pricing/openrouter.rs  fetch + parse katalog
    src/pricing/matcher.rs   normalize + match_model
    src/period.rs            PeriodSpec → PeriodRange
    src/query.rs             agregasi untuk semua halaman
    src/process.rs           deteksi proses agent
    src/discovery/walk.rs, data_scan.rs, footprint.rs
    src/settings.rs          Settings + default
    src/export.rs            CSV
    src/tray.rs              ikon & menu bar
    src/commands.rs          semua #[tauri::command]
    tests/fixtures/...       fixture sintetis
```

---

### Task 1: Scaffold aplikasi Tauri + repo git

**Files:**
- Create: seluruh scaffold `create-tauri-app` (template `react-ts`, package manager npm) di `/Users/ultravoucher/ai-token-tracker`
- Modify: `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`
- Create: `.gitignore`

**Interfaces:**
- Produces: crate lib `ai_token_tracker_lib` dengan `pub fn run()`; `npm run tauri dev` membuka jendela.

- [ ] **Step 1:** `cd /Users/ultravoucher && npm create tauri-app@latest ai-token-tracker-tmp -- --template react-ts --manager npm --yes`, lalu pindahkan isinya ke `/Users/ultravoucher/ai-token-tracker` (folder `docs/` yang sudah ada tetap dipertahankan) dan hapus folder tmp.
- [ ] **Step 2:** Set di `tauri.conf.json`: `productName` `"AI Token Tracker"`, `identifier` `"com.aitokentracker.app"`, `version` `"0.1.0"`, jendela utama `label: "main"`, `title: "AI Token Tracker"`, `width: 1200`, `height: 800`, `bundle.targets: ["dmg", "app"]`.
- [ ] **Step 3:** Tambah dependensi Rust di `Cargo.toml`: `tauri` (features `["tray-icon"]`), `tauri-plugin-autostart`, `tauri-plugin-dialog`, `rusqlite` (`bundled`), `notify`, `notify-debouncer-mini`, `sysinfo`, `reqwest` (`default-features=false`, `["blocking","json","rustls-tls"]`), `walkdir`, `chrono` (`serde`), `serde` (`derive`), `serde_json`, `anyhow`; dev: `tempfile`. Tambah npm: `recharts`, `@tauri-apps/plugin-dialog`, `@tauri-apps/plugin-autostart`; dev: `vitest`.
- [ ] **Step 4:** `.gitignore` berisi `node_modules/`, `dist/`, `src-tauri/target/`, `.DS_Store`.
- [ ] **Step 5: Verifikasi** — `cd src-tauri && cargo check` → selesai tanpa error; `npm run build` → sukses.
- [ ] **Step 6: Commit** — `git init && git add -A && git commit -m "chore: scaffold Tauri v2 React app"` (spec ikut ter-commit).

---

### Task 2: Model data + database

**Files:**
- Create: `src-tauri/src/model.rs`, `src-tauri/src/db/mod.rs`, `src-tauri/src/db/schema.sql`
- Modify: `src-tauri/src/lib.rs` (deklarasi `mod`)

**Interfaces:**
- Produces:
```rust
// model.rs  (semua derive Debug, Clone, PartialEq, Serialize, Deserialize; serde rename_all = "kebab-case" untuk enum)
pub enum AgentKind { ClaudeCode, Codex, OpenCode, GeminiCli, CopilotCli, ClaudeDesktop, VsCode }
impl AgentKind { pub fn as_str(&self) -> &'static str; pub fn from_str(s: &str) -> Option<Self>; pub fn label(&self) -> &'static str /* "Claude Code", ... */ }
pub enum TokenStatus { Recorded, NotRecorded }
pub struct UsageEvent { pub agent: AgentKind, pub event_key: String, pub session_id: String, pub ts_ms: i64,
  pub cwd: Option<String>, pub model: String, pub provider_hint: String,
  pub input: i64, pub output: i64, pub reasoning: i64, pub cache_read: i64, pub cache_write: i64 }
pub struct SessionMeta { pub agent: AgentKind, pub session_id: String, pub cwd: Option<String>, pub title: Option<String>,
  pub started_ms: Option<i64>, pub updated_ms: Option<i64>, pub token_status: TokenStatus }
#[derive(Default)] pub struct ParseOutput { pub events: Vec<UsageEvent>, pub sessions: Vec<SessionMeta> }

// db/mod.rs
pub struct Db { pub conn: rusqlite::Connection }
impl Db {
  pub fn open(path: &Path) -> anyhow::Result<Db>;          // buat folder induk, PRAGMA journal_mode=WAL, migrasi
  pub fn open_in_memory() -> anyhow::Result<Db>;
  pub fn apply_parse(&mut self, source_path: &str, agent: AgentKind, out: &ParseOutput, state: &SourceState) -> anyhow::Result<()>; // satu transaksi
  pub fn source_state(&self, source_path: &str) -> anyhow::Result<Option<SourceState>>;
  pub fn get_meta(&self, key: &str) -> anyhow::Result<Option<String>>;
  pub fn set_meta(&self, key: &str, value: &str) -> anyhow::Result<()>;
  pub fn wipe_all(&mut self) -> anyhow::Result<()>;       // hapus semua baris kecuali settings
}
```
`SourceState` didefinisikan di Task 4 (`adapters/mod.rs`); di task ini buat dulu di sana sebagai `pub struct SourceState { pub offset: u64, pub size: u64, pub mtime_ms: i64, pub context: serde_json::Value }` dengan `Default` (context = `Value::Null`).

- [ ] **Step 1: Tulis skema `schema.sql`** (dijalankan saat `PRAGMA user_version = 0`, lalu set ke 1):
```sql
CREATE TABLE events(event_key TEXT PRIMARY KEY, agent TEXT NOT NULL, session_id TEXT NOT NULL, ts_ms INTEGER NOT NULL,
  cwd TEXT, model TEXT NOT NULL, provider_hint TEXT NOT NULL, input INTEGER NOT NULL, output INTEGER NOT NULL,
  reasoning INTEGER NOT NULL, cache_read INTEGER NOT NULL, cache_write INTEGER NOT NULL);
CREATE INDEX events_ts ON events(ts_ms);
CREATE INDEX events_session ON events(agent, session_id);
CREATE INDEX events_model ON events(provider_hint, model);
CREATE TABLE sessions(agent TEXT NOT NULL, session_id TEXT NOT NULL, cwd TEXT, title TEXT, started_ms INTEGER,
  updated_ms INTEGER, token_status TEXT NOT NULL, PRIMARY KEY(agent, session_id));
CREATE TABLE cwd_projects(cwd TEXT PRIMARY KEY, project_key TEXT NOT NULL, is_git INTEGER NOT NULL);
CREATE TABLE source_state(path TEXT PRIMARY KEY, agent TEXT NOT NULL, offset INTEGER NOT NULL, size INTEGER NOT NULL,
  mtime_ms INTEGER NOT NULL, context TEXT NOT NULL);
CREATE TABLE prices(openrouter_id TEXT PRIMARY KEY, prompt REAL NOT NULL, completion REAL NOT NULL,
  cache_read REAL, cache_write REAL, fetched_ms INTEGER NOT NULL);
CREATE TABLE model_overrides(provider_hint TEXT NOT NULL, raw_model TEXT NOT NULL, action TEXT NOT NULL, -- map|ignore|manual
  openrouter_id TEXT, prompt REAL, completion REAL, cache_read REAL, cache_write REAL, PRIMARY KEY(provider_hint, raw_model));
CREATE TABLE resolved_prices(provider_hint TEXT NOT NULL, raw_model TEXT NOT NULL, status TEXT NOT NULL, -- matched|mapped|manual|ignored|unknown
  openrouter_id TEXT, prompt REAL, completion REAL, cache_read REAL, cache_write REAL, PRIMARY KEY(provider_hint, raw_model));
CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE app_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE discovery_candidates(path TEXT PRIMARY KEY, guessed_agent TEXT, known_format TEXT, file_count INTEGER NOT NULL,
  decision TEXT NOT NULL DEFAULT 'pending', found_ms INTEGER NOT NULL);
CREATE TABLE discovery_denied(path TEXT PRIMARY KEY, found_ms INTEGER NOT NULL);
CREATE TABLE project_footprints(project_key TEXT NOT NULL, agent TEXT NOT NULL, marker TEXT NOT NULL,
  PRIMARY KEY(project_key, agent, marker));
INSERT INTO model_overrides(provider_hint, raw_model, action) VALUES ('anthropic','<synthetic>','ignore'), ('openai','codex-auto-review','ignore');
```
- [ ] **Step 2: Tulis tes gagal** di `db/mod.rs`:
```rust
#[test] fn apply_parse_is_idempotent_and_last_write_wins() {
  // apply event_key "k1" output=10, lalu apply lagi "k1" output=50
  // assert: SELECT COUNT(*) FROM events == 1; output == 50
}
#[test] fn session_merge_keeps_min_start_max_update_and_latest_title() {
  // apply SessionMeta(started=200, updated=300, title=None), lalu (started=100, updated=250, title=Some("T"))
  // assert: started_ms==100, updated_ms==300, title=="T"
}
#[test] fn source_state_roundtrip() { /* apply_parse dengan state offset=42 context={"m":"x"}; source_state() mengembalikan sama */ }
#[test] fn wipe_all_keeps_settings() { /* settings punya 1 baris; events punya 1 baris; setelah wipe: events 0, settings 1 */ }
```
- [ ] **Step 3:** `cd src-tauri && cargo test db::` → FAIL (belum ada implementasi).
- [ ] **Step 4: Implementasi.** Upsert events dengan `ON CONFLICT(event_key) DO UPDATE SET` semua kolom. Upsert sessions: `started_ms = MIN(non-null)`, `updated_ms = MAX`, `title = COALESCE(excluded.title, title)`, `cwd = COALESCE(cwd, excluded.cwd)`, `token_status = 'recorded'` jika salah satu recorded.
- [ ] **Step 5:** `cargo test db::` → PASS.
- [ ] **Step 6: Commit** `feat(db): schema v1 and idempotent ingest writes`.

---

### Task 3: Resolver project (root git / worktree)

**Files:**
- Create: `src-tauri/src/project.rs`

**Interfaces:**
- Produces:
```rust
pub struct ResolvedProject { pub project_key: String, pub is_git: bool }
pub fn resolve_project(cwd: &Path) -> ResolvedProject;
pub fn ensure_cwd_projects(db: &Db, cwds: &[String]) -> anyhow::Result<()>; // resolve hanya cwd yang belum ada di cwd_projects
pub fn display_name(project_key: &str) -> String; // nama folder terakhir
```

- [ ] **Step 1: Tulis tes gagal** (pakai `tempfile::tempdir`):
```rust
#[test] fn subfolder_resolves_to_repo_root()      // repo/.git (dir), cwd=repo/src/api → key == repo, is_git
#[test] fn worktree_resolves_to_main_repo()       // main/.git (dir) + main/.git/worktrees/wt; wt/.git (file) berisi "gitdir: <main>/.git/worktrees/wt" → key == main
#[test] fn claude_worktree_inside_repo()          // repo/.claude/worktrees/fitur-x/.git (file) → gitdir repo/.git/worktrees/fitur-x → key == repo
#[test] fn non_git_folder_is_its_own_project()    // key == cwd, !is_git
#[test] fn missing_folder_falls_back_to_cwd()     // path tidak ada → key == cwd, !is_git
#[test] fn ensure_cwd_projects_does_not_overwrite_existing() // baris lama tetap walau folder kini hilang
```
- [ ] **Step 2:** `cargo test project::` → FAIL.
- [ ] **Step 3: Implementasi.** Naik ke atas mencari `.git`. Jika `.git` file berisi `gitdir: X` dan X mengandung `/.git/worktrees/`, root = bagian sebelum `/.git/worktrees/`. Jika `.git` file tanpa pola worktree (submodule), pakai folder itu. Path dikanonikalisasi bila ada.
- [ ] **Step 4:** `cargo test project::` → PASS.
- [ ] **Step 5: Commit** `feat: resolve project key from git root and worktrees`.

---

### Task 4: Trait adapter + adapter Claude Code

**Files:**
- Create: `src-tauri/src/adapters/mod.rs`, `src-tauri/src/adapters/jsonl.rs`, `src-tauri/src/adapters/claude_code.rs`
- Test fixtures: `src-tauri/tests/fixtures/claude/projects/-tmp-demo/sess-1.jsonl`, `src-tauri/tests/fixtures/claude/projects/-tmp-demo/sess-1/subagents/agent-x.jsonl`

**Interfaces:**
- Produces:
```rust
// adapters/mod.rs
pub enum SourceKind { Jsonl, Sqlite, Dir }
pub struct Source { pub path: PathBuf, pub kind: SourceKind }
pub struct SourceState { /* lihat Task 2 */ }
pub trait Adapter: Send + Sync {
  fn kind(&self) -> AgentKind;
  fn root(&self) -> &Path;
  fn detect(&self) -> bool;                         // root ada
  fn sources(&self) -> Vec<Source>;
  fn parse(&self, src: &Source, prev: &SourceState) -> anyhow::Result<(ParseOutput, SourceState)>;
}
pub fn adapter_for(kind: AgentKind, root: PathBuf) -> Box<dyn Adapter>;
pub fn default_adapters(home: &Path) -> Vec<Box<dyn Adapter>>;  // diisi bertahap di Task 4–7
pub fn truncate_title(s: &str) -> String;                      // trim, ganti newline dgn spasi, maks 80 char (char, bukan byte)

// adapters/jsonl.rs
pub fn read_complete_lines(path: &Path, offset: u64) -> std::io::Result<(Vec<String>, u64)>; // hanya baris berakhir '\n'; return offset baru

// adapters/claude_code.rs
pub struct ClaudeCodeAdapter { pub root: PathBuf } // default root ~/.claude
```

**Aturan Claude Code:**
- Sources: `root/projects/*/*.jsonl` dan `root/projects/*/*/subagents/*.jsonl`.
- Baris `type=="assistant"` yang punya `message.usage` → event. `event_key = "claude:" + message.id`, fallback `"claude:uuid:" + uuid`. Baris duplikat dengan `message.id` sama → event yang sama (upsert, baris terakhir menang).
- `session_id`: jika path berada di `.../<X>/subagents/`, pakai `X`; selain itu `sessionId` baris.
- Token: `input=input_tokens`, `cache_write=cache_creation_input_tokens`, `cache_read=cache_read_input_tokens`, `output=output_tokens`, `reasoning=output_tokens_details.thinking_tokens` (0 jika tidak ada). `provider_hint="anthropic"`, `model=message.model`, `ts_ms` dari `timestamp` (RFC3339), `cwd` dari baris.
- Judul: `ai-title.aiTitle` terakhir; jika tidak ada, pesan `type=="user"` pertama yang `message.content` berupa string → `truncate_title`. Judul fallback disimpan di `context.title_fallback_done` agar tidak menimpa ketika parse inkremental.
- SessionMeta: `started_ms`/`updated_ms` = min/max timestamp baris pada parse ini; `token_status=Recorded`.

- [ ] **Step 1: Tulis fixture `sess-1.jsonl`** (setiap baris diakhiri `\n`):
```
{"type":"ai-title","aiTitle":"Fix login bug","sessionId":"sess-1"}
{"type":"user","sessionId":"sess-1","cwd":"/tmp/demo","timestamp":"2026-11-20T01:00:00.000Z","uuid":"u0","message":{"role":"user","content":"Please fix the login bug"}}
{"type":"assistant","sessionId":"sess-1","cwd":"/tmp/demo","timestamp":"2026-11-20T01:00:05.000Z","uuid":"u1","requestId":"r1","message":{"id":"msg_A","model":"claude-opus-5-5","usage":{"input_tokens":10,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":50,"output_tokens_details":{"thinking_tokens":20}}}}
{"type":"assistant","sessionId":"sess-1","cwd":"/tmp/demo","timestamp":"2026-11-20T01:00:05.500Z","uuid":"u2","requestId":"r1","message":{"id":"msg_A","model":"claude-opus-5-5","usage":{"input_tokens":10,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":50,"output_tokens_details":{"thinking_tokens":20}}}}
{"type":"assistant","sessionId":"sess-1","cwd":"/tmp/demo","timestamp":"2026-11-20T01:01:00.000Z","uuid":"u3","message":{"id":"msg_B","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0}}}
```
dan `subagents/agent-x.jsonl`:
```
{"type":"assistant","isSidechain":true,"sessionId":"sess-1","cwd":"/tmp/demo","timestamp":"2026-11-20T01:02:00.000Z","uuid":"u4","message":{"id":"msg_C","model":"claude-sonnet-5-5","usage":{"input_tokens":5,"cache_read_input_tokens":200,"output_tokens":30}}}
```
- [ ] **Step 2: Tulis tes gagal:**
```rust
#[test] fn read_complete_lines_stops_before_partial_line() // file "a\nb\npar" → lines ["a","b"], offset 4
#[test] fn claude_dedupes_streamed_duplicates()   // parse sess-1.jsonl → events dengan key "claude:msg_A" tepat 1 di output setelah dedupe-per-parse; total distinct keys = 2
#[test] fn claude_maps_usage_fields()             // msg_A: input 10, cache_write 100, cache_read 1000, output 50, reasoning 20, model "claude-opus-5-5", provider "anthropic", cwd "/tmp/demo"
#[test] fn claude_subagent_events_join_parent_session() // agent-x.jsonl → msg_C session_id "sess-1"
#[test] fn claude_title_prefers_ai_title()        // SessionMeta.title == Some("Fix login bug")
#[test] fn claude_sources_include_subagent_files() // sources() pada root fixture → 2 file
#[test] fn truncate_title_limits_to_80_chars()    // 100 char 'é' → 80 char
```
- [ ] **Step 3:** `cargo test adapters::` → FAIL.
- [ ] **Step 4:** Implementasi sesuai aturan di atas. Dalam satu parse, dedupe event berdasarkan `event_key` (yang terakhir menang).
- [ ] **Step 5:** `cargo test adapters::` → PASS.
- [ ] **Step 6: Commit** `feat(adapters): adapter trait and Claude Code parser`.

---

### Task 5: Adapter Codex

**Files:**
- Create: `src-tauri/src/adapters/codex.rs`
- Test fixture: `src-tauri/tests/fixtures/codex/sessions/2026/11/20/rollout-cx-1.jsonl`

**Interfaces:**
- Produces: `pub struct CodexAdapter { pub root: PathBuf }` (default `~/.codex`), terdaftar di `adapter_for`/`default_adapters`.

**Aturan Codex:**
- Sources: `root/sessions/**/*.jsonl` dan `root/archived_sessions/**/*.jsonl`.
- `session_meta.payload` → `session_id = payload.id`, `cwd`, `provider_hint = payload.model_provider` (default `"openai"`).
- `turn_context.payload` → model & cwd aktif saat ini.
- `event_msg` dengan `payload.type=="user_message"` pertama → judul (`truncate_title(payload.message)`).
- `event_msg` dengan `payload.type=="token_count"` dan `info` tidak null → event dari `info.last_token_usage`: `input = input_tokens - cached_input_tokens`, `cache_read = cached_input_tokens`, `cache_write = cache_write_input_tokens` (0 jika tidak ada), `output = output_tokens`, `reasoning = reasoning_output_tokens`. `event_key = "codex:{session_id}:{info.total_token_usage.total_tokens}"` (event kumulatif yang sama → key sama).
- `SourceState.context` menyimpan `{session_id, cwd, model, provider_hint, title_done}` supaya parse dari tengah file tetap tahu model & sesi.

- [ ] **Step 1: Tulis fixture:**
```
{"timestamp":"2026-11-20T02:00:00.000Z","type":"session_meta","payload":{"id":"cx-1","cwd":"/tmp/demo","model_provider":"openai","timestamp":"2026-11-20T02:00:00.000Z"}}
{"timestamp":"2026-11-20T02:00:01.000Z","type":"turn_context","payload":{"cwd":"/tmp/demo/sub","model":"gpt-6-sol"}}
{"timestamp":"2026-11-20T02:00:01.500Z","type":"event_msg","payload":{"type":"user_message","message":"Refactor the parser module please"}}
{"timestamp":"2026-11-20T02:00:02.000Z","type":"event_msg","payload":{"type":"token_count","info":null}}
{"timestamp":"2026-11-20T02:00:03.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":400,"output_tokens":200,"reasoning_output_tokens":50,"total_tokens":1200},"last_token_usage":{"input_tokens":1000,"cached_input_tokens":400,"output_tokens":200,"reasoning_output_tokens":50,"total_tokens":1200}}}}
{"timestamp":"2026-11-20T02:00:03.500Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":400,"output_tokens":200,"reasoning_output_tokens":50,"total_tokens":1200},"last_token_usage":{"input_tokens":1000,"cached_input_tokens":400,"output_tokens":200,"reasoning_output_tokens":50,"total_tokens":1200}}}}
{"timestamp":"2026-11-20T02:00:10.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":2500,"cached_input_tokens":1400,"output_tokens":300,"reasoning_output_tokens":60,"total_tokens":2800},"last_token_usage":{"input_tokens":1500,"cached_input_tokens":1000,"output_tokens":100,"reasoning_output_tokens":10,"total_tokens":1600}}}}
```
- [ ] **Step 2: Tulis tes gagal:**
```rust
#[test] fn codex_emits_one_event_per_distinct_cumulative_total() // 2 event: keys "codex:cx-1:1200", "codex:cx-1:2800"
#[test] fn codex_splits_cached_from_input()   // e1: input 600, cache_read 400, output 200, reasoning 50; e2: input 500, cache_read 1000, output 100, reasoning 10
#[test] fn codex_uses_turn_context_model_and_cwd() // model "gpt-6-sol", cwd "/tmp/demo/sub", provider "openai"
#[test] fn codex_title_from_first_user_message() // "Refactor the parser module please"
#[test] fn codex_resumes_mid_file_with_context()  // parse 5 baris pertama (offset dari read_complete_lines), lalu parse sisanya dgn state → e2 tetap model "gpt-6-sol", session "cx-1"
```
- [ ] **Step 3:** `cargo test adapters::codex` → FAIL.
- [ ] **Step 4:** Implementasi.
- [ ] **Step 5:** `cargo test adapters::codex` → PASS.
- [ ] **Step 6: Commit** `feat(adapters): Codex rollout parser`.

---

### Task 6: Adapter opencode

**Files:**
- Create: `src-tauri/src/adapters/opencode.rs`

**Interfaces:**
- Produces: `pub struct OpenCodeAdapter { pub root: PathBuf }` (default `~/.local/share/opencode`); source tunggal `root/opencode.db` (`SourceKind::Sqlite`).

**Aturan opencode:**
- Buka `OpenFlags::SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX`.
- Sesi: `SELECT id, directory, title, time_created, time_updated FROM session_v2 WHERE time_updated > ?1`.
- Pesan: `SELECT m.id, m.session_id, m.time_updated, m.data, s.directory FROM session_message m JOIN session_v2 s ON s.id = m.session_id WHERE m.type='assistant' AND m.time_updated > ?1`. Hanya yang `data.tokens` ada.
- Mapping: `input=tokens.input`, `output=tokens.output`, `reasoning=tokens.reasoning`, `cache_read=tokens.cache.read`, `cache_write=tokens.cache.write`, `model=data.model.id`, `provider_hint=data.model.providerID`, `ts_ms=data.time.created`, `event_key="opencode:"+m.id`. Field `cost` milik opencode diabaikan.
- `SourceState.offset` = `MAX(time_updated)` yang sudah dibaca (sesi & pesan); bukan offset byte.

- [ ] **Step 1: Tulis tes gagal** — tes membuat DB sementara dengan subset kolom (`session_v2(id, project_id, slug, directory, title, version, time_created, time_updated)`, `session_message(id, session_id, type, seq, time_created, time_updated, data)`), isi 1 sesi (`directory="/tmp/oc"`, `title="Explore repo"`) dan 2 pesan: 1 `assistant` dengan data `{"time":{"created":1795136400000},"model":{"id":"gpt-5.6-luna-fast","providerID":"openai"},"tokens":{"input":5603,"output":173,"reasoning":32,"cache":{"read":1536,"write":0}}}`, 1 `user`.
```rust
#[test] fn opencode_reads_assistant_tokens()   // 1 event: input 5603, output 173, reasoning 32, cache_read 1536, model "gpt-5.6-luna-fast", provider "openai", cwd "/tmp/oc"
#[test] fn opencode_session_title()            // SessionMeta.title == Some("Explore repo")
#[test] fn opencode_incremental_uses_time_updated() // parse ke-2 dgn state hasil parse ke-1 → 0 event
#[test] fn opencode_never_writes()             // checksum file db sebelum == sesudah parse
```
- [ ] **Step 2:** `cargo test adapters::opencode` → FAIL.
- [ ] **Step 3:** Implementasi.
- [ ] **Step 4:** `cargo test adapters::opencode` → PASS.
- [ ] **Step 5: Commit** `feat(adapters): opencode SQLite reader`.

---

### Task 7: Adapter Gemini, Copilot, dan deteksi-saja

**Files:**
- Create: `src-tauri/src/adapters/gemini.rs`, `src-tauri/src/adapters/copilot.rs`, `src-tauri/src/adapters/presence.rs`
- Test fixture: `src-tauri/tests/fixtures/gemini/tmp/abc/chats/session-2026-11-20T03-00-g1.jsonl`, `src-tauri/tests/fixtures/gemini/tmp/abc/.project_root`

**Interfaces:**
- Produces: `GeminiAdapter { root }` (default `~/.gemini`), `CopilotAdapter { root }` (default `~/.copilot`), `PresenceAdapter { kind, root }` untuk `ClaudeDesktop` (`~/Library/Application Support/Claude`) dan `VsCode` (`~/Library/Application Support/Code`). `default_adapters(home)` kini mengembalikan ketujuh adapter.

**Aturan:**
- Gemini — sources `root/tmp/*/chats/session-*.jsonl`. Baris pertama memberi `sessionId`, `startTime`; `updated_ms` = mtime file; `cwd` = isi `../.project_root` (trim). Jika ada pesan dalam array `messages` (top-level atau di bawah `$set`) dengan `type=="gemini"` dan objek `tokens` → event: `input = tokens.input - tokens.cached`, `cache_read = tokens.cached`, `output = tokens.output + tokens.thoughts`, `reasoning = tokens.thoughts`, `model = message.model`, `provider_hint = "google"`, key `"gemini:{sessionId}:{message.id}"`; `token_status = Recorded` jika ada event, selain itu `NotRecorded`. Gemini selalu di-parse ulang penuh (file kecil, format `$set` menimpa): offset tetap 0, pakai `size`+`mtime` untuk skip.
- Copilot — sources `root/session-state/*` (Dir). SessionMeta: `session_id` = nama folder, `started_ms`/`updated_ms` = mtime folder, `cwd = None`, `NotRecorded`.
- Presence — `detect()` = root ada; `sources()` kosong.

- [ ] **Step 1: Fixture Gemini** — `.project_root` berisi `/tmp/gem`; file sesi:
```
{"sessionId":"g1","projectHash":"abc","startTime":"2026-11-20T03:00:00.000Z","lastUpdated":"2026-11-20T03:05:00.000Z","kind":"main"}
{"$set":{"messages":[{"id":"m1","timestamp":"2026-11-20T03:00:10.000Z","type":"user","content":"hi"},{"id":"m2","timestamp":"2026-11-20T03:00:12.000Z","type":"gemini","model":"gemini-3-pro","tokens":{"input":900,"output":100,"cached":300,"thoughts":40,"tool":0,"total":1040}}]}}
```
- [ ] **Step 2: Tulis tes gagal:**
```rust
#[test] fn gemini_session_meta_and_project_root() // session "g1", cwd "/tmp/gem"
#[test] fn gemini_tokens_when_present()          // 1 event: input 600, cache_read 300, output 140, reasoning 40, provider "google"; status Recorded
#[test] fn gemini_without_tokens_is_not_recorded() // file hanya baris pertama → 0 event, NotRecorded
#[test] fn copilot_sessions_from_state_dirs()    // tempdir dengan 2 folder → 2 SessionMeta NotRecorded
#[test] fn presence_detects_only()               // root ada → detect true, sources kosong
#[test] fn default_adapters_has_seven_kinds()
```
- [ ] **Step 3:** `cargo test adapters::` → FAIL.
- [ ] **Step 4:** Implementasi.
- [ ] **Step 5:** `cargo test adapters::` → PASS.
- [ ] **Step 6: Commit** `feat(adapters): Gemini, Copilot and presence detection`.

---

### Task 8: Ingest inkremental + watcher

**Files:**
- Create: `src-tauri/src/ingest.rs`, `src-tauri/src/watcher.rs`

**Interfaces:**
- Consumes: `Adapter` (Task 4), `Db::apply_parse/source_state` (Task 2), `ensure_cwd_projects` (Task 3).
- Produces:
```rust
pub struct IngestReport { pub sources_read: usize, pub events: usize, pub sessions: usize }
pub struct Progress { pub agent: AgentKind, pub done: usize, pub total: usize }
pub fn ingest_adapter(db: &Mutex<Db>, a: &dyn Adapter, on_progress: &dyn Fn(Progress)) -> anyhow::Result<IngestReport>;
pub fn ingest_all(db: &Mutex<Db>, adapters: &[Box<dyn Adapter>], on_progress: &dyn Fn(Progress)) -> anyhow::Result<IngestReport>;
// watcher.rs
pub fn spawn_watcher(roots: Vec<(AgentKind, PathBuf)>, on_change: impl Fn(AgentKind) + Send + 'static) -> anyhow::Result<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>>;
```

**Aturan:**
- Per source: baca `size`/`mtime`. Jika sama dengan state → skip. Jika `size < offset` (file ditulis ulang) → parse dari offset 0 dengan `context = Null` (upsert idempoten mencegah dobel). Source yang hilang dari disk → tidak melakukan apa-apa (event tetap).
- Parse dilakukan **di luar** lock DB; lock hanya saat `apply_parse` + `ensure_cwd_projects` untuk cwd baru dari event & sesi.
- Watcher: debounce 2 dtk, recursive, `on_change(kind)` untuk root yang cocok; root yang tidak ada dilewati.

- [ ] **Step 1: Tulis tes gagal** (tempdir + `Db::open_in_memory`):
```rust
#[test] fn second_ingest_without_changes_reads_nothing()  // report ke-2 sources_read == 0
#[test] fn appended_lines_are_ingested_incrementally()    // tambah 1 baris assistant baru → total events naik 1
#[test] fn partial_trailing_line_is_read_next_time()      // tulis baris tanpa '\n' → 0 event baru; tambahkan '\n' → 1 event baru
#[test] fn deleted_source_keeps_events()                  // hapus file setelah ingest → events tetap sama
#[test] fn rewritten_smaller_file_is_reparsed_without_duplicates() // truncate & tulis ulang subset → jumlah distinct event_key tidak berlebih
#[test] fn codex_archived_copy_counted_once()             // salin rollout fixture ke archived_sessions/ → events tetap 2
#[test] fn cwds_get_project_rows()                        // setelah ingest, cwd_projects berisi "/tmp/demo"
```
- [ ] **Step 2:** `cargo test ingest::` → FAIL.
- [ ] **Step 3:** Implementasi `ingest.rs`, lalu `watcher.rs`.
- [ ] **Step 4:** `cargo test ingest::` → PASS. Tes watcher: `#[test] fn watcher_fires_on_append()` — append ke file di tempdir, tunggu channel ≤ 10 dtk → menerima kind. `cargo test watcher::` → PASS.
- [ ] **Step 5: Commit** `feat: incremental ingest with permanent history and file watcher`.

---

### Task 9: Harga OpenRouter + pencocokan model

**Files:**
- Create: `src-tauri/src/pricing/mod.rs`, `src-tauri/src/pricing/openrouter.rs`, `src-tauri/src/pricing/matcher.rs`

**Interfaces:**
- Produces:
```rust
// openrouter.rs
pub struct OrModel { pub id: String, pub prompt: f64, pub completion: f64, pub cache_read: Option<f64>, pub cache_write: Option<f64> }
pub fn parse_models_json(body: &str) -> anyhow::Result<Vec<OrModel>>;   // data[].pricing.{prompt,completion,input_cache_read,input_cache_write} (string USD/token)
pub fn fetch_models() -> anyhow::Result<Vec<OrModel>>;                  // GET https://openrouter.ai/api/v1/models, timeout 20 dtk, User-Agent "ai-token-tracker/0.1"
// matcher.rs
pub fn normalize(s: &str) -> String;
pub fn match_model(provider_hint: &str, raw_model: &str, catalog_ids: &[String]) -> Option<String>;
// mod.rs
pub struct PricingStatus { pub fetched_ms: Option<i64>, pub last_error: Option<String>, pub catalog_size: usize, pub unknown_models: Vec<(String, String)> }
pub fn refresh_prices(db: &Mutex<Db>, fetch: &dyn Fn() -> anyhow::Result<Vec<OrModel>>) -> PricingStatus; // simpan katalog, lalu rebuild_resolved
pub fn rebuild_resolved(db: &Db) -> anyhow::Result<()>;
pub fn set_override(db: &Db, o: &ModelOverride) -> anyhow::Result<()>;   // lalu rebuild_resolved
pub struct ModelOverride { pub provider_hint: String, pub raw_model: String, pub action: String /* map|ignore|manual|clear */,
  pub openrouter_id: Option<String>, pub prompt: Option<f64>, pub completion: Option<f64>, pub cache_read: Option<f64>, pub cache_write: Option<f64> }
pub fn status(db: &Db) -> anyhow::Result<PricingStatus>;
```

**Aturan:**
- `normalize`: lowercase; buang sufiks `[...]`; ganti `.` dan `_` dengan `-`; buang sufiks tanggal `-\d{8}`.
- `match_model`: kandidat = id katalog berprefix `{provider_hint}/`; cocok jika `normalize(id tanpa prefix) == normalize(raw_model)`. Tidak ada pencocokan fuzzy lain (varian seperti `-fast` sengaja tidak dipetakan ke model dasar).
- `rebuild_resolved`: untuk setiap `(provider_hint, model)` distinct di `events`: override `ignore` → `ignored`; `manual` → harga manual; `map` → harga katalog id tsb (`mapped`); selain itu `match_model` → `matched`; tidak ada → `unknown` (harga NULL).
- Gagal fetch → katalog lama tetap, `last_error` diisi (`app_meta` key `prices_last_error`), `fetched_ms` lama tetap.

- [ ] **Step 1: Tulis tes gagal:**
```rust
#[test] fn parses_openrouter_pricing_strings()   // body {"data":[{"id":"anthropic/claude-opus-5.5","pricing":{"prompt":"0.000005","completion":"0.000025","input_cache_read":"0.0000005","input_cache_write":"0.00000625"}}]} → prompt 5e-6, cache_write Some(6.25e-6)
#[test] fn missing_cache_prices_are_none()
#[test] fn normalize_handles_dots_dates_and_brackets() // "claude-opus-5-5[1m]"→"claude-opus-5-5"; "claude-sonnet-4-5-20250929"→"claude-sonnet-4-5"; "gpt-5.6-luna"→"gpt-5-6-luna"
#[test] fn matches_claude_dash_version_to_dotted_id() // ("anthropic","claude-opus-5-5",["anthropic/claude-opus-5.5"]) → Some
#[test] fn does_not_match_fast_variant_to_base()     // ("openai","gpt-5.6-luna-fast",["openai/gpt-5.6-luna"]) → None
#[test] fn respects_provider_prefix()                // ("openai","claude-opus-5-5",["anthropic/claude-opus-5.5"]) → None
#[test] fn default_ignores_synthetic_and_auto_review() // events dgn "<synthetic>" → status ignored
#[test] fn manual_override_wins_over_match()
#[test] fn refresh_failure_keeps_old_prices_and_reports_offline() // fetch Ok dulu, lalu fetch Err("offline") → prices tetap, last_error Some, fetched_ms sama; DB kosong + Err → semua unknown, tidak panic
```
- [ ] **Step 2:** `cargo test pricing::` → FAIL.
- [ ] **Step 3:** Implementasi.
- [ ] **Step 4:** `cargo test pricing::` → PASS.
- [ ] **Step 5: Commit** `feat(pricing): OpenRouter catalog, model matching and overrides`.

---

### Task 10: Periode + layer query

**Files:**
- Create: `src-tauri/src/period.rs`, `src-tauri/src/query.rs`

**Interfaces:**
- Consumes: tabel `events`, `sessions`, `cwd_projects`, `resolved_prices`, `project_footprints`.
- Produces (semua struct `Serialize` dengan `rename_all = "camelCase"`):
```rust
// period.rs
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PeriodSpec { Today, ThisWeek, LastWeek, ThisMonth, LastMonth, Custom { start_ms: i64, end_ms: i64 } }
pub struct PeriodRange { pub start_ms: i64, pub end_ms: i64 }   // [start, end)
pub fn resolve(spec: &PeriodSpec, now: DateTime<FixedOffset>) -> PeriodRange;
pub fn previous(spec: &PeriodSpec, now: DateTime<FixedOffset>) -> PeriodRange; // periode kalender sebelumnya; Custom: panjang sama tepat sebelumnya
pub fn local_offset_secs() -> i32;                               // dari chrono::Local::now().offset()

// query.rs
pub struct Totals { pub cost_usd: f64, pub unpriced_tokens: i64, pub tokens: i64, pub input: i64, pub output: i64,
  pub reasoning: i64, pub cache_read: i64, pub cache_write: i64, pub events: i64 }
pub struct AgentCost { pub agent: String, pub cost_usd: f64, pub tokens: i64 }
pub struct DailyPoint { pub date: String /* YYYY-MM-DD lokal */, pub by_agent: Vec<AgentCost> }
pub struct ProjectRow { pub project_key: String, pub name: String, pub cost_usd: f64, pub unpriced_tokens: i64, pub tokens: i64,
  pub sessions: i64, pub agents: Vec<String>, pub footprints: Vec<String> }
pub struct SessionRow { pub agent: String, pub session_id: String, pub title: Option<String>, pub project_key: Option<String>,
  pub started_ms: Option<i64>, pub updated_ms: Option<i64>, pub models: Vec<String>, pub tokens: i64, pub cost_usd: f64,
  pub unpriced_tokens: i64, pub token_status: String }
pub struct Overview { pub range: PeriodRange, pub totals: Totals, pub previous: Totals, pub change_pct: Option<f64>,
  pub daily: Vec<DailyPoint>, pub top_projects: Vec<ProjectRow>, pub top_sessions: Vec<SessionRow> }
pub enum Granularity { Week, Month }     // serde camelCase
pub enum Split { Agent, Model, Project } // serde camelCase
pub struct TrendBucket { pub label: String, pub start_ms: i64, pub totals: Totals, pub by: Vec<(String, f64)> }
pub struct SessionFilter { pub range: Option<PeriodRange>, pub agent: Option<String>, pub project_key: Option<String> }
pub struct EventRow { pub ts_ms: i64, pub model: String, pub input: i64, pub output: i64, pub reasoning: i64,
  pub cache_read: i64, pub cache_write: i64, pub cost_usd: Option<f64>, pub price_status: String, pub cwd: Option<String> }
pub struct SessionDetail { pub row: SessionRow, pub events: Vec<EventRow> }
pub struct ModelRow { pub provider_hint: String, pub model: String, pub status: String, pub openrouter_id: Option<String>,
  pub prompt: Option<f64>, pub completion: Option<f64>, pub totals: Totals }

pub fn totals(db: &Db, r: &PeriodRange) -> anyhow::Result<Totals>;
pub fn overview(db: &Db, spec: &PeriodSpec, now: DateTime<FixedOffset>) -> anyhow::Result<Overview>;
pub fn trends(db: &Db, g: Granularity, count: u32, split: Split, now: DateTime<FixedOffset>) -> anyhow::Result<Vec<TrendBucket>>;
pub fn projects(db: &Db, r: &PeriodRange) -> anyhow::Result<Vec<ProjectRow>>;
pub fn sessions(db: &Db, f: &SessionFilter) -> anyhow::Result<Vec<SessionRow>>;   // urut updated_ms desc
pub fn session_detail(db: &Db, agent: &str, session_id: &str) -> anyhow::Result<Option<SessionDetail>>;
pub fn models(db: &Db, r: &PeriodRange) -> anyhow::Result<Vec<ModelRow>>;
```

**Aturan:**
- Ekspresi biaya per event (left join `resolved_prices` on `(provider_hint, model)`):
```sql
CASE WHEN rp.status IN ('matched','mapped','manual') THEN e.input*rp.prompt + e.output*rp.completion
       + e.cache_read*COALESCE(rp.cache_read, rp.prompt) + e.cache_write*COALESCE(rp.cache_write, rp.prompt)
     WHEN rp.status = 'ignored' THEN 0 ELSE NULL END
```
  `cost_usd` = SUM non-null; `unpriced_tokens` = SUM token event yang biayanya NULL; `tokens = input+output+cache_read+cache_write`.
- Hari lokal: `date(e.ts_ms/1000 + :off, 'unixepoch')`; minggu: `date(e.ts_ms/1000 + :off, 'unixepoch', 'weekday 0', '-6 days')` (Senin); bulan: `strftime('%Y-%m', e.ts_ms/1000 + :off, 'unixepoch')`. `:off` = `now.offset().local_minus_utc()`.
- Project event = `cwd_projects.project_key` via `e.cwd`; jika `e.cwd` NULL pakai `sessions.cwd`. Project sesi (`SessionRow.project_key`) = project dengan token terbanyak di sesi itu; sesi tanpa event pakai `sessions.cwd`.
- `daily` mengisi semua hari dalam rentang (hari kosong = `by_agent` kosong). `top_projects`/`top_sessions` = 5 teratas berdasarkan `cost_usd`.
- `change_pct = (cur - prev) / prev * 100`, `None` jika `prev == 0`.
- `trends`: `count` bucket terakhir termasuk yang berjalan; `by` = biaya per kunci split, diurutkan desc.
- Sesi `NotRecorded` tetap muncul di `sessions()` dengan token 0.

- [ ] **Step 1: Tulis tes gagal** (DB in-memory, sisipkan event & resolved_prices langsung; `now` = `2026-11-21T10:00:00+07:00`):
```rust
#[test] fn this_week_starts_monday_local()  // resolve(ThisWeek) start == 2026-11-16T00:00:00+07:00
#[test] fn last_month_range()               // LastMonth == [2026-10-01T00:00+07, 2026-11-01T00:00+07)
#[test] fn cost_uses_all_four_components()  // input 1000 @1e-6 + output 100 @2e-6 + cache_read 10000 @1e-7 + cache_write 100 @1.25e-6 → 0.002325
#[test] fn cache_price_missing_falls_back_to_prompt()
#[test] fn unpriced_model_excluded_from_cost_and_counted() // 1 event priced $0.01 + 1 event unknown 500 token → cost 0.01, unpriced_tokens 500
#[test] fn ignored_model_costs_zero_not_unpriced()
#[test] fn event_after_utc_evening_lands_on_next_local_day() // ts 2026-11-20T18:30:00Z → daily date "2026-11-21"; tidak masuk total LastWeek
#[test] fn change_pct_against_previous_period()   // minggu lalu $10, minggu ini $15 → Some(50.0); minggu lalu 0 → None
#[test] fn project_groups_subfolders()            // cwd /r/a dan /r/b → cwd_projects key /r → 1 ProjectRow, sessions 2
#[test] fn session_project_is_majority_tokens()
#[test] fn weekly_trend_buckets_are_mondays()     // trends(Week, 3) → labels start 2026-11-02, 2026-11-09, 2026-11-16
#[test] fn session_detail_lists_events_in_time_order()
```
- [ ] **Step 2:** `cargo test period:: query::` → FAIL.
- [ ] **Step 3:** Implementasi.
- [ ] **Step 4:** `cargo test period:: query::` → PASS.
- [ ] **Step 5: Commit** `feat: period resolution and aggregation queries`.

---

### Task 11: Deteksi proses yang sedang berjalan

**Files:**
- Create: `src-tauri/src/process.rs`

**Interfaces:**
- Produces:
```rust
pub struct RunningAgent { pub agent: AgentKind, pub pid: u32, pub cwd: Option<String>, pub started_ms: i64, pub session_hint: Option<String> }
pub fn classify(name: &str, exe: Option<&str>, cmd: &[String]) -> Option<AgentKind>;
pub fn snapshot(sys: &mut sysinfo::System) -> Vec<RunningAgent>;   // refresh proses, classify, ambil cwd
pub fn attach_session_hints(db: &Db, running: &mut [RunningAgent]) -> anyhow::Result<()>; // sesi agent sama dgn cwd sama, updated_ms terbaru
```

**Aturan `classify`** (urut, pertama yang cocok):
- `name == "claude"` atau cmd[0] berakhiran `/claude` → `ClaudeCode`
- `name == "codex"` atau exe mengandung `/Codex.app/` → `Codex`
- `name == "opencode"` → `OpenCode`
- cmd memuat `@google/gemini-cli` atau cmd[1] berakhiran `/gemini` → `GeminiCli`
- cmd memuat `@github/copilot` atau `name == "copilot"` → `CopilotCli`
- exe mengandung `/Claude.app/Contents/MacOS/Claude` → `ClaudeDesktop`
- exe mengandung `/Visual Studio Code.app/Contents/MacOS/` → `VsCode`
- Proses helper (`Helper` di nama) diabaikan. Satu PID per agent+cwd (dedupe parent/child dengan cwd sama).

- [ ] **Step 1: Tulis tes gagal:**
```rust
#[test] fn classifies_claude_cli()      // ("claude", Some("/Users/x/.local/bin/claude"), ["claude"]) → ClaudeCode
#[test] fn classifies_gemini_node()     // ("node", Some("/opt/homebrew/bin/node"), ["node","/opt/homebrew/bin/gemini"]) → GeminiCli
#[test] fn classifies_codex_desktop()   // exe ".../Codex.app/Contents/MacOS/Codex" → Codex
#[test] fn ignores_helpers_and_unrelated() // ("Claude Helper (Renderer)", ...) → None; ("bash", ...) → None
#[test] fn session_hint_matches_latest_session_in_same_cwd()
```
- [ ] **Step 2:** `cargo test process::` → FAIL.
- [ ] **Step 3:** Implementasi (`sysinfo::Process::cwd()`, `start_time()` × 1000).
- [ ] **Step 4:** `cargo test process::` → PASS.
- [ ] **Step 5: Commit** `feat: detect running AI agent processes`.

---

### Task 12: Pemindaian direktori (A: data agent, B: jejak project)

**Files:**
- Create: `src-tauri/src/discovery/mod.rs`, `walk.rs`, `data_scan.rs`, `footprint.rs`

**Interfaces:**
- Consumes: `resolve_project` (Task 3), `adapter_for` (Task 4).
- Produces:
```rust
// walk.rs
pub const SKIP_DIRS: &[&str] = &["node_modules", ".git", "vendor", "target", "dist", "build", ".venv", ".Trash"];
pub const SKIP_PATHS: &[&str] = &["Library/Caches", "Library/Containers"]; // relatif ke home
pub fn walk(roots: &[PathBuf], home: &Path, cancel: &AtomicBool, visit: &mut dyn FnMut(&walkdir::DirEntry), denied: &mut Vec<PathBuf>);
// data_scan.rs
pub struct Sniff { pub guessed_agent: Option<String>, pub known_format: Option<AgentKind> }
pub fn sniff_file(path: &Path) -> Option<Sniff>;   // baca maks 8 KB
pub struct Candidate { pub path: String, pub guessed_agent: Option<String>, pub known_format: Option<AgentKind>, pub file_count: i64 }
pub fn scan_data(roots: &[PathBuf], home: &Path, known_roots: &[PathBuf], cancel: &AtomicBool) -> (Vec<Candidate>, Vec<PathBuf>);
// footprint.rs
pub fn marker_agent(file_name: &str, is_dir: bool) -> Option<&'static str>;
pub struct Footprint { pub project_key: String, pub agent: String, pub marker: String }
pub fn scan_footprints(roots: &[PathBuf], home: &Path, cancel: &AtomicBool) -> (Vec<Footprint>, Vec<PathBuf>);
// mod.rs
pub fn run_discovery(db: &Mutex<Db>, roots: &[PathBuf], home: &Path, known_roots: &[PathBuf], cancel: &AtomicBool) -> anyhow::Result<()>; // simpan ke discovery_candidates (pertahankan decision lama), project_footprints, discovery_denied
pub fn tracked_adapters(db: &Db) -> anyhow::Result<Vec<Box<dyn Adapter>>>; // kandidat decision='track' & known_format → adapter_for(kind, path)
```

**Aturan:**
- `walk`: tidak mengikuti symlink; skip `SKIP_DIRS` di mana pun dan `SKIP_PATHS` relatif home; error izin (EPERM/EACCES) → masukkan ke `denied`, lanjut.
- `sniff_file` hanya untuk ekstensi `jsonl`, `json`, `db`, `sqlite`. JSON(L): cocok jika 8 KB pertama memuat salah satu kunci `"input_tokens"`, `"output_tokens"`, `"prompt_tokens"`, `"completion_tokens"`, `"cache_read_input_tokens"`, `"usage"` **dan** `"model"`. `known_format`: memuat `"sessionId"` + `"message"` + `"usage"` → `ClaudeCode`; `"type":"session_meta"` atau `"token_count"` → `Codex`. SQLite: header `SQLite format 3\0`, buka read-only, ada tabel `session_message` + `session_v2` → `OpenCode`; atau ada kolom bernama mengandung `token` → kandidat tanpa format.
- `scan_data` mengelompokkan per folder induk file; root adapter yang sudah dikenal (`known_roots`) dan isinya dikecualikan. Kandidat yang `known_format` = `ClaudeCode` dikelompokkan ke folder yang berisi `projects/` (root config), `Codex` ke folder berisi `sessions/`.
- `marker_agent`: `.claude`(dir)/`CLAUDE.md` → `"claude-code"`; `AGENTS.md`/`.codex`(dir) → `"codex"`; `GEMINI.md`/`.gemini`(dir) → `"gemini-cli"`; `.cursor`(dir)/`.cursorrules` → `"cursor"`; `copilot-instructions.md` (di dalam `.github`) → `"copilot"`; `.opencode`(dir)/`opencode.json` → `"opencode"`; nama berawalan `.aider` → `"aider"`; `.windsurf`(dir)/`.windsurfrules` → `"windsurf"`; `.continue`(dir) → `"continue"`. Marker `.claude`/`.codex`/`.gemini` tepat di home diabaikan (itu folder data, bukan project).
- Footprint project = `resolve_project(folder yang memuat marker)`.

- [ ] **Step 1: Tulis tes gagal** (tempdir sebagai home):
```rust
#[test] fn walk_skips_node_modules_and_caches()
#[test] fn walk_does_not_follow_symlinks()
#[test] fn sniff_recognizes_claude_and_codex_shapes()  // pakai baris fixture Task 4/5
#[test] fn sniff_rejects_json_without_model_key()      // {"usage":1} → None
#[test] fn scan_data_finds_relocated_claude_dir()      // home/alt-claude/projects/p/s.jsonl → Candidate path home/alt-claude, known_format ClaudeCode
#[test] fn scan_data_excludes_known_roots()            // home/.claude/projects/... dgn known_roots=[home/.claude] → 0 kandidat
#[test] fn footprints_map_markers_to_git_project()     // repo/.git + repo/sub/CLAUDE.md + repo/AGENTS.md → {(repo,"claude-code"),(repo,"codex")}
#[test] fn home_level_data_dirs_are_not_footprints()
#[test] fn rerun_preserves_user_decision()             // decision 'ignore' tetap setelah run_discovery kedua
#[test] fn tracked_candidate_becomes_adapter()
```
- [ ] **Step 2:** `cargo test discovery::` → FAIL.
- [ ] **Step 3:** Implementasi.
- [ ] **Step 4:** `cargo test discovery::` → PASS.
- [ ] **Step 5: Commit** `feat: directory discovery for agent data and project footprints`.

---

### Task 13: Settings, export CSV, commands, dan wiring aplikasi

**Files:**
- Create: `src-tauri/src/settings.rs`, `src-tauri/src/export.rs`, `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/capabilities/default.json`

**Interfaces:**
- Consumes: semua modul sebelumnya.
- Produces:
```rust
// settings.rs  (struct: serde camelCase; Currency: serde rename_all = "lowercase" → "usd" | "idr")
pub enum Currency { Usd, Idr }
pub struct Settings { pub currency: Currency, pub idr_rate: Option<f64>, pub launch_at_login: bool, pub process_poll_secs: u64, pub scan_roots: Vec<String> }
impl Settings { pub fn defaults(home: &Path) -> Settings; pub fn load(db: &Db, home: &Path) -> Settings; pub fn save(&self, db: &Db) -> anyhow::Result<()>; }
// export.rs
pub fn sessions_csv(rows: &[SessionRow]) -> String;  // header: agent,session_id,title,project,started,updated,models,tokens,cost_usd,unpriced_tokens
pub fn daily_csv(points: &[DailyPoint]) -> String;   // header: date,agent,cost_usd,tokens
// lib.rs
pub struct AppState { pub db: Arc<Mutex<Db>>, pub home: PathBuf, pub adapters: RwLock<Vec<Box<dyn Adapter>>>,
  pub running: RwLock<Vec<RunningAgent>>, pub progress: RwLock<Option<Progress>>, pub discovery_cancel: Arc<AtomicBool> }
pub struct AppStatus { pub first_run: bool, pub gap_days: Option<f64>, pub retention_warning: Option<String>,
  pub progress: Option<Progress>, pub pricing: PricingStatus, pub discovery_running: bool, pub data_since_ms: Option<i64> }
pub struct AgentRow { pub agent: String, pub label: String, pub detected: bool, pub data_root: String,
  pub level: String /* "full"|"sessions"|"presence" */, pub sessions: i64, pub first_ms: Option<i64>, pub last_ms: Option<i64>,
  pub running: Vec<RunningAgent> }
```
Commands (nama persis, argumen camelCase di sisi JS):
`get_overview(period: PeriodSpec) -> Overview`, `get_trends(granularity, count: u32, split) -> Vec<TrendBucket>`, `list_projects(period) -> Vec<ProjectRow>`, `list_sessions(filter: SessionFilterArg { period: Option<PeriodSpec>, agent, projectKey }) -> Vec<SessionRow>`, `get_session(agent, sessionId) -> Option<SessionDetail>`, `list_models(period) -> Vec<ModelRow>`, `list_agents() -> Vec<AgentRow>`, `get_app_status() -> AppStatus`, `refresh_prices() -> PricingStatus`, `set_model_override(o: ModelOverride) -> ()`, `list_price_catalog() -> Vec<String>`, `get_settings() -> Settings`, `update_settings(s: Settings) -> ()`, `start_discovery() -> ()`, `cancel_discovery() -> ()`, `get_discovery() -> DiscoveryView { candidates, footprintsCount, denied: Vec<String> }`, `set_candidate_decision(path, decision: "track"|"ignore") -> ()`, `export_csv(kind: "sessions"|"daily", period, path) -> ()`, `wipe_all_data() -> ()`.

**Wiring `run()`:**
- Buka DB; jika belum ada settings → simpan default, aktifkan autostart (`tauri-plugin-autostart`, `MacosLauncher::LaunchAgent`), set `first_run=true`. `update_settings` menyelaraskan autostart dengan `launch_at_login`.
- Heartbeat: saat start hitung `gap_days` dari `app_meta.last_seen_ms`; jika > 20 → `retention_warning = "Aplikasi tidak berjalan {n} hari. Sesi Claude Code yang lebih tua dari 30 hari mungkin sudah terhapus sebelum sempat tercatat."`. Perbarui `last_seen_ms` tiap 5 menit.
- Thread background: (1) `ingest_all(default_adapters + tracked_adapters)` dengan progress → emit `"data-updated"`; (2) watcher → `ingest_adapter` → emit; (3) poll proses tiap `process_poll_secs`; (4) refresh harga saat start + tiap 24 jam → emit; (5) jika `first_run`, jalankan discovery sekali setelah ingest awal.
- `set_candidate_decision("track")` → tambah adapter ke `adapters`, ingest, emit.
- Jendela `main`: `CloseRequested` → `prevent_close()` + `hide()`.
- Capabilities: `core:default`, `dialog:allow-save`, `autostart:default`.

- [ ] **Step 1: Tulis tes gagal:**
```rust
#[test] fn settings_default_values()           // USD, idr_rate None, launch_at_login true, poll 5, scan_roots [home]
#[test] fn settings_roundtrip()
#[test] fn sessions_csv_escapes_commas_and_quotes() // title `He said "hi", ok` → "\"He said \"\"hi\"\", ok\""
#[test] fn daily_csv_one_row_per_agent_per_day()
#[test] fn retention_warning_after_20_days_gap()   // fungsi pure `retention_warning(gap_days: f64) -> Option<String>`
```
- [ ] **Step 2:** `cargo test settings:: export:: commands::` → FAIL.
- [ ] **Step 3:** Implementasi settings, export, commands, wiring.
- [ ] **Step 4:** `cargo test` (semua) → PASS; `cargo build` → sukses.
- [ ] **Step 5: Verifikasi manual** — `npm run tauri dev`; di devtools console: `await window.__TAURI__.core.invoke('list_agents')` (aktifkan `withGlobalTauri` hanya di dev) → memuat Claude Code dengan `sessions > 0`.
- [ ] **Step 6: Commit** `feat: Tauri commands, settings, CSV export and background services`.

---

### Task 14: Ikon menu bar

**Files:**
- Create: `src-tauri/src/tray.rs`, `src-tauri/icons/tray-template.png` (ikon monokrom 22×22 @2x, `icon_as_template(true)`)
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces:
```rust
pub struct TraySummary { pub today: f64, pub week: f64, pub week_change: Option<f64>, pub month: f64, pub month_change: Option<f64>, pub running: Vec<(String, String)> }
pub fn format_money(usd: f64, s: &Settings) -> String;   // "$4.12" | "Rp 65.920" (kurs × usd, pemisah ribuan titik, tanpa desimal); IDR tanpa kurs → fallback USD
pub fn format_title(sum: &TraySummary, s: &Settings) -> String;  // = format_money(today)
pub fn menu_lines(sum: &TraySummary, s: &Settings) -> Vec<String>; // ["Hari ini  $4.12", "Minggu ini  $31.80  ▲12%", "Bulan ini  $142.30  ▲18%", "Sedang jalan:", "• Claude Code — backoffice-be"] ; "▼" untuk turun; tanpa persen jika None; "Tidak ada agent berjalan" jika kosong
pub fn setup_tray(app: &AppHandle) -> tauri::Result<()>;
pub fn refresh_tray(app: &AppHandle) -> tauri::Result<()>;
```
- Menu: baris `menu_lines` (disabled), separator, `Buka dashboard` (show + focus jendela `main`), `Refresh harga`, separator, `Keluar` (`app.exit(0)`). Judul tray = `format_title`. Refresh tiap 30 dtk dan setelah setiap emit `data-updated`.

- [ ] **Step 1: Tulis tes gagal:**
```rust
#[test] fn money_usd_two_decimals()      // 4.1234 → "$4.12"
#[test] fn money_idr_uses_rate()         // 4.12 @16000 → "Rp 65.920"
#[test] fn idr_without_rate_falls_back_to_usd()
#[test] fn menu_lines_show_change_arrows() // week_change Some(12.0) → "▲12%"; Some(-5.0) → "▼5%"
#[test] fn menu_lines_when_nothing_running()
```
- [ ] **Step 2:** `cargo test tray::` → FAIL.
- [ ] **Step 3:** Implementasi.
- [ ] **Step 4:** `cargo test tray::` → PASS; `npm run tauri dev` → ikon & judul biaya muncul di menu bar, menutup jendela tidak mengakhiri aplikasi, `Keluar` mengakhiri.
- [ ] **Step 5: Commit** `feat: menu bar summary with today's cost`.

---

### Task 15: Frontend — shell, format, Dashboard, Mingguan/Bulanan

**Files:**
- Create: `src/types.ts`, `src/api.ts`, `src/lib/format.ts`, `src/lib/format.test.ts`, `src/components/PeriodPicker.tsx`, `StatCard.tsx`, `CostCell.tsx`, `DailyStackedBar.tsx`, `DataTable.tsx`, `src/pages/Dashboard.tsx`, `src/pages/Trends.tsx`
- Modify: `src/App.tsx`, `src/main.tsx`, `package.json` (script `"test": "vitest run"`)

**Interfaces:**
- Consumes: commands Task 13 (nama & bentuk persis).
- Produces:
```ts
// lib/format.ts
export function formatMoney(usd: number, s: Settings): string;           // sama dengan Rust format_money
export function formatTokens(n: number): string;                         // 950 → "950"; 12_300 → "12.3K"; 4_560_000 → "4.56M"; 1.2e9 → "1.2B"
export function formatPct(p: number | null): string;                     // 12.04 → "▲12%"; -5 → "▼5%"; null → "—"
export function costLabel(usd: number, unpriced: number, s: Settings): string; // unpriced>0 → `${formatMoney(usd)} + ${formatTokens(unpriced)} token tanpa harga`
// api.ts: satu fungsi per command, mis. export const getOverview = (period: PeriodSpec) => invoke<Overview>('get_overview', { period });
// App.tsx: type Page = 'dashboard'|'trends'|'projects'|'sessions'|'models'|'agents'|'pricing'|'settings'; state `refreshKey` naik tiap event "data-updated"
```
- Sidebar label: Dashboard, Mingguan & Bulanan, Project, Sesi, Model, Agent, Harga, Pengaturan.
- Dashboard: PeriodPicker (Hari ini / Minggu ini / Minggu lalu / Bulan ini / Bulan lalu / Custom), 3 StatCard (Biaya + `formatPct(change)`, Token, Token tanpa harga), DailyStackedBar (recharts `BarChart` stacked per agent), tabel Top 5 project & Top 5 sesi (klik → halaman terkait), daftar agent sedang berjalan, banner `retentionWarning` dan progress scan dari `get_app_status`.
- Trends: toggle Minggu/Bulan (12 bucket), split Agent/Model/Project, stacked bar + tabel ringkasan per bucket dengan kolom perubahan vs bucket sebelumnya. Di bawah grafik tampil keterangan "Data tersedia sejak {tanggal event tertua}" (dari `MIN(ts_ms)` di `events`, ditambahkan sebagai `data_since_ms: Option<i64>` di `AppStatus`), supaya bucket kosong di awal tidak terbaca sebagai pemakaian nol.

- [ ] **Step 1: Tulis tes gagal** `format.test.ts`:
```ts
test('formatTokens scales', () => { expect(formatTokens(950)).toBe('950'); expect(formatTokens(12300)).toBe('12.3K'); expect(formatTokens(4560000)).toBe('4.56M'); });
test('formatMoney idr', () => { expect(formatMoney(4.12, {...base, currency:'idr', idrRate:16000})).toBe('Rp 65.920'); });
test('formatMoney idr without rate falls back', () => { expect(formatMoney(4.12, {...base, currency:'idr', idrRate:null})).toBe('$4.12'); });
test('formatPct arrows', () => { expect(formatPct(12.04)).toBe('▲12%'); expect(formatPct(-5)).toBe('▼5%'); expect(formatPct(null)).toBe('—'); });
test('costLabel mentions unpriced', () => { expect(costLabel(1, 500, base)).toBe('$1.00 + 500 token tanpa harga'); });
```
- [ ] **Step 2:** `npm test` → FAIL.
- [ ] **Step 3:** Implementasi format, api, types, komponen, App shell, Dashboard, Trends.
- [ ] **Step 4:** `npm test` → PASS; `npm run build` → sukses; `npm run tauri dev` → Dashboard menampilkan biaya bulan ini dari data asli, grafik harian terisi.
- [ ] **Step 5: Commit** `feat(ui): app shell, dashboard and weekly/monthly trends`.

---

### Task 16: Frontend — Project, Sesi, Detail Sesi, Model

**Files:**
- Create: `src/pages/Projects.tsx`, `Sessions.tsx`, `SessionDetail.tsx`, `Models.tsx`
- Modify: `src/App.tsx` (navigasi ke detail dengan state `{agent, sessionId}` dan filter project)

**Interfaces:**
- Consumes: `listProjects`, `listSessions`, `getSession`, `listModels` dari `api.ts`.

- Project: tabel (Nama, Biaya, Token, Sesi, Agent dipakai, badge footprint), sort per kolom, klik → Sesi terfilter project itu.
- Sesi: filter periode/agent/project; kolom Agent, Judul, Project, Mulai, Durasi (`updated-started`), Model, Token, Biaya (`CostCell`); sesi `not-recorded` diberi label "token tidak tercatat lokal".
- Detail sesi: header (judul, agent, project, folder kerja yang dipakai), grafik garis biaya kumulatif per respons, tabel per respons (waktu, model, input, output, reasoning, cache read, cache write, biaya / "—" + status harga).
- Model: tabel (Model, Provider, Status harga, ID OpenRouter, harga input/output per 1M token, Token, Biaya), status `unknown` ditandai dan ditautkan ke halaman Harga.

- [ ] **Step 1:** Implementasi keempat halaman.
- [ ] **Step 2: Verifikasi** — `npm run build` sukses; `npm run tauri dev`: buka sesi Claude Code terbaru → jumlah baris respons = jumlah `message.id` distinct di file JSONL-nya. Cek independen:
  `jq -r 'select(.type=="assistant" and .message.usage) | .message.id' <file sesi> | sort -u | wc -l` → sama dengan jumlah baris di Detail sesi.
- [ ] **Step 3: Commit** `feat(ui): projects, sessions, session detail and models pages`.

---

### Task 17: Frontend — Agent (termasuk discovery), Harga, Pengaturan

**Files:**
- Create: `src/pages/Agents.tsx`, `Pricing.tsx`, `Settings.tsx`

**Interfaces:**
- Consumes: `listAgents`, `getAppStatus`, `startDiscovery`, `cancelDiscovery`, `getDiscovery`, `setCandidateDecision`, `getPricingStatus` (via `getAppStatus().pricing`), `refreshPrices`, `listPriceCatalog`, `setModelOverride`, `getSettings`, `updateSettings`, `exportCsv`, `wipeAllData`; `@tauri-apps/plugin-dialog` `save()`.

- Agent: kartu per agent (status Sedang jalan / Pernah jalan / Tidak terdeteksi; level "Token penuh" / "Sesi saja" / "Deteksi saja"; lokasi data; jumlah sesi; data sejak; proses aktif + folder kerja + sesi). Bagian **Scan direktori**: tombol "Scan ulang"/"Batalkan", progress, tabel kandidat (lokasi, perkiraan agent, format, jumlah file, tombol Lacak/Abaikan), daftar folder yang ditolak izinnya dengan keterangan "macOS menolak akses — beri izin di System Settings › Privacy & Security › Files and Folders", jumlah footprint project. Banner `retentionWarning`.
- Harga: sumber + waktu update terakhir + error terakhir, tombol "Perbarui harga", tabel model `unknown` dengan aksi: petakan ke ID OpenRouter (input dengan autocomplete dari `listPriceCatalog`), isi harga manual (per 1M token di UI, dikonversi ke per token), atau Abaikan; tabel override aktif dengan tombol hapus (`action: "clear"`).
- Pengaturan: mata uang (USD/IDR) + kurs IDR, jalankan saat login, interval poll proses, daftar folder scan (tambah/hapus), ekspor CSV (sesi / harian, periode dipilih, dialog simpan), "Hapus semua data" dengan konfirmasi dua langkah.

- [ ] **Step 1:** Implementasi ketiga halaman.
- [ ] **Step 2: Verifikasi** — `npm run build` sukses; `npm run tauri dev`: halaman Agent menampilkan Claude Code, Codex, opencode (Token penuh), Gemini CLI & Copilot CLI (Sesi saja, kecuali Gemini punya token), Claude desktop & VS Code (Deteksi saja); "Scan ulang" selesai dan menampilkan footprint > 0; ubah mata uang ke IDR dengan kurs → Dashboard & menu bar berganti ke Rupiah; ekspor CSV sesi menghasilkan file dengan header Task 13.
- [ ] **Step 3: Commit** `feat(ui): agents with discovery, pricing overrides and settings`.

---

### Task 18: Build `.dmg` dan uji asap dengan data asli

**Files:**
- Modify: `src-tauri/tauri.conf.json` (jika perlu: `bundle.macOS.minimumSystemVersion: "13.0"`), `README.md` (cara install + catatan Gatekeeper + lokasi database)

- [ ] **Step 1:** `cd /Users/ultravoucher/ai-token-tracker && npm run tauri build -- --target aarch64-apple-darwin --bundles dmg` → selesai; file ada di `src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/AI Token Tracker_0.1.0_aarch64.dmg`.
- [ ] **Step 2:** Mount dmg, salin app ke `/Applications`, buka (klik kanan → Open). Expected: jendela terbuka, ikon menu bar muncul dengan biaya hari ini, scan awal berjalan dengan progress.
- [ ] **Step 3: Cek silang token** — untuk satu sesi Claude Code nyata, hitung independen:
  `jq -s '[.[] | select(.type=="assistant" and .message.usage)] | unique_by(.message.id) | map(.message.usage.output_tokens) | add' <file sesi>` → sama dengan total output di Detail sesi. Untuk satu sesi Codex: jumlah `last_token_usage.output_tokens` pada `total_tokens` distinct = total output di aplikasi.
- [ ] **Step 4: Cek read-only** — `find ~/.claude ~/.codex ~/.gemini ~/.copilot ~/.local/share/opencode -newer <timestamp sebelum app dibuka> -type f` tidak memuat file yang ditulis oleh aplikasi (hanya file yang ditulis agent itu sendiri).
- [ ] **Step 5: Cek "jalankan saat login"** — `ls ~/Library/LaunchAgents | grep -i "AI Token Tracker"` → ada.
- [ ] **Step 6: Cek performa** — setelah scan awal selesai dan aplikasi idle 2 menit: `ps -o %cpu,rss -p $(pgrep -f "AI Token Tracker")` → CPU ~0%, RSS < 150 MB.
- [ ] **Step 7: Commit** `chore: release build 0.1.0 and README`.
