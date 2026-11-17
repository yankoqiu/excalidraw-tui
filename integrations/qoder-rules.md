# Qoder rules: Excalidraw diagrams

Copy this file to `.qoder/rules/excalidraw.md` in a project where agents should
author diagrams, or import it from project rules.

## When this applies

Any request to create, update, or repair an `.excalidraw` file that a human may
preview with `exv`.

## Required output

- Write one valid JSON object; no comments, trailing commas, or fences.
- Use the standard document skeleton:

```json
{
  "type": "excalidraw",
  "version": 2,
  "source": "https://excalidraw.com",
  "elements": [],
  "appState": { "gridSize": null, "viewBackgroundColor": "#ffffff" },
  "files": {}
}
```

- Elements need unique type-prefixed ids (`rect-api`, `text-api`, `arrow-api-db`)
  and the usual fields: `x`, `y`, `width`, `height`, `angle`, `strokeColor`,
  `backgroundColor`, `fillStyle`, `strokeWidth`, `strokeStyle`, `roughness`,
  `opacity`, `seed`, `version`, `versionNonce`, `isDeleted`, `boundElements`,
  `updated`, `link`, `locked`.
- Keep the canvas around 1200 x 800 units; coordinates grow right and down.

## Diagram quality

- Maximum 15 visible nodes; split or summarize larger systems.
- One flow direction per diagram (top-to-bottom or left-to-right).
- Nodes 160-240 wide x 60-100 tall, with >= 60 units horizontal / >= 80 units
  vertical gaps. Align nodes on shared axes.
- Fonts >= 20; labels concise (about 14 CJK chars or 24 Latin chars), details on
  a second line using `\n`.
- Arrows start/end on shape borders, never centers; route around nodes, labels,
  and other arrows.
- Palette: `#1e1e1e` strokes; `#e7f5ff`, `#fff3bf`, `#d3f9d8` fills; at most two
  accent nodes in `#1971c2` or `#e03131`.
- Avoid images unless requested; they require base64 `files` entries.

## Binding rules

- Container-bound text: text has `containerId: "<shape-id>"`; the shape has
  `boundElements: [{ "type": "text", "id": "<text-id>" }]`.
- Arrow `points` are relative to the arrow origin; first point is `[0, 0]`.
- `startBinding`/`endBinding` must reference existing element ids.
- Elbow arrows use intermediate points and axis-aligned segments.

## Validate and hand off

```sh
jq . diagram.excalidraw > /dev/null
exv --watch diagram.excalidraw
```

Do not block an agent turn on `exv`; it is an interactive TUI. Tell the user to
run it in a terminal pane. With `--watch`, it live-reloads on save, tolerates
partial JSON by keeping the last good frame, and reacts to temp-file-plus-rename
saves.

For edits: read the file first, preserve unrelated content and ids, update only
changed elements, and mark removals with `isDeleted: true`. Prefer atomic saves.
