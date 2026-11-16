#!/usr/bin/env node
/**
 * Long-lived Excalidraw JSON -> SVG renderer.
 *
 * Protocol: newline-delimited JSON over stdin/stdout.
 *   request  : {"id": <number>, "json": "<.excalidraw content>"}
 *   response : {"id": <number>, "svg": "<svg ...>", "elements": <number>}
 *            | {"id": <number>, "error": "<message>"}
 *   startup  : {"ready": true} | {"fatal": "<message>"}
 *
 * All conversion happens in memory. Nothing is written to disk.
 */

import { createInterface } from "node:readline";
import { JSDOM } from "jsdom";

const FONT_STACK =
  "'Microsoft YaHei', 'Microsoft YaHei UI', 'PingFang SC', 'Hiragino Sans GB', " +
  "'Noto Sans CJK SC', 'Noto Sans SC', 'Source Han Sans SC', " +
  "'WenQuanYi Micro Hei', 'Segoe UI', 'Helvetica Neue', Arial, sans-serif";

const KNOWN_TYPES = new Set([
  "rectangle",
  "diamond",
  "ellipse",
  "arrow",
  "line",
  "text",
  "freedraw",
  "image",
  "frame",
]);

const MAX_ERROR_LENGTH = 2000;

let elementCounter = 0;

