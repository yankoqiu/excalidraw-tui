# excalidraw-tui (`exv`)

[English](README.md) | [简体中文](README.zh-CN.md)

Live, zero-disk Excalidraw preview in your terminal. Point `exv` at an
`.excalidraw` file, put the terminal pane next to your AI coding agent, and watch
the diagram update as the agent writes. Pan, zoom, and inspect without leaving
the terminal.

```text
demo.excalidraw | 120% | 10 elements | kitty | Ready | hjkl pan  +/- zoom  0 reset  q quit
```

## Why

- **Zero disk temp files.** The SVG travels from the JavaScript renderer to Rust
  over a pipe; pixels go straight to the terminal. `exv` never writes `.svg`,
  `.png`, or cache files.
- **Resident renderer.** One Node/Bun process loads the Excalidraw renderer once
  and converts documents over newline-delimited JSON-RPC, so reloads are fast.
- **Atomic-save aware.** Editors and agents often write a temp file and rename
  it. `exv` watches the parent directory, debounces 300 ms, validates JSON, and
  keeps the last good frame while a write is incomplete.
- **Chinese/CJK ready.** The renderer rewrites font stacks and the rasterizer
  uses system font fallback, so Chinese labels stay readable instead of tofu.
- **No stray processes.** The worker is killed and reaped on every exit path,
  including panic unwinds and terminal-pane `SIGHUP`.
- **Works across terminals.** Kitty graphics, Sixel, or Unicode half-blocks,
  auto-detected with a manual override.

## Requirements

- Node.js >= 22.22.2 (Node 24 LTS or 26 recommended)
- A terminal with Kitty graphics, Sixel, or any true-color terminal (half-blocks)

No Rust toolchain, C compiler, or renderer dependency install is needed for the
published packages: they contain a prebuilt `exv` binary and a bundled renderer.

| Terminal | Typical path |
| --- | --- |
| Kitty, Ghostty, WezTerm | Kitty graphics protocol |
| iTerm2, foot, mlterm, WezTerm | Sixel |
| Zed / Herdr panes | host-terminal protocol if passed through |
| Anything else | Unicode half-blocks fallback |

Use `--protocol` if auto-detection guesses wrong.

## Install

Run once without installing globally:

```sh
npx --yes excalidraw-tui@latest diagram.excalidraw
```

Or install the `exv` command globally:

```sh
npm install -g excalidraw-tui
exv diagram.excalidraw
```

The npm package selects a prebuilt binary for your platform through optional
dependencies:

| OS | Architectures | Binary notes |
| --- | --- | --- |
| Linux | x64, arm64 | statically linked with musl |
| macOS | Intel x64, Apple Silicon arm64 | native |
| Windows | x64 | native |

Keep optional dependencies enabled. Installing with `--omit=optional` removes
the platform binary and `exv` will explain how to reinstall.

Updating:

```sh
npx --yes excalidraw-tui@latest diagram.excalidraw  # always fetch the latest release
npm update -g excalidraw-tui                        # update a global install
```

If no prebuilt binary exists for your OS or architecture, build from source or
set `EXV_BIN` to a custom `exv` binary.

### From source

This path is intended for development and unsupported platforms. It requires
Rust 1.88+ and Node.js >= 22.22.2.

```sh
git clone https://github.com/iam-yanko/excalidraw-tui.git
cd excalidraw-tui
./install.sh
```

`install.sh` runs `cargo build --release --locked`, installs the binary and
renderer to `${PREFIX:-$HOME/.local}/share/excalidraw-tui/`, and symlinks `exv`
into `${BIN_DIR:-$PREFIX/bin}`. It copies existing `renderer/node_modules` when
present, otherwise installs production dependencies with Bun or npm.

Overrides:

```sh
PREFIX=/usr/local ./install.sh
BIN_DIR="$HOME/bin" ./install.sh
EXV_FORCE_DEPS=1 ./install.sh   # reinstall renderer dependencies
```

Manual build:

```sh
npm ci --prefix renderer
cargo build --release --locked
./target/release/exv examples/demo.excalidraw
```

On Windows PowerShell:

```powershell
npm ci --prefix renderer
cargo build --release --locked
.\target\release\exv.exe examples\demo.excalidraw
```

