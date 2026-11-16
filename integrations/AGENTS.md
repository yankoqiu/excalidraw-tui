# Excalidraw authoring guide for coding agents

When asked to create or modify an `.excalidraw` file, follow these rules so the
result opens cleanly in Excalidraw and stays readable in `exv`'s live terminal
preview.

## Output contract

- Emit a single valid JSON object: no comments, trailing commas, or fences.
- Base skeleton:

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

- Include on each element: `id`, `type`, `x`, `y`, `width`, `height`, `angle`,
  `strokeColor`, `backgroundColor`, `fillStyle`, `strokeWidth`, `strokeStyle`,
  `roughness`, `opacity`, `seed`, `version`, `versionNonce`, `isDeleted`,
  `boundElements`, `updated`, `link`, `locked`.
- Ids must be unique. Use type-prefixed names such as `rect-api`, `text-api`,
  `arrow-api-db`.
- Coordinates grow right/down. Keep the diagram inside about 1200 x 800 units.

## Legibility rules

- At most 15 visible nodes. Split larger designs or summarize a subsystem.
- Choose one direction (top-to-bottom or left-to-right); do not mix.
- Nodes 160-240 wide x 60-100 tall. Leave >= 60 units horizontally and
  >= 80 units vertically between neighboring nodes.
- Font size >= 20. Short labels only (about 14 CJK chars / 24 Latin chars);
  use `\n` for a second line.
- Arrows connect shape borders, not centers. Route around nodes and labels.
  Never let an arrow cross a text label.
- Palette: stroke `#1e1e1e`; soft fills `#e7f5ff`, `#fff3bf`, `#d3f9d8`;
  accent stroke `#1971c2` or `#e03131` on at most two nodes.
- Avoid images unless asked; they require base64 entries in `files`.

## Minimal element examples

Bound shape + text:

```json
{
  "id": "rect-api", "type": "rectangle",
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
  "id": "text-api", "type": "text",
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

Arrow whose target starts at `x=460`:

```json
{
  "id": "arrow-api-db", "type": "arrow",
  "x": 300, "y": 140, "width": 160, "height": 0, "angle": 0,
  "strokeColor": "#1e1e1e", "backgroundColor": "transparent",
  "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid",
  "roughness": 1, "opacity": 100, "seed": 555, "version": 1,
  "versionNonce": 666, "isDeleted": false, "updated": 1700000000000,
  "link": null, "locked": false,
  "points": [[0, 0], [160, 0]], "lastCommittedPoint": null,
  "startBinding": { "elementId": "rect-api", "focus": 0, "gap": 8 },
  "endBinding": { "elementId": "rect-db", "focus": 0, "gap": 8 },
  "startArrowhead": null, "endArrowhead": "arrow", "elbowed": false
}
```

Binding checklist:

- Text bound to a shape: text has `containerId`, shape has matching
  `boundElements` text entry.
- Arrow points are relative; first point is `[0, 0]`.
- Binding ids must exist in the same document and must not be deleted.
- For elbow arrows, use intermediate points and axis-aligned segments.

## Validation and preview

```sh
jq . diagram.excalidraw > /dev/null
exv diagram.excalidraw
```

`exv` is interactive, so the human runs it in a terminal pane, not inside a
blocking tool call. It watches the file and refreshes as you save. Invalid or
partial JSON keeps the last good frame and shows `Syncing...` in the status bar.

When editing an existing file, preserve unrelated elements and styles, update
only changed elements, and set `isDeleted: true` for removals instead of
dropping referenced ids. Use temp-file-plus-rename saves when practical; `exv`
watches the parent directory and handles atomic renames.
