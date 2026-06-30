# Block Duplicator

A fast, block-level copier for raw data. It moves bytes between **files**, **raw block devices**, **VHDX images**, and **TCP streams** through a single, uniform source/sink interface, with live progress reporting.

> ⚠️ **Destructive operations:**
> Block Duplicator writes raw bytes directly to whatever destination you point it at. Targeting the wrong block device or VHDX **can overwrite data and render a disk unbootable**. Always double-check `--src` and `--dst` before running.

## Features

- Uniform `BlockSource` / `BlockSink` abstraction over multiple backends.
- Backends:
  - `file` – regular files (read/write, any OS).
  - `block` – raw physical/logical drives via `\\.\` paths (Windows, requires elevation).
  - `vhdx` – creates and writes an expandable VHDX image via `diskpart` (Windows destination only, requires elevation).
  - `network` – sequential copy over `tcp://host:port` (source or sink).
- Automatic backend inference from the path.
- Live progress bar (percentage and bytes copied).
- Automatic UAC elevation on Windows: when raw device/VHDX access is needed, the tool relaunches an elevated worker process and streams the operation to it over a local, token-authenticated TCP channel.

## Platform support

| Backend | Windows | Other OS |
| --- | --- | --- |
| `file` | ✅ | ✅ |
| `network` | ✅ | ✅ |
| `block` | ✅ (elevated) | ❌ |
| `vhdx` | ✅ (elevated) | ❌ |

Raw device, VHDX, and the elevation worker are Windows-only.

## Requirements

- A Rust toolchain with **edition 2024** support (MSRV: Rust 1.85+).

## Build

```bash
cargo build --release
```

The binary is produced at `target/release/bd` (`bd.exe` on Windows).

## Usage

```text
bd --src <path> --dst <path>
   [--src-kind file|block|network]
   [--dst-kind file|block|vhdx|network]
   [--chunk-size <bytes>]
   [--vhdx-size-bytes <bytes>]
   [--help|-h]
```

If `--src-kind` / `--dst-kind` are omitted, the kind is inferred from the path:

| Path pattern | Inferred kind |
| --- | --- |
| starts with `\\.\` | `block` |
| starts with `tcp://` | `network` |
| ends with `.vhdx` | `vhdx` |
| anything else | `file` |

### Options

| Option | Description | Default |
| --- | --- | --- |
| `--src` | Source path / endpoint (required) | — |
| `--dst` | Destination path / endpoint (required) | — |
| `--src-kind` | Force the source backend | inferred |
| `--dst-kind` | Force the destination backend | inferred |
| `--chunk-size` | Transfer buffer size in bytes (clamped up to backend block size) | `1048576` (1 MiB) |
| `--vhdx-size-bytes` | Size of the VHDX to create; falls back to source length when measurable | source length |
| `--help`, `-h` | Print usage and exit | — |

### Examples

Copy a file to a file:

```bash
bd --src input.img --dst output.img
```

Clone a raw drive to a file (Windows, will prompt for elevation):

```bash
bd --src \\.\PhysicalDrive1 --dst backup.img
```

Write an image into a new VHDX (Windows, will prompt for elevation):

```bash
bd --src disk.img --dst out.vhdx
```

Send over the network:

```bash
bd --src input.img --dst tcp://127.0.0.1:9000
```

Receive over the network:

```bash
bd --src tcp://127.0.0.1:9000 --dst output.img
```

## How it works

- `src/core/io.rs` defines the `BlockSource` / `BlockSink` traits and backend capabilities (random access, elevation requirement).
- `src/engine/transfer.rs` performs the offset-based copy loop with progress callbacks.
- `src/backends/` contains one module per backend.
- `src/elevated_worker.rs` implements the Windows elevation flow: a non-elevated parent spawns an elevated copy of itself (`--bd-worker`), authenticates it with a one-time token, and proxies all reads/writes over a localhost TCP IPC protocol.

### Network protocol notes

The `network` backends are **sequential-only**. A network source expects the peer to send an 8-byte little-endian length prefix followed by the raw bytes; a network sink streams raw bytes after connecting. Non-sequential `read_at`/`write_at` are rejected.

## Tests

```bash
cargo test
```

Integration tests under `tests/` invoke the built binary. The `file_to_file` test runs anywhere; tests touching block devices / VHDX require Windows and elevation. Set `BLOCK_DUP_KEEP_TMP=1` to keep generated test artifacts.

> **Note (CI):** No CI workflow is configured yet. A future workflow could run `cargo fmt --check`, `cargo clippy`, `cargo build`, and the `file_to_file` test; block-device/VHDX tests can't run on standard CI runners since they need Windows + elevation.

## License

Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0). See [LICENSE](LICENSE).

## Author

By DocterDum.
