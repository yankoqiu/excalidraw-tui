#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const { PLATFORM_PACKAGES } = require("../npm/cli.js");

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const distRoot = join(root, "dist", "npm");
const mainDir = join(distRoot, "main");
const platformsDir = join(distRoot, "platforms");
const tarballsDir = join(distRoot, "tarballs");
const npmCommand = process.platform === "win32" ? "npm.cmd" : "npm";

const FORBIDDEN_ENTRY =
  /(^|\/)(node_modules|target|dist|\.git|\.qoder|\.npmrc)(\/|$)|\.tgz$/i;

function usage() {
  return `Usage: node scripts/prepare-npm.mjs [--local | --binaries <dir>] [--pack]

  --local          use target/release/exv for the current platform
  --binaries <dir> assemble all platforms from downloaded CI artifacts
  --pack           validate with npm pack --dry-run and create tarballs
  --help           show this message`;
}

function fail(message) {
  throw new Error(message);
}

function parseCargoPackage() {
  const text = readFileSync(join(root, "Cargo.toml"), "utf8");
  const header = text.indexOf("[package]");
  if (header === -1) fail("could not find [package] in Cargo.toml");
  const rest = text.slice(header + "[package]".length);
  const nextSection = rest.search(/\n\[/);
  const section = nextSection === -1 ? rest : rest.slice(0, nextSection);
  const version = section.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  const repository = section.match(/^repository\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) fail("Cargo.toml is missing package.version");
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
    fail(`Cargo.toml version is not semver: ${version}`);
  }
  return { version, repository };
}

function repositoryFields(repository) {
  const base = (repository || "https://github.com/iam-yanko/excalidraw-tui").replace(
    /\.git$/,
    "",
  );
  return {
    repository: { type: "git", url: `git+${base}.git` },
    homepage: `${base}#readme`,
    bugs: { url: `${base}/issues` },
  };
}

function parseArgs(argv) {
  const options = { local: false, binaries: null, pack: false };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--local") {
      options.local = true;
    } else if (arg === "--binaries") {
      index += 1;
      if (!argv[index]) fail("--binaries requires a directory");
      options.binaries = resolve(argv[index]);
    } else if (arg === "--pack") {
      options.pack = true;
    } else if (arg === "--help" || arg === "-h") {
      console.log(usage());
      process.exit(0);
    } else {
      fail(`unknown option: ${arg}\n\n${usage()}`);
    }
  }
  if (options.local && options.binaries) {
    fail("--local and --binaries cannot be used together");
  }
  if (!options.local && !options.binaries) {
    fail("choose --local or --binaries <dir>\n\n" + usage());
  }
  return options;
}

function localPlatformKey() {
  return `${process.platform}-${process.arch}`;
}

function findPlatformBinary(binariesDir, key) {
  const os = key.split("-")[0];
  const binaryName = os === "win32" ? "exv.exe" : "exv";
  const packageName = PLATFORM_PACKAGES[key];
  const candidates = [
    join(binariesDir, `exv-${key}`, binaryName),
    join(binariesDir, `exv-${key}`, "bin", binaryName),
    join(binariesDir, packageName, binaryName),
    join(binariesDir, packageName, "bin", binaryName),
    join(binariesDir, binaryName),
  ];
  const found = candidates.find((candidate) => existsSync(candidate));
  if (!found) {
    fail(
      `missing binary for ${key}; looked for:\n  ${candidates.join("\n  ")}`,
    );
  }
  return found;
}

