#!/usr/bin/env bash
# Developer/source install. End users should run:
#   npx --yes excalidraw-tui@latest <file>
# or: npm install -g excalidraw-tui
#
# Install exv and its resident renderer into a user-local prefix.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"
APP_DIR="$PREFIX/share/excalidraw-tui"
RENDERER_SRC="$ROOT/renderer"
RENDERER_DST="$APP_DIR/renderer"

say() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null 2>&1 ||
    die "cargo not found; install the Rust toolchain from https://rustup.rs"

if ! command -v node >/dev/null 2>&1 && ! command -v bun >/dev/null 2>&1; then
    die "Node.js >= 22.22.2 or Bun is required to run the renderer"
fi

say "Building exv (release)"
cargo build --release --locked --manifest-path "$ROOT/Cargo.toml"

say "Installing into $APP_DIR"
install -d "$APP_DIR" "$RENDERER_DST" "$BIN_DIR"
install -m 755 "$ROOT/target/release/exv" "$APP_DIR/exv"
install -m 644 "$RENDERER_SRC/worker.mjs" "$RENDERER_SRC/package.json" "$RENDERER_DST/"
if [ -f "$RENDERER_SRC/package-lock.json" ]; then
    install -m 644 "$RENDERER_SRC/package-lock.json" "$RENDERER_DST/"
fi

DEPS_READY=0
if [ -d "$RENDERER_DST/node_modules/@excalidraw/utils" ] &&
    [ "${EXV_FORCE_DEPS:-0}" != "1" ]; then
    DEPS_READY=1
fi

if [ "$DEPS_READY" -eq 0 ]; then
    if [ -d "$RENDERER_SRC/node_modules/@excalidraw/utils" ] &&
        [ "${EXV_FORCE_DEPS:-0}" != "1" ]; then
        say "Copying renderer dependencies from the working tree"
        install -d "$RENDERER_DST/node_modules"
        cp -R "$RENDERER_SRC/node_modules/." "$RENDERER_DST/node_modules/"
    elif command -v bun >/dev/null 2>&1; then
        say "Installing renderer dependencies with Bun"
        (
            cd "$RENDERER_DST"
            bun install --production
        )
    elif command -v npm >/dev/null 2>&1; then
        say "Installing renderer dependencies with npm"
        (
            cd "$RENDERER_DST"
            if [ -f package-lock.json ]; then
                npm ci --omit=dev
            else
                npm install --omit=dev
            fi
        )
    else
        die "npm is required to install renderer dependencies when Bun is absent"
    fi
fi

ln -sfn "$APP_DIR/exv" "$BIN_DIR/exv"

say "Installed: $APP_DIR/exv"
say "Symlink:   $BIN_DIR/exv"

case ":$PATH:" in
*":$BIN_DIR:"*) ;;
*)
    say "Add $BIN_DIR to PATH to run exv from anywhere:"
    printf '\n    export PATH="%s:$PATH"\n\n' "$BIN_DIR"
    ;;
esac

say "Try it: exv $ROOT/examples/demo.excalidraw"
