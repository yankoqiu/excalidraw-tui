#!/usr/bin/env node
import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const worker = resolve(
  process.argv[2] ?? join(root, "renderer", "worker.bundle.mjs"),
);
const documentJson = readFileSync(
  join(root, "tests", "data", "sample.excalidraw"),
  "utf8",
);

function run() {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(process.execPath, [worker], {
      cwd: dirname(worker),
      stdio: ["pipe", "pipe", "pipe"],
    });
    const lines = createInterface({ input: child.stdout });
    let stderr = "";
    let settled = false;

    const settle = async (error, _result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      lines.close();
      child.stdin.end();
      if (error) {
        child.kill();
        reject(error);
        return;
      }
      await new Promise((resolveExit) => {
        if (child.exitCode !== null || child.signalCode !== null) {
          resolveExit();
          return;
        }
        const killTimer = setTimeout(() => {
          child.kill();
          resolveExit();
        }, 5000);
        child.once("exit", () => {
          clearTimeout(killTimer);
          resolveExit();
        });
      });
      resolvePromise(_result);
    };

    const timer = setTimeout(
      () =>
        settle(
          new Error(`renderer smoke timed out\n${stderr}`.trim()),
        ),
      120_000,
    );

    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
      if (stderr.length > 8000) stderr = stderr.slice(-8000);
    });
    child.on("error", (error) => settle(error));
    child.on("exit", (code) => {
      if (!settled) {
        settle(
          new Error(
            `renderer exited with code ${code} before replying\n${stderr}`.trim(),
          ),
        );
      }
    });

    lines.on("line", (line) => {
      let message;
      try {
        message = JSON.parse(line);
      } catch {
        return;
      }
      if (message.ready === true) {
        child.stdin.write(
          `${JSON.stringify({ id: 1, json: documentJson })}\n`,
        );
        return;
      }
      if (message.fatal) {
        settle(new Error(`renderer fatal: ${message.fatal}`));
        return;
      }
      if (message.id !== 1) return;
      if (typeof message.error === "string") {
        settle(new Error(`renderer error: ${message.error}`));
        return;
      }
      const svg = message.svg;
      if (typeof svg !== "string" || !svg.includes("<svg")) {
        settle(new Error("renderer returned no SVG"));
        return;
      }
      for (const label of ["中文起点", "中文终点"]) {
        if (!svg.includes(label)) {
          settle(new Error(`SVG is missing expected label: ${label}`));
          return;
        }
      }
      if (message.elements !== 6) {
        settle(
          new Error(`expected 6 elements, received ${message.elements}`),
        );
        return;
      }
      settle(null, { bytes: Buffer.byteLength(svg), elements: message.elements });
    });
  });
}

try {
  const result = await run();
  console.log(
    `renderer smoke: ok (${result.elements} elements, ${(result.bytes / 1024).toFixed(1)} KiB SVG)`,
  );
} catch (error) {
  console.error(`renderer smoke failed: ${error.message}`);
  process.exitCode = 1;
}