function num(value, fallback) {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function str(value, fallback) {
  return typeof value === "string" ? value : fallback;
}

function randomInt() {
  return Math.floor(Math.random() * 2 ** 31);
}

// ---------------------------------------------------------------------------
// DOM environment
// ---------------------------------------------------------------------------

function installCanvasStub(window) {
  const makeContext = () => {
    const ctx = {
      font: "16px sans-serif",
      fillStyle: "#000000",
      strokeStyle: "#000000",
      lineWidth: 1,
      textAlign: "left",
      textBaseline: "alphabetic",
      globalAlpha: 1,
      direction: "ltr",
      measureText(text) {
        const match = /(\d+(?:\.\d+)?)px/.exec(ctx.font || "");
        const size = match ? parseFloat(match[1]) : 16;
        let width = 0;
        for (const ch of String(text)) {
          width += charAdvance(ch, size);
        }
        return {
          width,
          actualBoundingBoxAscent: size * 0.8,
          actualBoundingBoxDescent: size * 0.2,
          actualBoundingBoxLeft: 0,
          actualBoundingBoxRight: width,
          fontBoundingBoxAscent: size * 0.8,
          fontBoundingBoxDescent: size * 0.2,
        };
      },
    };
    for (const method of [
      "save",
      "restore",
      "beginPath",
      "closePath",
      "moveTo",
      "lineTo",
      "quadraticCurveTo",
      "bezierCurveTo",
      "arc",
      "arcTo",
      "rect",
      "fill",
      "stroke",
      "clip",
      "fillRect",
      "strokeRect",
      "clearRect",
      "fillText",
      "strokeText",
      "translate",
      "rotate",
      "scale",
      "setTransform",
      "resetTransform",
      "setLineDash",
      "drawImage",
      "createLinearGradient",
      "createPattern",
    ]) {
      ctx[method] = () => (method.startsWith("create") ? {} : undefined);
    }
    ctx.getImageData = () => ({ data: new Uint8ClampedArray(4), width: 1, height: 1 });
    ctx.putImageData = () => undefined;
    return ctx;
  };

  window.HTMLCanvasElement.prototype.getContext = function getContext(type) {
    return type === "2d" ? makeContext() : null;
  };
  window.HTMLCanvasElement.prototype.toDataURL = () => "data:image/png;base64,";
  window.HTMLCanvasElement.prototype.toBlob = (callback) => callback?.(null);
}

function installDomGlobals() {
  const dom = new JSDOM("<!doctype html><html><body></body></html>", {
    pretendToBeVisual: true,
    url: "http://localhost/",
  });
  const { window } = dom;

  window.matchMedia = (query) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener() {},
    removeListener() {},
    addEventListener() {},
    removeEventListener() {},
    dispatchEvent() {
      return false;
    },
  });
  if (!window.CSS) {
    window.CSS = { supports: () => false, escape: (value) => String(value) };
  } else if (typeof window.CSS.supports !== "function") {
    window.CSS.supports = () => false;
  }
  if (!window.document.fonts) {
    Object.defineProperty(window.document, "fonts", {
      configurable: true,
      value: {
        ready: Promise.resolve(),
        status: "loaded",
        check: () => true,
        add() {},
        delete() {},
        clear() {},
        forEach() {},
        addEventListener() {},
        removeEventListener() {},
      },
    });
  }

  installCanvasStub(window);

  const globals = [
    "window",
    "document",
    "navigator",
    "location",
    "history",
    "DOMParser",
    "XMLSerializer",
    "Node",
    "Element",
    "HTMLElement",
    "HTMLCanvasElement",
    "HTMLImageElement",
    "SVGElement",
    "SVGSVGElement",
    "Image",
    "Blob",
    "File",
    "FileReader",
    "getComputedStyle",
    "MutationObserver",
    "Event",
    "CustomEvent",
    "KeyboardEvent",
    "MouseEvent",
    "PointerEvent",
    "matchMedia",
    "requestAnimationFrame",
    "cancelAnimationFrame",
    "requestIdleCallback",
    "cancelIdleCallback",
    "CSS",
    "localStorage",
    "sessionStorage",
    "screen",
    "devicePixelRatio",
    "innerWidth",
    "innerHeight",
    "scrollX",
    "scrollY",
    "pageXOffset",
    "pageYOffset",
  ];
  const setGlobal = (key, value) => {
    try {
      Object.defineProperty(globalThis, key, {
        configurable: true,
        writable: true,
        value,
      });
    } catch {
      // Non-configurable Node globals (e.g. crypto) stay untouched.
    }
  };

  for (const key of globals) {
    if (key === "window") {
      setGlobal("window", window);
      continue;
    }
    if (window[key] !== undefined) {
      setGlobal(
        key,
        typeof window[key] === "function" && !window[key].prototype
          ? window[key].bind(window)
          : window[key],
      );
    }
  }
  setGlobal("self", window);
  setGlobal("top", window);
  setGlobal("parent", window);
  setGlobal("frames", window);

  if (typeof globalThis.Path2D === "undefined") {
    globalThis.Path2D = class Path2D {
      constructor(path) {
        this.path = path;
      }
    };
    window.Path2D = globalThis.Path2D;
  }
  if (typeof globalThis.ResizeObserver === "undefined") {
    globalThis.ResizeObserver = class ResizeObserver {
      observe() {}
      unobserve() {}
      disconnect() {}
    };
    window.ResizeObserver = globalThis.ResizeObserver;
  }
  if (typeof globalThis.requestIdleCallback === "undefined") {
    globalThis.requestIdleCallback = (callback) =>
      setTimeout(() => callback({ didTimeout: false, timeRemaining: () => 5 }), 0);
    globalThis.cancelIdleCallback = (handle) => clearTimeout(handle);
  }
  if (typeof globalThis.matchMedia === "undefined") {
    globalThis.matchMedia = window.matchMedia;
  }
}

// ---------------------------------------------------------------------------
// Element normalization
// ---------------------------------------------------------------------------

function isWideChar(ch) {
  const code = ch.codePointAt(0) ?? 0;
  return (
    (code >= 0x1100 && code <= 0x115f) ||
    (code >= 0x2e80 && code <= 0xa4cf) ||
    (code >= 0xac00 && code <= 0xd7a3) ||
    (code >= 0xf900 && code <= 0xfaff) ||
    (code >= 0xfe30 && code <= 0xfe4f) ||
    (code >= 0xff00 && code <= 0xff60) ||
    (code >= 0xffe0 && code <= 0xffe6) ||
    (code >= 0x1f300 && code <= 0x1faff) ||
    (code >= 0x20000 && code <= 0x3fffd)
  );
}

function charAdvance(ch, fontSize) {
  if (ch === "\t") return fontSize * 2;
  if (ch === " ") return fontSize * 0.3;
  if (isWideChar(ch)) return fontSize;
  return fontSize * 0.55;
}

function estimateTextWidth(line, fontSize) {
  let width = 0;
  for (const ch of line) {
    width += charAdvance(ch, fontSize);
  }
  return width;
}

