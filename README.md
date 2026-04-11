# Kirin (Jirafeau-style file sharing)

Rust service modeled after [Jirafeau](https://gitlab.com/jirafeau/Jirafeau). The PHP reference clone lives in `Jirafeau/` in this repository.

## Run

Copy `config.example.toml` to `config.toml`, then:

```bash
cargo run --release -- config.toml
```

Set `RUST_LOG=kirin=debug,tower_http=debug` for request logging.

## Configuration

All settings live in one TOML file. The authoritative annotated example is `config.example.toml`. Notable sections:

- `server`: listen address, `public_base_url` (used in generated links; trailing slash optional), `data_dir`, `max_body_mb`, optional `max_upload_chunk_bytes` for async chunk uploads.
- `limits`: `max_upload_bytes` (0 = unlimited) and `link_id_length` (clamped between 4 and 32).
- `upload_auth`: optional upload passwords and IP allow lists (mirrors Jirafeau-style rules).
- `availabilities`: which expiry choices the UI may offer; `default` is the preselected duration on the index form.
- `features`: one-time download, preview, and download-password policy (`optional`, `required`, or `generated` placeholder).
- `admin`: SHA-256 hex of the admin password (empty disables admin), 64-hex session signing key, optional admin IP allow list.
- `ui`: page title and optional organisation string.

See `docs/ARCHITECTURE.md` for routes, storage layout, and security notes.

## Docker

Build an image from the repository root (this crate):

```bash
docker build -t kirin:latest -f Dockerfile .
```

Run with a mounted config and persistent data directory:

```bash
docker run --rm -p 8080:8080 \
  -v /path/to/config.toml:/etc/kirin/config.toml:ro \
  -v /path/to/data:/srv/data \
  kirin:latest
```

Point `server.data_dir` in the mounted config at the path visible inside the container (for example `/srv/data`) and set `server.bind` to `0.0.0.0:8080` if you publish port 8080.
