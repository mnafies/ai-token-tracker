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
