#!/usr/bin/env node
import { stat } from "node:fs/promises";
import { build } from "esbuild";

await build({
  entryPoints: ["worker.mjs"],
  outfile: "worker.bundle.mjs",
  bundle: true,
  platform: "node",
  format: "esm",
  target: "node20",
  minify: true,
  legalComments: "eof",
  external: ["canvas", "@napi-rs/canvas"],
  banner: {
    // jsdom resolves a synchronous-XHR worker at module load. We never use
    // synchronous XHR, so return a placeholder instead of shipping that file.
    js: [
      "import { createRequire as __exvCreateRequire } from 'node:module';",
      "import { fileURLToPath as __exvFileURLToPath } from 'node:url';",
      "const __exvNativeRequire = __exvCreateRequire(import.meta.url);",
      "const require = (id) => __exvNativeRequire(id);",
      "require.resolve = (id, options) => id === './xhr-sync-worker.js'",
      "  ? __exvFileURLToPath(new URL('./xhr-sync-worker.js', import.meta.url))",
      "  : __exvNativeRequire.resolve(id, options);",
    ].join("\n"),
  },
  logLevel: "info",
});

const { size } = await stat("worker.bundle.mjs");
console.log(`worker.bundle.mjs: ${(size / 1024 / 1024).toFixed(1)} MiB`);
