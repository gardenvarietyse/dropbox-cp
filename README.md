# dropbox-cp

[![Tests](https://github.com/gardenvarietyse/dropbox-cp/actions/workflows/test.yml/badge.svg)](https://github.com/gardenvarietyse/dropbox-cp/actions/workflows/test.yml)

### `cp` for Dropbox

**dropbox-cp** provides the **`dcp`** command-line tool: it copies a local file or directory tree into a path in a Dropbox account using the Dropbox API. Missing remote parent folders are created as needed. By default, if a destination file already exists in Dropbox, `dcp` prints an error, skips that file, and continues with the rest; use `-f` to overwrite existing files.

## Build and test

From the repository root:

```sh
cargo build --bin dcp
```

```sh
cargo test
```

Release binary:

```sh
cargo build --release --bin dcp
```

The executable is written to `target/debug/dcp` (or `target/release/dcp` for `--release`).

### Optional: mise tasks

If you use [mise](https://mise.jdx.dev), the same workflows are available as tasks (see [`mise.toml`](mise.toml)):

```sh
mise run build
mise run test
mise run build-release
```

`mise tasks` lists them with descriptions. These are thin wrappers around the `cargo` commands above.

## How to run

Set credentials in the environment (see below), then:

```sh
cargo run --bin dcp -- [OPTIONS] <SOURCE> <DEST>
```

Or run the built binary directly, e.g. `./target/release/dcp …`.

### Command-line flags and arguments

| Item                | Description                                                                                                                                     |
| ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `<SOURCE>`          | Local path to a file or directory to upload.                                                                                                    |
| `<DEST>`            | Destination path in Dropbox (e.g. `/backup/stuff` or `backup/stuff`). A leading `/` is optional; relative paths are rooted at the account root. |
| `-r`, `--recursive` | Required when `<SOURCE>` is a directory. Recursively uploads files; relative paths under the source are preserved under `<DEST>`.               |
| `-f`, `--force`     | Overwrite files that already exist at the corresponding Dropbox path. Without this, existing files are skipped after an error message.          |
| `-h`, `--help`      | Print usage, examples, and environment variable documentation. Does not require credentials.                                                    |

Examples:

```sh
dcp ./notes.txt /backup/notes.txt
dcp -r ./stuff /backup/stuff
dcp -r -f ./stuff /backup/stuff
```

### Credentials (environment)

`dcp` needs to authenticate with Dropbox. Use **either**:

- **Option A:** `DROPBOX_ACCESS_TOKEN` — requires frequent manual token refresh.
- **Option B:** `DROPBOX_APP_KEY`, `DROPBOX_APP_SECRET`, and `DROPBOX_REFRESH_TOKEN` — recommended.

Details and links: `dcp --help`.

### Other

Implemented, to my great shame, by Cursor.
