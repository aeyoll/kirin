# Kirin architecture (Jirafeau-style file sharing)

This service reimplements the core behaviour of [Jirafeau](https://gitlab.com/jirafeau/Jirafeau): time-limited uploads, optional download password (argon2-hashed in metadata), one-time download, chunked uploads, and an admin area.

The Rust crate lives at the repository root (`Cargo.toml`, `src/`). The PHP reference clone is in `Jirafeau/`.

## Reference mapping (PHP clone at `Jirafeau/`)

| PHP surface | Rust surface |
|-------------|--------------|
| `index.php` | `GET /` upload UI |
| `f.php?h=` | `GET /f/{id}` download gate / preview; `GET /f/{id}?d=1` stream; `GET /f/{id}?d={code}` delete UI |
| `script.php` multipart upload | `POST /script` (multipart); browser UI uses `POST /api/upload/async/*` then `POST /upload/complete/session` for the signed result page |
| `script.php?init_async` / `push_async` / `end_async` | `POST /api/upload/async/init`, `/push`, `/end` |
| `admin.php` | `GET/POST /admin` (session cookie after password) |
| `lib/config.local.php` | `config.toml` |
| `var/files/` + `var/links/` line-based link file | `data/files/{aa}/{id}` blob + `data/files/{aa}/{id}.meta.toml` sidecar |

## Crate layout

Single binary crate `kirin` at repo root. Modules:

- `config` — TOML configuration, defaults aligned with Jirafeau options where practical.
- `models` — `FileMeta`, async session structs, serde types for sidecars.
- `expiry` — availability strings to Unix expiry (pure logic, unit-tested).
- `password` — Argon2 for download passwords; admin uses SHA-256 hex of password (Jirafeau-compatible).
- `storage` — `StorageBackend` trait; `local_fs` implements sharded paths under `data_dir`.
- `error` — `AppError` (`thiserror`) and `IntoResponse`.
- `templates` — MiniJinja templates embedded via `include_str!` from `templates/`.
- `routes` — Axum handlers merged in `app.rs`.

## Shared state

`AppState` holds `Arc<AppConfig>`, `Arc<dyn StorageBackend>`, template environment, and a `tokio::sync::Mutex`-protected async upload table (in-memory codes plus temp files on disk). Handlers receive `State<AppState>`.

## Storage backend trait

`StorageBackend` abstracts object placement. The default `LocalFsStorage` writes:

- Blob: `data_dir/files/{first_two(id)}/{id}` (streaming writes, BLAKE3 computed while writing).
- Metadata: `data_dir/files/{shard}/{id}.meta.toml` (filename, mime, size, expiry, flags, delete code, optional argon2 password hash).

## Errors

Library-style errors use `thiserror`. `main.rs` uses `anyhow` for startup. HTTP mapping centralises status codes and JSON bodies for API-style errors.

## Security notes

- Download passwords are never stored in plain text (argon2 hash in TOML).
- Admin password is compared to configured SHA-256 hex (same representation as Jirafeau `admin_password`).
- Admin session uses HMAC-SHA256 signed cookie (`subtle` constant-time verify).
- Link IDs are generated from `rand` alphanumeric, length configurable.

## Async uploads

Init creates a random reference, random 4-char rolling code, and a temp file under `data_dir/async/`. Push appends chunk bytes after validating the code and total size. End finalises: moves temp to blob path, writes `.meta.toml`, deletes async descriptor.

## Observability

`tracing` + `tracing-subscriber` with `EnvFilter`, request spans via `TraceLayer`.
