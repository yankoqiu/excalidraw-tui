# Excalidraw diagram rules for Claude Code

Use these rules whenever a task asks you to create or edit an `.excalidraw` file.
The goal is a diagram that opens in Excalidraw and renders legibly in the `exv`
terminal preview while a human watches.

## File contract

- Write one JSON object. No comments, trailing commas, or Markdown fences.
- Use this top-level shape:

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

- Every element should carry: `id`, `type`, `x`, `y`, `width`, `height`,
  `angle`, `strokeColor`, `backgroundColor`, `fillStyle`, `strokeWidth`,
  `strokeStyle`, `roughness`, `opacity`, `seed`, `version`, `versionNonce`,
  `isDeleted`, `boundElements`, `updated`, `link`, `locked`.
- Keep ids unique and descriptive: `rect-api`, `text-api`, `arrow-api-db`.
  Never reuse an id.
- Canvas coordinates grow right and down. Keep the whole diagram inside
  roughly 1200 x 800 units unless asked otherwise.

## Layout rules

- Maximum 15 visible nodes. If the design is larger, split it into two files
  or summarize a subsystem instead of shrinking text.
- Pick one flow direction: top-to-bottom or left-to-right. Do not mix.
- Node size 160-240 wide and 60-100 tall, with at least 60 units horizontally
  or 80 units vertically between neighbors. Align nodes on a shared axis.
- Font size 20 or larger. One short label per shape (about 14 CJK characters
  or 24 Latin characters). Put detail on a second line with `\n`.
- Arrows must start and end at shape borders, not centers. Add waypoints so
  routes do not cross nodes, labels, or other arrows.
- Use a small palette: stroke `#1e1e1e`; backgrounds `transparent`, `#e7f5ff`,
  `#fff3bf`, or `#d3f9d8`; accents `#1971c2` or `#e03131` for at most two
  highlighted nodes.
- Avoid `image` elements unless explicitly required; they need entries in
  `files` and inflate the document.

## Element recipes

Container with centered text (bound both ways):

```json
{
  "id": "rect-api",
  "type": "rectangle",
  "x": 100, "y": 100, "width": 200, "height": 80, "angle": 0,
  "strokeColor": "#1e1e1e", "backgroundColor": "#e7f5ff", "fillStyle": "solid",
  "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1, "opacity": 100,
  "seed": 123456789, "version": 1, "versionNonce": 987654321,
  "isDeleted": false, "updated": 1700000000000, "link": null, "locked": false,
  "roundness": { "type": 3 },
  "boundElements": [{ "type": "text", "id": "text-api" }]
}
```

```json
{
  "id": "text-api",
  "type": "text",
  "x": 120, "y": 122, "width": 160, "height": 36, "angle": 0,
  "strokeColor": "#1e1e1e", "backgroundColor": "transparent",
  "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid",
  "roughness": 1, "opacity": 100, "seed": 1234, "version": 1,
  "versionNonce": 47, "isDeleted": false, "updated": 1700000000000,
  "link": null, "locked": false,
  "text": "API", "originalText": "API", "fontSize": 24,
  "fontFamily": 1, "textAlign": "center", "verticalAlign": "middle",
  "containerId": "rect-api", "lineHeight": 1.25
}
```

Arrow between the container above and another node at `x=460`:

```json
{
  "id": "arrow-api-db",
  "type": "arrow",
  "x": 300, "y": 140, "width": 160, "height": 0, "angle": 0,
  "strokeColor": "#1e1e1e", "backgroundColor": "transparent",
  "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid",
  "roughness": 1, "opacity": 100, "seed": 555, "version": 1,
  "versionNonce": 666, "isDeleted": false, "updated": 1700000000000,
  "link": null, "locked": false,
  "points": [[0, 0], [160, 0]],
  "lastCommittedPoint": null,
  "startBinding": { "elementId": "rect-api", "focus": 0, "gap": 8 },
  "endBinding": { "elementId": "rect-db", "focus": 0, "gap": 8 },
  "startArrowhead": null, "endArrowhead": "arrow", "elbowed": false
}
```

Rules for bindings:

- A bound text element needs `containerId` pointing at the container, and the
  container needs `boundElements` containing that text id.
- Arrow `points` are relative to the arrow's `x`/`y`; the first point is
  `[0, 0]`. Bindings use element ids that exist in the same file.
- For an elbow route, add intermediate points; keep segments axis-aligned.

## Validate before finishing

1. Validate JSON:

   ```sh
   jq . diagram.excalidraw > /dev/null
   ```

   If `jq` is unavailable, use `node -e 'JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"))' diagram.excalidraw`.

2. If `exv` is on PATH, tell the user it is available:

   ```sh
   exv --watch diagram.excalidraw
   ```

   Do not block an agent turn on this interactive command. The user runs it in
   a terminal pane; with `--watch`, `exv` updates automatically.

3. Prefer atomic saves when editing an existing file: write a sibling temp file
   and rename it over the target. `--watch` monitors the parent directory, so
   both plain writes and rename-based saves are detected.

4. A truncated or invalid write is harmless for the preview: the last good
   frame stays visible and the status bar shows `Syncing...`. Still fix the
   file before reporting completion.

## When editing an existing diagram

- Read the file first, preserve unrelated elements, ids, styles, and `files`.
- Update `version`/`versionNonce`/`updated` on changed elements only.
- Remove an element by setting `isDeleted` to `true` rather than dropping
  references that other elements may use.
- Keep the requested filename. If the user did not name one, use
  `diagram.excalidraw` in the project root.