function normalizePoints(el) {
  let points = Array.isArray(el.points)
    ? el.points
        .filter((point) => Array.isArray(point) && point.length >= 2)
        .map((point) => [num(point[0], 0), num(point[1], 0)])
    : [];

  if (points.length === 0) {
    const width = num(el.width, 0);
    const height = num(el.height, 0);
    points =
      width !== 0 || height !== 0
        ? [
            [0, 0],
            [width, height],
          ]
        : [
            [0, 0],
            [120, 0],
          ];
  } else if (points.length === 1) {
    points.push([points[0][0] + 120, points[0][1]]);
  }

  const minX = Math.min(...points.map((point) => point[0]));
  const minY = Math.min(...points.map((point) => point[1]));
  if (minX !== 0 || minY !== 0) {
    el.x = num(el.x, 0) + minX;
    el.y = num(el.y, 0) + minY;
    points = points.map(([x, y]) => [x - minX, y - minY]);
  }
  return points;
}

function normalizeText(el) {
  el.text = str(el.text, "");
  el.originalText = str(el.originalText, el.text);
  el.fontSize = num(el.fontSize, 20);
  el.fontFamily = num(el.fontFamily, 1);
  el.lineHeight = num(el.lineHeight, 1.25);
  el.textAlign = str(el.textAlign, "left");
  el.verticalAlign = str(el.verticalAlign, "top");
  el.containerId = el.containerId ?? null;
  el.autoResize = el.autoResize ?? el.containerId === null;
  if (el.containerId === undefined) el.containerId = null;

  const lines = el.text.split("\n");
  const estimatedWidth = Math.max(
    el.fontSize,
    ...lines.map((line) => estimateTextWidth(line, el.fontSize)),
  );
  const estimatedHeight = Math.max(
    el.fontSize * el.lineHeight,
    lines.length * el.fontSize * el.lineHeight,
  );

  if (!(num(el.width, 0) > 0)) el.width = Math.ceil(estimatedWidth);
  if (!(num(el.height, 0) > 0)) el.height = Math.ceil(estimatedHeight);

  if (!el.containerId) {
    // Keep single-line/multi-line labels fully visible even when the source
    // JSON contains an inaccurate (too small) width.
    el.width = Math.max(el.width, Math.ceil(estimatedWidth));
    el.width = Math.min(el.width, 4000);
    el.height = Math.max(el.height, Math.ceil(estimatedHeight));
  }
  if (typeof el.baseline !== "number") {
    el.baseline = Math.round(el.fontSize * 0.8);
  }
}

