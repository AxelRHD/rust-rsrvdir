<p align="center">
  <img src="logo.svg" alt="rsrvdir logo" width="480">
</p>

<p align="center"><strong>Simple directory server with file info</strong><br>Serve one directory – or several, listed on a start page.</p>

Rust successor of [gosrvdir](https://github.com/AxelRHD/gosrvdir).

## Features

- 📁 **Directory listing** — file sizes and modification dates, sortable columns, filter box
- 🗂️ **Several shares** — a TOML config serves multiple folders, the start page lists them
- 🚦 **Allow list per share** — glob patterns for files that may be served; everything else is a 404 and not listed
- ⬇️ **Download or preview** — per share, plus a charset for legacy text files (e.g. `windows-1252`)
- 🧭 **Breadcrumb navigation**
- 🎨 **Themeable** — Auto, Nord, Squirrel, Archlinux, Monokai, Zenburn (choice is remembered in the browser)
- 🔒 **Basic Auth** — optional, inline or htpasswd (bcrypt)
- 🛡️ **Stays inside the share** — no `..`, symlinks pointing outside are ignored
- ⚡ **Single binary** — assets embedded (Open Props, Alpine.js), static musl build

## Usage

```bash
rsrvdir                        # serve the current directory on port 8080
rsrvdir ./mydir -p 9000        # serve mydir on port 9000
rsrvdir --host 127.0.0.1       # only listen on localhost
rsrvdir --theme nord
rsrvdir --auth admin:secret    # Basic Auth (inline, single user)
rsrvdir --auth-file .htpasswd  # Basic Auth (htpasswd file)
rsrvdir --config rsrvdir.toml  # several shares, see rsrvdir.example.toml
rsrvdir htpasswd .htpasswd admin   # add or update a user (prompts for the password)
```

| Flag | Env | Default | Description |
|------|-----|---------|-------------|
| `-c, --config` | `RSRVDIR_CONFIG` | — | TOML file with `[[share]]` entries |
| `-p, --port` | `RSRVDIR_PORT` | `8080` | Port (overrides `listen` from the config) |
| `--host` | `RSRVDIR_HOST` | `0.0.0.0` | Interface (overrides `listen` from the config) |
| `--theme` | `RSRVDIR_THEME` | `auto` | Default theme |
| `--auth` | — | — | Inline Basic Auth `user:password` |
| `--auth-file` | `RSRVDIR_AUTH_FILE` | — | htpasswd file (bcrypt) |

Logging: one line per request on stdout; level via `RUST_LOG` (default `info`).

### Allow and hide patterns

Patterns without `/` match the file name at any depth (`*.txt`), patterns with `/` match the path
relative to the share (`*/translog_*.log`); `*` never crosses a `/`. `allow` applies to files only –
folders stay navigable unless they match `hide`.

## Build

```bash
cargo build --release                                       # local
cargo build --release --target x86_64-unknown-linux-musl    # static binary for servers
cargo test
```
