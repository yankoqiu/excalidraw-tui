import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const cliPath = path.join(root, "npm", "cli.js");
const workerPath = path.join(root, "renderer", "worker.mjs");
const cli = require(cliPath);

test("maps every supported platform to its optional package", () => {
  const expected = {
    "linux-x64": "excalidraw-tui-linux-x64",
    "linux-arm64": "excalidraw-tui-linux-arm64",
    "darwin-x64": "excalidraw-tui-darwin-x64",
    "darwin-arm64": "excalidraw-tui-darwin-arm64",
    "win32-x64": "excalidraw-tui-win32-x64",
  };
  assert.deepEqual(cli.PLATFORM_PACKAGES, expected);
  for (const [key, packageName] of Object.entries(expected)) {
    const [platform, arch] = key.split("-");
    assert.equal(cli.resolvePlatformPackage(platform, arch), packageName);
  }
});

test("requires Node.js 22.22.2 or newer", () => {
  assert.doesNotThrow(() => cli.ensureNodeVersion("22.22.2"));
  assert.doesNotThrow(() => cli.ensureNodeVersion("24.15.0"));
  assert.throws(
    () => cli.ensureNodeVersion("20.19.0"),
    /Node\.js >=22\.22\.2 is required/,
  );
});

test("rejects unsupported platforms with actionable guidance", () => {
  assert.throws(
    () => cli.resolvePlatformPackage("freebsd", "x64"),
    (error) =>
      error.message.includes("unsupported platform freebsd-x64") &&
      error.message.includes("EXV_BIN"),
  );
});

test("honors EXV_BIN without resolving a platform package", () => {
  const result = cli.resolveBinPath({
    env: { EXV_BIN: "/custom/bin/exv" },
    platform: "freebsd",
    arch: "riscv64",
  });
  assert.equal(result.binPath, "/custom/bin/exv");
  assert.equal(result.source, "EXV_BIN");
});

test("builds child environment with renderer and current Node", () => {
  const env = cli.buildEnvironment({
    env: { PATH: "/usr/bin", EXV_RENDERER: "/custom/worker.mjs" },
    rendererPath: "/package/renderer/worker.mjs",
    nodePath: "/opt/node/bin/node",
  });
  assert.equal(env.PATH, "/usr/bin");
  assert.equal(env.EXV_RENDERER, "/package/renderer/worker.mjs");
  assert.equal(env.EXV_JS_RUNTIME, "/opt/node/bin/node");
});

test("preserves an explicit EXV_JS_RUNTIME override", () => {
  const env = cli.buildEnvironment({
    env: { EXV_JS_RUNTIME: "/custom/bun" },
    rendererPath: "/package/renderer/worker.mjs",
    nodePath: "/opt/node/bin/node",
  });
  assert.equal(env.EXV_JS_RUNTIME, "/custom/bun");
});

test("resolves the bundled renderer by default", () => {
  assert.equal(
    cli.resolveRendererPath({ env: {}, packageDir: root }),
    workerPath,
  );
});

test("propagates args, environment, and exit code to the binary", () => {
  const marker = "npm-wrapper-test-marker";
  const probe =
    "process.stdout.write(JSON.stringify({" +
    "renderer: process.env.EXV_RENDERER," +
    "runtime: process.env.EXV_JS_RUNTIME," +
    "marker: process.env.EXV_TEST_MARKER" +
    "})); process.exit(7)";
  const result = spawnSync(process.execPath, [cliPath, "-e", probe], {
    cwd: root,
    encoding: "utf8",
    env: {
      ...process.env,
      EXV_BIN: process.execPath,
      EXV_RENDERER: workerPath,
      EXV_TEST_MARKER: marker,
    },
  });

  assert.equal(result.status, 7, result.stderr);
  const payload = JSON.parse(result.stdout.trim());
  assert.equal(payload.renderer, workerPath);
  assert.equal(payload.runtime, process.execPath);
  assert.equal(payload.marker, marker);
});