function writeJson(file, value) {
  writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function copyExecutable(source, destination) {
  copyFileSync(source, destination);
  try {
    chmodSync(destination, 0o755);
  } catch {
    // Windows and some filesystems do not preserve Unix modes.
  }
}

function copyCommon(mainOrPlatformDir) {
  copyFileSync(join(root, "LICENSE"), join(mainOrPlatformDir, "LICENSE"));
}

function assembleMain({ version, repository, nodeRange }) {
  mkdirSync(join(mainDir, "renderer"), { recursive: true });
  copyExecutable(join(root, "npm", "cli.js"), join(mainDir, "cli.js"));
  copyFileSync(
    join(root, "renderer", "worker.bundle.mjs"),
    join(mainDir, "renderer", "worker.mjs"),
  );
  for (const name of ["README.md", "README.zh-CN.md"]) {
    copyFileSync(join(root, name), join(mainDir, name));
  }
  copyCommon(mainDir);

  const optionalDependencies = {};
  for (const packageName of Object.values(PLATFORM_PACKAGES)) {
    optionalDependencies[packageName] = version;
  }

  writeJson(join(mainDir, "package.json"), {
    name: "excalidraw-tui",
    version,
    description:
      "Preview and convert Excalidraw diagrams in the terminal with Kitty, Sixel, half-block output, and SVG/PNG export",
    bin: { exv: "cli.js" },
    files: [
      "cli.js",
      "renderer/worker.mjs",
      "README.md",
      "README.zh-CN.md",
      "LICENSE",
    ],
    engines: { node: nodeRange },
    optionalDependencies,
    keywords: [
      "excalidraw",
      "tui",
      "diagram",
      "terminal",
      "kitty",
      "sixel",
    ],
    license: "MIT",
    ...repositoryFields(repository),
    publishConfig: { access: "public" },
  });
}

function assemblePlatform(key, sourceBinary, { version, repository }) {
  const packageName = PLATFORM_PACKAGES[key];
  const [os, cpu] = key.split("-");
  const packageDir = join(platformsDir, packageName);
  mkdirSync(join(packageDir, "bin"), { recursive: true });

  const binaryName = os === "win32" ? "exv.exe" : "exv";
  copyExecutable(sourceBinary, join(packageDir, "bin", binaryName));
  copyCommon(packageDir);

  writeJson(join(packageDir, "package.json"), {
    name: packageName,
    version,
    description: `Prebuilt exv binary for ${os} ${cpu} (excalidraw-tui)`,
    files: ["bin/", "LICENSE"],
    os: [os],
    cpu: [cpu],
    preferUnplugged: true,
    license: "MIT",
    ...repositoryFields(repository),
    publishConfig: { access: "public" },
  });

  return packageDir;
}

function quoteWindowsArg(arg) {
  return /[\s"&|<>^()]/.test(arg) ? `"${arg.replaceAll('"', '""')}"` : arg;
}

function runNpm(packageDir, args) {
  const result =
    process.platform === "win32"
      ? spawnSync(
          [npmCommand, ...args.map(quoteWindowsArg)].join(" "),
          {
            cwd: packageDir,
            encoding: "utf8",
            shell: true,
            windowsHide: true,
          },
        )
      : spawnSync(npmCommand, args, {
          cwd: packageDir,
          encoding: "utf8",
          shell: false,
        });
  if (result.error) fail(`failed to run npm: ${result.error.message}`);
  if (result.status !== 0) {
    fail(
      `npm ${args.join(" ")} failed in ${packageDir}\n` +
        `${result.stdout || ""}${result.stderr || ""}`,
    );
  }
  return result.stdout;
}

function parsePackJson(stdout) {
  const start = stdout.indexOf("[");
  const end = stdout.lastIndexOf("]");
  if (start === -1 || end < start) {
    fail(`unexpected npm pack output:\n${stdout}`);
  }
  try {
    return JSON.parse(stdout.slice(start, end + 1));
  } catch (error) {
    fail(`could not parse npm pack output: ${error.message}\n${stdout}`);
  }
}

function validatePack(packageDir, expectedFiles, label) {
  const [result] = parsePackJson(
    runNpm(packageDir, [
      "pack",
      "--dry-run",
      "--json",
      "--ignore-scripts",
      "--pack-destination",
      tarballsDir,
    ]),
  );
  if (!result || !Array.isArray(result.files)) {
    fail(`npm pack --dry-run returned no file list for ${label}`);
  }
  const entries = result.files.map((file) =>
    file.path.replaceAll("\\", "/"),
  );
  const forbidden = entries.filter((entry) => FORBIDDEN_ENTRY.test(entry));
  if (forbidden.length > 0) {
    fail(`${label} would include forbidden files: ${forbidden.join(", ")}`);
  }
  const missing = expectedFiles.filter((entry) => !entries.includes(entry));
  if (missing.length > 0) {
    fail(`${label} is missing required files: ${missing.join(", ")}`);
  }
  console.log(
    `  ${label}: ${entries.length} files, ${(result.size / 1024 / 1024).toFixed(2)} MiB unpacked`,
  );
}

function packPackage(packageDir) {
  const [result] = parsePackJson(
    runNpm(packageDir, [
      "pack",
      "--json",
      "--ignore-scripts",
      "--pack-destination",
      tarballsDir,
    ]),
  );
  if (!result?.filename) fail(`npm pack returned no filename in ${packageDir}`);
  const tarball = join(tarballsDir, result.filename);
  const size = statSync(tarball).size;
  return { tarball, size, name: result.name };
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const { version, repository } = parseCargoPackage();
  const rendererPackage = JSON.parse(
    readFileSync(join(root, "renderer", "package.json"), "utf8"),
  );
  const nodeRange = rendererPackage.engines?.node;
  if (!nodeRange) fail("renderer/package.json is missing engines.node");
  if (!existsSync(join(root, "renderer", "worker.bundle.mjs"))) {
    fail("renderer/worker.bundle.mjs is missing; run: npm run bundle --prefix renderer");
  }

  const platforms = options.local
    ? [localPlatformKey()]
    : Object.keys(PLATFORM_PACKAGES);
  for (const key of platforms) {
    if (!PLATFORM_PACKAGES[key]) {
      fail(
        `unsupported platform ${key}; supported: ${Object.keys(PLATFORM_PACKAGES).join(", ")}`,
      );
    }
  }

  const binaries = new Map();
  for (const key of platforms) {
    if (options.local) {
      const binaryName = process.platform === "win32" ? "exv.exe" : "exv";
      const path = join(root, "target", "release", binaryName);
      if (!existsSync(path)) {
        fail(`${path} is missing; run: cargo build --release --locked`);
      }
      binaries.set(key, path);
    } else {
      binaries.set(key, findPlatformBinary(options.binaries, key));
    }
  }

  rmSync(distRoot, { recursive: true, force: true });
  mkdirSync(mainDir, { recursive: true });
  mkdirSync(platformsDir, { recursive: true });

  assembleMain({ version, repository, nodeRange });
  const platformDirs = [];
  for (const key of platforms) {
    platformDirs.push(
      assemblePlatform(key, binaries.get(key), { version, repository }),
    );
  }

  console.log(`Assembled npm packages v${version} for ${platforms.join(", ")}`);
  for (const dir of platformDirs) console.log(`  ${dir}`);
  console.log(`  ${mainDir}`);

  if (!options.pack) {
    console.log("Run with --pack to create tarballs.");
    return;
  }

  mkdirSync(tarballsDir, { recursive: true });
  console.log("Validating package contents:");
  for (const [index, dir] of platformDirs.entries()) {
    const key = platforms[index];
    const os = key.split("-")[0];
    validatePack(
      dir,
      [
        "package.json",
        "LICENSE",
        `bin/${os === "win32" ? "exv.exe" : "exv"}`,
      ],
      PLATFORM_PACKAGES[key],
    );
  }
  validatePack(
    mainDir,
    [
      "package.json",
      "cli.js",
      "LICENSE",
      "README.md",
      "README.zh-CN.md",
      "renderer/worker.mjs",
    ],
    "excalidraw-tui",
  );

  const packedPlatforms = [];
  for (const dir of platformDirs) packedPlatforms.push(packPackage(dir));
  const packedMain = packPackage(mainDir);

  console.log("Tarballs:");
  for (const item of [...packedPlatforms, packedMain]) {
    console.log(`  ${item.tarball} (${(item.size / 1024 / 1024).toFixed(2)} MiB)`);
  }

  if (options.local) {
    console.log("\nLocal install test:");
    console.log(
      `  npm install -g ${packedPlatforms[0].tarball} ${packedMain.tarball}`,
    );
  }

  console.log("\nPublish order (platforms first, main last):");
  for (const item of [...packedPlatforms, packedMain]) {
    console.log(`  npm publish ${item.tarball} --access public --provenance`);
  }
}

try {
  await main();
} catch (error) {
  console.error(`prepare-npm: ${error.message}`);
  process.exitCode = 1;
}