function normalizeElement(raw) {
  if (!raw || typeof raw !== "object" || !KNOWN_TYPES.has(raw.type)) {
    return null;
  }
  const el = { ...raw };
  elementCounter += 1;
  if (elementCounter > 1_000_000) elementCounter = 1;

  el.id = str(el.id, `exv-${elementCounter}`);
  el.x = num(el.x, 0);
  el.y = num(el.y, 0);
  el.width = Math.max(0, num(el.width, 0));
  el.height = Math.max(0, num(el.height, 0));
  el.angle = num(el.angle, 0);
  el.strokeColor = str(el.strokeColor, "#1e1e1e");
  el.backgroundColor = str(el.backgroundColor, "transparent");
  el.fillStyle = str(el.fillStyle, "solid");
  el.strokeWidth = num(el.strokeWidth, 2);
  el.strokeStyle = str(el.strokeStyle, "solid");
  el.roughness = num(el.roughness, 1);
  el.opacity = num(el.opacity, 100);
  el.groupIds = Array.isArray(el.groupIds) ? el.groupIds : [];
  el.frameId = el.frameId ?? null;
  el.seed = num(el.seed, randomInt()) | 0;
  el.version = num(el.version, 1);
  el.versionNonce = num(el.versionNonce, randomInt()) | 0;
  el.updated = num(el.updated, Date.now());
  el.link = el.link ?? null;
  el.locked = Boolean(el.locked);
  el.isDeleted = Boolean(el.isDeleted);
  if (el.isDeleted) return null;
  el.boundElements = Array.isArray(el.boundElements) ? el.boundElements : [];

  if (el.type === "rectangle" || el.type === "ellipse" || el.type === "diamond") {
    if (el.width <= 0) el.width = 120;
    if (el.height <= 0) el.height = 80;
    if (el.roundness === undefined) {
      el.roundness =
        el.type === "rectangle" ? { type: 3 } : el.type === "diamond" ? { type: 2 } : null;
    }
  }

  if (el.type === "text") {
    normalizeText(el);
  }

  if (el.type === "line" || el.type === "arrow") {
    el.points = normalizePoints(el);
    el.width = Math.max(...el.points.map((point) => point[0]));
    el.height = Math.max(...el.points.map((point) => point[1]));
    el.lastCommittedPoint = null;
    el.startBinding = el.startBinding ?? null;
    el.endBinding = el.endBinding ?? null;
    el.startArrowhead = el.startArrowhead ?? null;
    if (el.endArrowhead === undefined) {
      el.endArrowhead = el.type === "arrow" ? "arrow" : null;
    }
    if (el.roundness === undefined) el.roundness = { type: 2 };
  }

  if (el.type === "freedraw") {
    el.points = normalizePoints(el);
    el.width = Math.max(...el.points.map((point) => point[0]));
    el.height = Math.max(...el.points.map((point) => point[1]));
    el.pressures = Array.isArray(el.pressures)
      ? el.pressures
      : new Array(el.points.length).fill(0.5);
    el.simulatePressure = el.simulatePressure ?? true;
    el.lastCommittedPoint = null;
  }

  if (el.type === "image") {
    el.scale = Array.isArray(el.scale) ? el.scale : [1, 1];
    el.status = "saved";
  }

  if (el.type === "frame") {
    el.name = str(el.name, "");
  }

  return el;
}

function normalizeElements(rawElements, files) {
  if (!Array.isArray(rawElements)) {
    throw new Error("invalid document: 'elements' must be an array");
  }
  const elements = rawElements
    .map(normalizeElement)
    .filter((el) => el !== null)
    .filter((el) => {
      // Images without embedded file data cannot be rendered by exportToSvg.
      if (el.type !== "image") return true;
      return Boolean(el.fileId && files && files[el.fileId]);
    });

  const byId = new Map(elements.map((el) => [el.id, el]));

  for (const el of elements) {
    if (el.type !== "text" || !el.containerId) continue;
    const container = byId.get(el.containerId);
    if (!container) {
      el.containerId = null;
      continue;
    }
    el.textAlign = "center";
    el.verticalAlign = "middle";
    el.width = Math.max(20, num(container.width, 0) - 20);
    el.height = Math.max(
      el.fontSize * el.lineHeight,
      num(container.height, 0) - 12,
    );
    el.x = container.x + (num(container.width, 0) - el.width) / 2;
    el.y = container.y + (num(container.height, 0) - el.height) / 2;
    container.boundElements = [
      ...container.boundElements.filter((bound) => bound?.id !== el.id),
      { id: el.id, type: "text" },
    ];
  }

  return elements;
}

// ---------------------------------------------------------------------------
// SVG post-processing
// ---------------------------------------------------------------------------

function normalizeSvgFonts(svg) {
  const stack = FONT_STACK.replace(/"/g, "'");
  return svg
    .replace(/font-family="[^"]*"/g, `font-family="${stack}"`)
    .replace(/font-family\s*:\s*[^;}]+/g, `font-family:${stack}`);
}

// jsdom's XMLSerializer can emit the same attribute twice on the root tag
// (notably xmlns), which is illegal XML for strict parsers such as usvg.
function dedupeRootAttributes(svg) {
  const tagStart = svg.indexOf("<svg");
  if (tagStart === -1) return svg;
  const tagEnd = svg.indexOf(">", tagStart);
  if (tagEnd === -1) return svg;

  const seen = new Set();
  const tag = svg
    .slice(tagStart, tagEnd)
    .replace(/([\w:.-]+)\s*=\s*("[^"]*"|'[^']*')/g, (match, name) => {
      const key = name.toLowerCase();
      if (seen.has(key)) return "";
      seen.add(key);
      return match;
    });

  return svg.slice(0, tagStart) + tag + svg.slice(tagEnd);
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

