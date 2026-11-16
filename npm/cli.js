#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { spawn } = require("node:child_process");

const MIN_NODE_VERSION = [22, 22, 2];

const PLATFORM_PACKAGES = {
  "linux-x64": "excalidraw-tui-linux-x64",
  "linux-arm64": "excalidraw-tui-linux-arm64",
  "darwin-x64": "excalidraw-tui-darwin-x64",
  "darwin-arm64": "excalidraw-tui-darwin-arm64",
  "win32-x64": "excalidraw-tui-win32-x64",
};

function supportedPlatforms() {
  return Object.keys(PLATFORM_PACKAGES).join(", ");
}

function ensureNodeVersion(version = process.versions.node) {
  const parts = version.split(".").map((part) => Number.parseInt(part, 10));
  for (let index = 0; index < MIN_NODE_VERSION.length; index += 1) {
    const actual = parts[index] || 0;
    const minimum = MIN_NODE_VERSION[index];
    if (actual > minimum) return;
    if (actual < minimum) {
      throw new Error(
        `Node.js >=${MIN_NODE_VERSION.join(".")} is required (found ${version}). ` +
          "Upgrade Node.js and retry.",
      );
    }
  }
}

function resolvePlatformPackage(platform = process.platform, arch = process.arch) {
  const key = `${platform}-${arch}`;
  const packageName = PLATFORM_PACKAGES[key];
  if (!packageName) {
    throw new Error(
      `unsupported platform ${key}; prebuilt binaries are available for ${supportedPlatforms()}. ` +
        "Set EXV_BIN to a custom exv build to run on this platform.",
    );
  }
  return packageName;
}

function resolveBinPath(options = {}) {
  const {
    env = process.env,
    packageDir = __dirname,
    platform = process.platform,
    arch = process.arch,
  } = options;

  if (env.EXV_BIN) {
    return { binPath: env.EXV_BIN, source: "EXV_BIN" };
  }

  const packageName = resolvePlatformPackage(platform, arch);
  let packageJsonPath;
  try {
    packageJsonPath = require.resolve(`${packageName}/package.json`, {
      paths: [packageDir],
    });
  } catch {
    throw new Error(
      `the platform package ${packageName} is not installed. ` +
        `Reinstall without --omit=optional, e.g. npm install -g excalidraw-tui@latest. ` +
        `Supported platforms: ${supportedPlatforms()}.`,
    );
  }

  const binaryName = platform === "win32" ? "exv.exe" : "exv";
  const binPath = path.join(path.dirname(packageJsonPath), "bin", binaryName);
  if (!fs.existsSync(binPath)) {
    throw new Error(
      `the platform package ${packageName} is missing bin/${binaryName}; ` +
        "reinstall with npm install -g excalidraw-tui@latest.",
    );
  }

  if (platform !== "win32") {
    try {
      fs.chmodSync(binPath, 0o755);
    } catch {
      // Package stores and some filesystems ignore mode changes; exec may still work.
    }
  }

  return { binPath, source: packageName };
}

function resolveRendererPath(options = {}) {
  const { env = process.env, packageDir = __dirname } = options;
  if (env.EXV_RENDERER) return env.EXV_RENDERER;

  const rendererPath = path.join(packageDir, "renderer", "worker.mjs");
  if (!fs.existsSync(rendererPath)) {
    throw new Error(
      `the bundled renderer is missing at ${rendererPath}; ` +
        "reinstall with npm install -g excalidraw-tui@latest.",
    );
  }
  return rendererPath;
}

function buildEnvironment(options = {}) {
  const {
    env = process.env,
    rendererPath,
    nodePath = process.execPath,
  } = options;
  return {
    ...env,
    EXV_RENDERER: rendererPath,
    EXV_JS_RUNTIME: env.EXV_JS_RUNTIME || nodePath,
  };
}

function forwardSignals(child) {
  const signals =
    process.platform === "win32"
      ? ["SIGTERM"]
      : ["SIGTERM", "SIGHUP", "SIGQUIT"];
  const handlers = new Map();

  for (const signal of signals) {
    const handler = () => {
      try {
        child.kill(signal);
      } catch {
        // The child may already have exited.
      }
    };
    handlers.set(signal, handler);
    process.on(signal, handler);
  }

  return () => {
    for (const [signal, handler] of handlers) {
      process.removeListener(signal, handler);
    }
  };
}

function main(argv = process.argv.slice(2)) {
  let binPath;
  let rendererPath;
  try {
    ensureNodeVersion();
    binPath = resolveBinPath().binPath;
    rendererPath = resolveRendererPath();
  } catch (error) {
    process.stderr.write(`exv: ${error.message}\n`);
    process.exitCode = 1;
    return null;
  }

  const child = spawn(binPath, argv, {
    stdio: "inherit",
    env: buildEnvironment({ rendererPath }),
  });

  const removeSignalHandlers = forwardSignals(child);

  child.on("error", (error) => {
    removeSignalHandlers();
    const hint =
      error.code === "ENOENT"
        ? ` (binary not found: ${binPath})`
        : "";
    process.stderr.write(`exv: failed to start ${binPath}: ${error.message}${hint}\n`);
    process.exitCode = 1;
  });

  child.on("exit", (code, signal) => {
    removeSignalHandlers();
    if (signal) {
      try {
        process.kill(process.pid, signal);
      } catch {
        process.exitCode = 1;
      }
      return;
    }
    process.exitCode = code ?? 1;
  });

  return child;
}

if (require.main === module) {
  main();
}

module.exports = {
  MIN_NODE_VERSION,
  PLATFORM_PACKAGES,
  supportedPlatforms,
  ensureNodeVersion,
  resolvePlatformPackage,
  resolveBinPath,
  resolveRendererPath,
  buildEnvironment,
  main,
};
