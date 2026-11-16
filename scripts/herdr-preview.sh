#!/bin/sh
# Herdr pane entrypoint: locate a diagram and hand the terminal to exv.
set -eu

exv_bin="${EXV_BIN:-exv}"
file="${EXV_FILE:-${1:-}}"

if [ -n "$file" ] && [ ! -f "$file" ]; then
    printf 'exv: %s does not exist yet; watching for it to be created\n' "$file" >&2
fi

if [ -z "$file" ] && [ -n "${HERDR_PLUGIN_CONTEXT_JSON:-}" ] &&
    command -v python3 >/dev/null 2>&1; then
    file="$(CTX="$HERDR_PLUGIN_CONTEXT_JSON" python3 - <<'PY'
import json
import os
import sys

try:
    context = json.loads(os.environ["CTX"])
except Exception:
    sys.exit(0)

candidates = []


def collect(node):
    if isinstance(node, dict):
        for key, value in node.items():
            lowered = key.lower()
            if isinstance(value, str) and value and any(
                token in lowered for token in ("cwd", "path", "root", "dir")
            ):
                candidates.append(value)
            else:
                collect(value)
    elif isinstance(node, list):
        for item in node:
            collect(item)


collect(context)

best = None
for base in candidates:
    if not os.path.isdir(base):
        continue
    for dirpath, dirnames, filenames in os.walk(base):
        if os.path.relpath(dirpath, base).count(os.sep) >= 3:
            dirnames[:] = []
        dirnames[:] = [
            name
            for name in dirnames
            if name not in (".git", "node_modules", "target", "dist", "build")
        ]
        for name in filenames:
            if not name.endswith(".excalidraw"):
                continue
            path = os.path.join(dirpath, name)
            try:
                mtime = os.path.getmtime(path)
            except OSError:
                continue
            if best is None or mtime > best[0]:
                best = (mtime, path)
    if best is not None:
        break

if best is not None:
    print(best[1])
PY
)"
fi

if [ -z "$file" ]; then
    file="${EXV_DEFAULT_FILE:-diagram.excalidraw}"
fi

printf 'exv: previewing %s\n' "$file" >&2
exec "$exv_bin" "$file"