The binary locates the renderer next to itself, in `../share/excalidraw-tui/`,
or in this repository's `renderer/`. Set `EXV_RENDERER` to an explicit
`worker.mjs` path if your layout differs. Set `EXV_JS_RUNTIME` to force a
specific Node or Bun executable. The npm launcher sets both automatically.

## Usage

```text
exv [OPTIONS] <FILE>
```

```sh
exv diagram.excalidraw
exv --theme dark --protocol kitty architecture.excalidraw
exv --no-watch diagram.excalidraw
npx --yes excalidraw-tui@latest --theme dark diagram.excalidraw
```

| Flag | Description |
| --- | --- |
| `--theme <light\|dark>` | Render on a light or dark canvas (default `light`) |
| `--no-watch` | Load once; requires the file to exist and contain valid JSON |
| `--protocol <auto\|kitty\|sixel\|halfblocks>` | Force a graphics protocol |

In watch mode the file may not exist yet: the parent directory must exist, and
the viewer shows `waiting for <name>` until the first valid save. `exv` must be
launched from an interactive terminal (stdin and stdout must be a TTY).

### Keys

| Key | Action |
| --- | --- |
| `h` `j` `k` `l` / arrow keys | Pan left/down/up/right |
| `+` or `=` | Zoom in 20% (re-rasterizes the SVG) |
| `-` or `_` | Zoom out 20% |
| `0` or `r` | Reset zoom and pan |
| `q`, `Esc`, `Ctrl+C` | Quit |

The status bar shows the filename, zoom percentage, element count, active
protocol, load/sync state, and the key legend.

## Live preview workflow

1. Start `exv diagram.excalidraw` in a terminal pane next to your agent.
2. Ask the agent to create or update the file.
3. The watcher notices the atomic save, validates the JSON, and replaces the
   frame. Partial JSON shows `Syncing...` and keeps the previous image.
4. Fix the file if needed — the next valid save appears automatically.

`exv` is read-only with respect to your diagram; it never rewrites the file.

## AI agent integrations

Ready-to-copy rules live in `integrations/`:

- `integrations/CLAUDE.md` — Claude Code
- `integrations/AGENTS.md` — generic agent format
- `integrations/qoder-rules.md` — Qoder project rules

Merge the relevant file into your project instructions rather than replacing
existing ones. The rules cover the JSON skeleton, element/binding fields,
readable layout limits, validation, and how to preview with `exv`.

Example prompt:

> Create `architecture.excalidraw` showing the request path from CLI to database.
> Keep it under 10 nodes and validate the JSON. I will preview it with exv.

## Herdr integration

`herdr-plugin.toml` exposes a popup pane and a `preview` action. Install `exv`
globally so the pane script can find it on `PATH`:

```sh
npm install -g excalidraw-tui
herdr plugin link "$PWD"     # link this checkout as a local plugin
herdr plugin action invoke excalidraw-tui.preview
```

The pane script (`scripts/herdr-preview.sh`) uses `EXV_FILE` when set, otherwise
searches the workspace described by `HERDR_PLUGIN_CONTEXT_JSON` for the newest
`.excalidraw` file. It falls back to `diagram.excalidraw` in the plugin
directory. `EXV_BIN` can point at a non-PATH binary. Bind the action to a key in
your Herdr config if you use it often.

## How it works

```text
 .excalidraw file
      │  parent-dir watch, 300 ms debounce, JSON validity gate
      ▼
 Rust TUI (ratatui + crossterm)
      │  {"id":N,"json":"..."} over stdin
      ▼
 resident Node/Bun worker (bundled @excalidraw/utils + jsdom shims)
      │  {"id":N,"svg":"...","elements":N} over stdout
      ▼
 resvg/usvg vector tree + system CJK font fallback
      │  viewport-sized RGBA pixmap (zoom/pan applied here)
      ▼
 ratatui-image → Kitty / Sixel / half-blocks → terminal
```