let exportScene = null;
let jsdomWindow = null;

async function loadExcalidraw() {
  installDomGlobals();
  jsdomWindow = globalThis.window;

  const module = await import("@excalidraw/utils");
  exportScene = module.exportScene;
  if (typeof exportScene !== "function") {
    throw new Error("@excalidraw/utils did not expose exportScene()");
  }
}

async function convertDocument(documentJson) {
  const doc = JSON.parse(documentJson);
  if (!doc || typeof doc !== "object" || Array.isArray(doc)) {
    throw new Error("invalid document: expected a JSON object");
  }
  const files = doc.files && typeof doc.files === "object" ? doc.files : null;
  const elements = normalizeElements(doc.elements, files);
  if (elements.length === 0) {
    throw new Error("no drawable elements found in the document");
  }

  const document = jsdomWindow.document;
  const environment = {
    document,
    createCanvas: () => document.createElement("canvas"),
    createImage: () => new jsdomWindow.Image(),
    createPath: (path) => new globalThis.Path2D(path),
    loadFonts: async () => {},
  };

  const svgElement = await exportScene({
    data: {
      elements,
      appState: {
        viewBackgroundColor: "#ffffff",
        gridSize: null,
        gridStep: null,
        gridModeEnabled: false,
      },
      files,
    },
    type: "svg",
    config: {
      theme: "light",
      canvasBackgroundColor: "#ffffff",
      padding: 12,
      scale: 1,
      skipInliningFonts: true,
      environment,
    },
  });
  if (!svgElement) {
    throw new Error("exportScene returned no SVG");
  }
  const serializer = new jsdomWindow.XMLSerializer();
  const serialized = serializer.serializeToString(svgElement);
  const svg = dedupeRootAttributes(normalizeSvgFonts(serialized));
  return { svg, elements: elements.length };
}

// ---------------------------------------------------------------------------
// Pipe protocol
// ---------------------------------------------------------------------------

let outputChain = Promise.resolve();
let shuttingDown = false;

function writeLineAndExit(payload, code) {
  const line = `${JSON.stringify(payload)}\n`;
  process.stdout.write(line, () => process.exit(code));
}

function failFatal(error) {
  if (shuttingDown) return;
  shuttingDown = true;
  const message = truncateError(error);
  writeLineAndExit({ fatal: message }, 1);
}

function truncateError(error) {
  const text =
    error && error.stack
      ? String(error.stack)
      : error && error.message
        ? String(error.message)
        : String(error);
  return text.length > MAX_ERROR_LENGTH
    ? `${text.slice(0, MAX_ERROR_LENGTH)}…`
    : text;
}

function send(payload) {
  const line = `${JSON.stringify(payload)}\n`;
  outputChain = outputChain.then(
    () =>
      new Promise((resolve) => {
        process.stdout.write(line, () => resolve());
      }),
  );
  return outputChain;
}

async function handleLine(line) {
  const trimmed = line.trim();
  if (!trimmed) return;

  let request;
  try {
    request = JSON.parse(trimmed);
  } catch {
    await send({ error: "invalid request: malformed JSON" });
    return;
  }

  const id = typeof request.id === "number" ? request.id : null;
  try {
    const json =
      typeof request.json === "string" ? request.json : JSON.stringify(request.json);
    const result = await convertDocument(json);
    await send({ id, svg: result.svg, elements: result.elements });
  } catch (error) {
    await send({ id, error: truncateError(error) });
  }
}

async function main() {
  try {
    await loadExcalidraw();
  } catch (error) {
    failFatal(error);
    return;
  }

  await send({ ready: true });

  const readline = createInterface({
    input: process.stdin,
    crlfDelay: Infinity,
    terminal: false,
  });

  let chain = Promise.resolve();
  readline.on("line", (line) => {
    chain = chain.then(() => handleLine(line)).catch(async (error) => {
      await send({ error: truncateError(error) });
    });
  });
  readline.on("close", () => {
    chain.finally(() => process.exit(0));
  });

  process.on("uncaughtException", failFatal);
  process.on("unhandledRejection", failFatal);
}

main();