| Path | Role |
| --- | --- |
| `src/main.rs` | CLI, terminal session RAII, protocol picker, signal flags |
| `src/converter.rs` | Worker spawn/handshake/timeouts; `Drop` kills and waits |
| `src/rasterizer.rs` | SVG parse once, viewport rendering, light/dark themes |
| `src/ui.rs` | App state, keys, status bar, image sync |
| `src/watcher.rs` | Debounced parent-directory watch with filename filter |
| `renderer/worker.mjs` | jsdom environment, element normalization, `exportScene` |
| `npm/cli.js` | Selects the platform binary and launches it with inherited stdio |
| `scripts/prepare-npm.mjs` | Assembles and validates the npm tarballs |
| `tests/` | Chinese full pipeline, worker error recovery, atomic rename |

The renderer is bundled with esbuild into one file before publishing, so users
do not download `@excalidraw/utils`, jsdom, or their dependency tree. Only the
visible viewport is rasterized, and the SVG is parsed once per file change, so
zooming and panning do not invoke the JavaScript worker again.

### Zero-disk and process-lifecycle guarantees

- No temporary SVG/PNG/cache files: SVG text and pixels stay in memory.
- One generation counter discards stale conversion results after rapid saves.
- `Converter::drop` closes stdin, kills the child, and waits for it. Unix
  builds also convert `SIGHUP`/`SIGINT`/`SIGQUIT`/`SIGTERM` into an orderly quit.
- Terminal raw mode and the alternate screen are restored by RAII, including on
  errors.

## Development

```sh
npm ci --prefix renderer
npm run bundle --prefix renderer
npm run smoke --prefix renderer
node --test tests/npm-cli.test.mjs
cargo test --locked                  # unit + watcher + full pipeline
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

The pipeline test needs Node or Bun plus `renderer/node_modules`; otherwise it
skips with a message. It converts the Chinese sample at
`tests/data/sample.excalidraw`, checks the per-request error path, verifies the
worker survives a malformed document, and rasterizes the SVG in memory.

`Cargo.lock` intentionally pins `notify-types` to `2.2.0`: version `2.3.x`
requires Rust 1.91. Build with `--locked`, or use a newer toolchain if you
update it.

To preview the npm artifacts locally:

```sh
cargo build --release --locked
node scripts/prepare-npm.mjs --local --pack
```

Tarballs land in `dist/npm/tarballs/`; the script validates their contents with
`npm pack --dry-run`. Release publishing is documented in
[RELEASING.md](RELEASING.md).

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| Chinese labels are squares | Install a CJK system font such as Noto Sans CJK SC or Source Han Sans; the rasterizer falls back across installed fonts |
| `Node.js >=22.22.2 is required` | Upgrade Node; the renderer depends on a modern jsdom |
| Platform package is missing | Reinstall without `--omit=optional`; check `npm config get omit`, then run `npm install -g excalidraw-tui@latest` |
| `unsupported platform ...` | Use a supported platform, build from source, or point `EXV_BIN` at a custom binary |
| `must run in an interactive terminal` | Run `exv` directly in a terminal; stdin and stdout cannot be piped |
| `waiting for ...` forever | The parent directory must exist; save valid JSON to the exact path |
| `Syncing...` stuck | The file currently has invalid/truncated JSON; run `jq . file.excalidraw` |
| Renderer fails to start | Check `node --version`; for source builds run `npm ci --prefix renderer`; or set `EXV_RENDERER` |
| `exv` not found with Herdr | Use a global npm install so the pane script can resolve `exv` from `PATH` |
| Garbled or misplaced image | Try `--protocol halfblocks` first, then `sixel` / `kitty`; some terminals misreport font cell size |
| Image does not update | Ensure you edit the same filename; the watcher filters by name and watches the directory |
| Build fails on `notify-types` MSRV | Use the committed `Cargo.lock` with Rust 1.88+, or update Rust |

## Limitations

- Custom Excalidraw fonts are not embedded; system fonts are used instead, with
  CJK fallback.
- Image elements render only when their base64 data is present in `files`.
- Very large documents are capped at 40 million raster pixels to protect memory.
- Prebuilt npm binaries cover Linux x64/arm64, macOS x64/arm64, and Windows x64.
  Windows arm64 users need a source build or `EXV_BIN`.
- The Herdr plugin shell entrypoint targets Linux and macOS.

## License

MIT
