#!/usr/bin/env node

import {
  closeSync,
  constants,
  existsSync,
  fstatSync,
  linkSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  unlinkSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, posix, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { JSDOM } from "jsdom";
import ts from "typescript";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDirectory, "..");
const normalizedDate = new Date("2000-01-01T00:00:00.000Z");
const executableExtensions = new Set([".js", ".mjs", ".cjs"]);
const runtimeExtensions = new Set([
  ".html", ".htm", ".js", ".mjs", ".cjs", ".css", ".json", ".svg",
  ".png", ".jpg", ".jpeg", ".gif", ".webp", ".ico", ".avif",
  ".woff", ".woff2", ".ttf", ".otf",
]);
const textExtensions = new Set([".html", ".htm", ".js", ".mjs", ".cjs", ".css", ".json", ".svg"]);
const sensitiveSegment = /(?:^|[-_.])(credential|credentials|secret|secrets|token|auth|private|private-key|id_rsa|id_ecdsa|id_ed25519)(?:$|[-_.])/i;
const supportedManifestFields = new Set([
  "manifest_version", "name", "version", "version_name", "description", "short_name", "key", "default_locale",
  "author", "homepage_url", "minimum_chrome_version", "update_url", "offline_enabled", "incognito",
  "permissions", "optional_permissions", "host_permissions", "optional_host_permissions", "content_scripts",
  "web_accessible_resources", "background", "icons", "action", "options_page", "options_ui", "devtools_page",
  "side_panel", "chrome_url_overrides", "sandbox", "declarative_net_request", "storage", "commands",
  "content_security_policy", "cross_origin_embedder_policy", "cross_origin_opener_policy", "externally_connectable",
  "oauth2", "webview", "protocol_handlers", "file_handlers", "chrome_settings_overrides", "tts_engine",
]);

function fail(message) {
  throw new Error(message);
}

function hasControlCharacter(value) {
  return /[\0-\x1f\x7f]/.test(value);
}

function isRemoteUrl(value) {
  return /^(?:[a-z][a-z\d+.-]*:|\/\/)/i.test(value);
}

function chromeVersion(value, subject) {
  if (typeof value !== "string") fail(`${subject} must be a Chrome numeric dotted version`);
  const parts = value.split(".");
  if (parts.length < 1 || parts.length > 4 || !parts.every((part) => /^(?:0|[1-9]\d*)$/.test(part))) {
    fail(`${subject} must be a Chrome numeric dotted version`);
  }
  if (parts.some((part) => Number(part) > 65535) || parts.every((part) => part === "0")) {
    fail(`${subject} must be a Chrome numeric dotted version`);
  }
  return parts.join(".");
}

function cargoChromeVersion() {
  const cargoToml = readFileSync(resolve(repoRoot, "apps/cli/Cargo.toml"), "utf8");
  const packageSection = cargoToml.match(/^\[package\]\s*$([\s\S]*?)(?=^\[|(?![\s\S]))/m)?.[1] ?? "";
  const version = packageSection.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
  const cargoVersion = version?.match(/^(\d+\.\d+\.\d+)(?:\+[0-9A-Za-z.-]+)?$/)?.[1];
  if (!cargoVersion) fail("apps/cli/Cargo.toml version cannot be translated to a Chrome numeric dotted version");
  return chromeVersion(cargoVersion, "apps/cli/Cargo.toml version");
}

function isNativeBinary(bytes) {
  const header = bytes.subarray(0, 4);
  return (
    header.subarray(0, 2).equals(Buffer.from("MZ")) ||
    header.equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46])) ||
    header.equals(Buffer.from([0xfe, 0xed, 0xfa, 0xce])) ||
    header.equals(Buffer.from([0xfe, 0xed, 0xfa, 0xcf])) ||
    header.equals(Buffer.from([0xce, 0xfa, 0xed, 0xfe])) ||
    header.equals(Buffer.from([0xcf, 0xfa, 0xed, 0xfe])) ||
    header.equals(Buffer.from([0xca, 0xfe, 0xba, 0xbe])) ||
    header.equals(Buffer.from([0xbe, 0xba, 0xfe, 0xca]))
  );
}

function textContents(bytes, purpose) {
  if (bytes.includes(0)) fail(`${purpose} has an invalid binary payload`);
  try {
    return new TextDecoder("utf8", { fatal: true }).decode(bytes);
  } catch {
    fail(`${purpose} must be valid UTF-8 text`);
  }
}

function hasPrefix(bytes, values) {
  return values.every((value, index) => bytes[index] === value);
}

function validateRuntimeContents(relativePath, bytes, purpose) {
  const extension = posix.extname(relativePath).toLowerCase();
  if (textExtensions.has(extension)) {
    const text = textContents(bytes, purpose);
    if (extension === ".json") {
      try {
        JSON.parse(text);
      } catch {
        fail(`${purpose} must contain valid JSON: ${relativePath}`);
      }
    }
    return;
  }
  const signatures = {
    ".png": () => hasPrefix(bytes, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    ".jpg": () => hasPrefix(bytes, [0xff, 0xd8, 0xff]),
    ".jpeg": () => hasPrefix(bytes, [0xff, 0xd8, 0xff]),
    ".gif": () => bytes.subarray(0, 6).equals(Buffer.from("GIF87a")) || bytes.subarray(0, 6).equals(Buffer.from("GIF89a")),
    ".webp": () => bytes.subarray(0, 4).equals(Buffer.from("RIFF")) && bytes.subarray(8, 12).equals(Buffer.from("WEBP")),
    ".ico": () => hasPrefix(bytes, [0, 0, 1, 0]),
    ".avif": () => bytes.subarray(4, 8).equals(Buffer.from("ftyp")),
    ".woff": () => bytes.subarray(0, 4).equals(Buffer.from("wOFF")),
    ".woff2": () => bytes.subarray(0, 4).equals(Buffer.from("wOF2")),
    ".ttf": () => hasPrefix(bytes, [0, 1, 0, 0]),
    ".otf": () => bytes.subarray(0, 4).equals(Buffer.from("OTTO")),
  };
  if (!signatures[extension]?.()) fail(`${purpose} does not match its allowed ${extension} runtime format`);
}

function fileIdentity(stat) {
  return [stat.dev, stat.ino, stat.size, stat.mtimeMs, stat.ctimeMs].join(":");
}

function sameFile(stat, descriptorStat) {
  return stat.isFile() && descriptorStat.isFile() && stat.dev === descriptorStat.dev && stat.ino === descriptorStat.ino;
}

function localPath(referencedPath, purpose) {
  if (typeof referencedPath !== "string" || !referencedPath) fail(`${purpose} must be a non-empty local path`);
  if (hasControlCharacter(referencedPath)) fail(`${purpose} must not contain a control character`);
  if (isRemoteUrl(referencedPath)) fail(`${purpose} must not use a remote URL: ${referencedPath}`);
  if (referencedPath.includes("\\") || referencedPath.startsWith("/")) fail(`${purpose} is outside the extension: ${referencedPath}`);
  const segments = referencedPath.split("/");
  if (segments.some((segment) => segment === ".." || segment === "" || segment === ".")) {
    fail(`${purpose} must not traverse outside the extension: ${referencedPath}`);
  }
  const relativePath = posix.normalize(referencedPath);
  if (
    relativePath !== "manifest.json" && relativePath !== "popup.html" && relativePath !== "prompt.html" &&
    !relativePath.startsWith("dist/") && !relativePath.startsWith("public/")
  ) fail(`${purpose} is not in the allowed roots: ${referencedPath}`);
  if (relativePath.split("/").some((segment) => segment.startsWith("."))) fail(`${purpose} must not include hidden files: ${referencedPath}`);
  const extension = posix.extname(relativePath).toLowerCase();
  if (!runtimeExtensions.has(extension)) {
    fail(`${purpose} is not a permitted Store runtime file: ${referencedPath}`);
  }
  if (relativePath.split("/").some((segment) => sensitiveSegment.test(segment))) fail(`${purpose} has a sensitive path segment: ${referencedPath}`);
  return relativePath;
}

function snapshotFile(extensionRoot, relativePath, purpose) {
  const path = resolve(extensionRoot, relativePath);
  if (!path.startsWith(`${extensionRoot}${sep}`)) fail(`${purpose} is outside the extension: ${relativePath}`);
  let canonicalPath;
  try {
    canonicalPath = realpathSync(path);
  } catch {
    fail(`${purpose} is missing: ${relativePath}`);
  }
  if (!canonicalPath.startsWith(`${extensionRoot}${sep}`)) fail(`${purpose} is outside the canonical extension root: ${relativePath}`);
  let current = extensionRoot;
  for (const segment of relativePath.split("/")) {
    current = resolve(current, segment);
    if (!existsSync(current)) fail(`${purpose} is missing: ${relativePath}`);
    const stat = lstatSync(current);
    if (stat.isSymbolicLink()) fail(`${purpose} must not be a symlink: ${relativePath}`);
  }
  let descriptor;
  try {
    const observed = lstatSync(canonicalPath);
    if (observed.isSymbolicLink() || !observed.isFile()) fail(`${purpose} must be a regular non-symlink file: ${relativePath}`);
    descriptor = openSync(canonicalPath, constants.O_RDONLY | constants.O_NOFOLLOW);
    const before = fstatSync(descriptor);
    if (!sameFile(observed, before)) fail(`${purpose} changed before it could be opened: ${relativePath}`);
    const bytes = readFileSync(descriptor);
    const after = fstatSync(descriptor);
    if (fileIdentity(before) !== fileIdentity(after)) fail(`${purpose} changed while being read: ${relativePath}`);
    if (isNativeBinary(bytes)) fail(`${purpose} must not be a native binary: ${relativePath}`);
    validateRuntimeContents(relativePath, bytes, purpose);
    return { relativePath, bytes };
  } catch (error) {
    if (error?.code === "ELOOP") fail(`${purpose} must not be a symlink: ${relativePath}`);
    throw error;
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
  }
}

function runtimePatternFiles(extensionRoot, pattern, purpose) {
  if (typeof pattern !== "string" || !pattern) fail(`${purpose} must be a non-empty local path pattern`);
  if (hasControlCharacter(pattern)) fail(`${purpose} must not contain a control character`);
  if (isRemoteUrl(pattern) || pattern.includes("\\") || pattern.startsWith("/")) fail(`${purpose} must use an allowed local path pattern: ${pattern}`);
  const segments = pattern.split("/");
  if (segments.some((segment) => segment === ".." || segment === "" || segment === ".")) fail(`${purpose} must not traverse outside the extension: ${pattern}`);
  if (segments[0] !== "dist" && segments[0] !== "public") fail(`${purpose} is not in the allowed roots: ${pattern}`);
  const matcher = new RegExp(`^${pattern.split(/([*?])/).map((part) => {
    if (part === "*") return "[^/]*";
    if (part === "?") return "[^/]";
    return part.replace(/[|\\{}()[\]^$+?.]/g, "\\$&");
  }).join("")}$`);
  const files = [];
  const visit = (directory, prefix) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const child = `${prefix}/${entry.name}`;
      if (entry.isDirectory()) visit(resolve(directory, entry.name), child);
      else if ((entry.isFile() || entry.isSymbolicLink()) && matcher.test(child)) files.push(child);
    }
  };
  const root = resolve(extensionRoot, segments[0]);
  if (!existsSync(root) || !lstatSync(root).isDirectory()) fail(`${purpose} does not match a runtime file: ${pattern}`);
  visit(root, segments[0]);
  if (files.length === 0) fail(`${purpose} does not match a runtime file: ${pattern}`);
  return files.sort();
}

function addIconPaths(add, value, purpose) {
  if (value === undefined) return;
  if (typeof value === "string") return add(value, purpose);
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${purpose} must be a path or icon map`);
  for (const [size, path] of Object.entries(value)) add(path, `${purpose} ${size}`);
}

function array(value, purpose) {
  if (!Array.isArray(value)) fail(`${purpose} must be an array`);
  return value;
}

function object(value, purpose) {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${purpose} must be an object`);
  return value;
}

function optionalPath(add, value, purpose) {
  if (value !== undefined) add(value, purpose);
}

function validateKeys(value, keys, purpose) {
  for (const key of Object.keys(value)) {
    if (!keys.has(key)) fail(`Unrecognized ${purpose} field: ${key}`);
  }
}

function validateStringArray(value, purpose) {
  for (const item of array(value, purpose)) {
    if (typeof item !== "string") fail(`${purpose} must contain strings`);
  }
}

function validateManifestSchema(manifest) {
  object(manifest, "manifest");
  validateKeys(manifest, supportedManifestFields, "manifest");
  for (const field of ["permissions", "optional_permissions", "host_permissions", "optional_host_permissions"]) {
    if (manifest[field] !== undefined) validateStringArray(manifest[field], field);
  }
  if (manifest.background !== undefined) validateKeys(object(manifest.background, "background"), new Set(["service_worker", "type"]), "background");
  if (manifest.action !== undefined) validateKeys(object(manifest.action, "action"), new Set(["default_icon", "default_popup", "default_title", "default_badge_text", "default_badge_color", "default_popup_height", "default_popup_width"]), "action");
  if (manifest.options_ui !== undefined) validateKeys(object(manifest.options_ui, "options UI"), new Set(["page", "open_in_tab"]), "options UI");
  if (manifest.side_panel !== undefined) validateKeys(object(manifest.side_panel, "side panel"), new Set(["default_path"]), "side panel");
  if (manifest.sandbox !== undefined) validateKeys(object(manifest.sandbox, "sandbox"), new Set(["pages", "content_security_policy"]), "sandbox");
  if (manifest.storage !== undefined) validateKeys(object(manifest.storage, "storage"), new Set(["managed_schema"]), "storage");
  if (manifest.declarative_net_request !== undefined) validateKeys(object(manifest.declarative_net_request, "declarative net request"), new Set(["rule_resources"]), "declarative net request");
}

function manifestPaths(extensionRoot, manifest, add) {
  add("popup.html", "required popup HTML");
  add("prompt.html", "required prompt HTML");
  if (manifest.background !== undefined) optionalPath(add, object(manifest.background, "background").service_worker, "background service worker");
  if (manifest.content_scripts !== undefined) {
    for (const [index, contentScript] of array(manifest.content_scripts, "content scripts").entries()) {
      const script = object(contentScript, `content script ${index}`);
      for (const [pathIndex, path] of (script.js === undefined ? [] : array(script.js, `content script ${index} JavaScript`)).entries()) add(path, `content script ${index} JavaScript ${pathIndex}`);
      for (const [pathIndex, path] of (script.css === undefined ? [] : array(script.css, `content script ${index} stylesheet`)).entries()) add(path, `content script ${index} stylesheet ${pathIndex}`);
    }
  }
  addIconPaths(add, manifest.icons, "extension icon");
  if (manifest.action !== undefined) {
    const action = object(manifest.action, "action");
    optionalPath(add, action.default_popup, "action popup");
    addIconPaths(add, action.default_icon, "action icon");
  }
  optionalPath(add, manifest.options_page, "options page");
  if (manifest.options_ui !== undefined) optionalPath(add, object(manifest.options_ui, "options UI").page, "options UI page");
  optionalPath(add, manifest.devtools_page, "DevTools page");
  if (manifest.side_panel !== undefined) optionalPath(add, object(manifest.side_panel, "side panel").default_path, "side panel");
  if (manifest.chrome_url_overrides !== undefined) {
    for (const [name, path] of Object.entries(object(manifest.chrome_url_overrides, "chrome URL overrides"))) add(path, `chrome URL override ${name}`);
  }
  if (manifest.sandbox !== undefined) {
    const sandbox = object(manifest.sandbox, "sandbox");
    for (const [index, path] of (sandbox.pages === undefined ? [] : array(sandbox.pages, "sandbox pages")).entries()) add(path, `sandbox page ${index}`);
  }
  if (manifest.web_accessible_resources !== undefined) {
    for (const [index, resource] of array(manifest.web_accessible_resources, "web accessible resources").entries()) {
      const paths = object(resource, `web accessible resource ${index}`).resources;
      for (const [pathIndex, path] of array(paths, `web accessible resource ${index} paths`).entries()) {
        if (typeof path === "string" && (path.includes("*") || path.includes("?"))) {
          for (const resolvedPath of runtimePatternFiles(extensionRoot, path, `web accessible resource ${index}:${pathIndex}`)) add(resolvedPath, `web accessible resource ${index}:${pathIndex}`);
        } else add(path, `web accessible resource ${index}:${pathIndex}`);
      }
    }
  }
  if (manifest.declarative_net_request !== undefined) {
    const dnr = object(manifest.declarative_net_request, "declarative net request");
    if (dnr.rule_resources !== undefined) {
      for (const [index, resource] of array(dnr.rule_resources, "declarative net request rule resources").entries()) {
        add(object(resource, `declarative net request rule resource ${index}`).path, `declarative net request rule resource ${index} path`);
      }
    }
  }
  if (manifest.storage !== undefined) optionalPath(add, object(manifest.storage, "storage").managed_schema, "managed storage schema");
}

function resolvedUrlPath(parent, reference, purpose, executable) {
  if (typeof reference !== "string" || !reference) fail(`${purpose} must be a non-empty URL`);
  if (hasControlCharacter(reference)) fail(`${purpose} must not contain a control character`);
  let url;
  try {
    url = new URL(reference, `chrome-extension://termkey/${parent}`);
  } catch {
    fail(`${purpose} must be a valid local URL: ${reference}`);
  }
  if (url.protocol !== "chrome-extension:" || url.hostname !== "termkey") {
    if (executable) fail(`${purpose} must not use a remote URL: ${reference}`);
    return undefined;
  }
  const path = decodeURIComponent(url.pathname).replace(/^\/+/, "");
  if (hasControlCharacter(path)) fail(`${purpose} must not contain a control character`);
  return path;
}

function htmlReferences(snapshot) {
  const document = new JSDOM(snapshot.bytes.toString("utf8")).window.document;
  const references = [];
  for (const script of document.querySelectorAll("script")) {
    const type = script.type.trim().toLowerCase();
    const executable = !type || type === "module" || /(?:java|ecma)script/.test(type);
    if (!executable) continue;
    if (!script.hasAttribute("src")) fail(`Inline executable script is not allowed in ${snapshot.relativePath}`);
    references.push({ value: script.getAttribute("src"), purpose: "HTML script", executable: true });
  }
  for (const link of document.querySelectorAll("link[href]")) {
    if (link.relList.contains("modulepreload")) references.push({ value: link.getAttribute("href"), purpose: "module preload", executable: true });
    else if (link.relList.contains("stylesheet")) references.push({ value: link.getAttribute("href"), purpose: "stylesheet", executable: false });
  }
  for (const image of document.querySelectorAll("img[src]")) references.push({ value: image.getAttribute("src"), purpose: "image", executable: false });
  return references;
}

function javascriptReferences(snapshot) {
  const references = [];
  const source = ts.createSourceFile(snapshot.relativePath, snapshot.bytes.toString("utf8"), ts.ScriptTarget.Latest, true);
  const staticValue = (node, purpose) => {
    if (!node || !ts.isStringLiteralLike(node)) fail(`${purpose} must use a string literal in ${snapshot.relativePath}`);
    return node.text;
  };
  const visit = (node) => {
    const importScriptsReference =
      (ts.isIdentifier(node) && node.text === "importScripts") ||
      (ts.isPropertyAccessExpression(node) && node.name.text === "importScripts") ||
      (ts.isElementAccessExpression(node) && ts.isStringLiteralLike(node.argumentExpression) && node.argumentExpression.text === "importScripts");
    if (importScriptsReference) fail(`importScripts is not permitted in ${snapshot.relativePath}`);
    if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier) references.push({ value: staticValue(node.moduleSpecifier, "module specifier"), purpose: "JavaScript import" });
    if (ts.isCallExpression(node)) {
      if (node.expression.kind === ts.SyntaxKind.ImportKeyword) references.push({ value: staticValue(node.arguments[0], "dynamic import"), purpose: "dynamic import" });
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return references;
}

function cssReferences(snapshot) {
  const text = textContents(snapshot.bytes, `stylesheet ${snapshot.relativePath}`);
  const references = new Set();
  for (const match of text.matchAll(/@import\s+(?:url\(\s*)?["']?([^"'()\s;]+)["']?\s*\)?/gi)) references.add(match[1]);
  for (const match of text.matchAll(/url\(\s*["']?([^"'()\s]+)["']?\s*\)/gi)) references.add(match[1]);
  return [...references];
}

function collectRuntimeFiles(extensionRoot, manifestSnapshot) {
  const snapshots = new Map([["manifest.json", manifestSnapshot]]);
  const pending = [];
  const add = (value, purpose) => {
    const relativePath = localPath(value, purpose);
    if (!snapshots.has(relativePath)) {
      const snapshot = snapshotFile(extensionRoot, relativePath, purpose);
      snapshots.set(relativePath, snapshot);
      pending.push(snapshot);
    }
  };
  const manifest = JSON.parse(manifestSnapshot.bytes.toString("utf8"));
  manifestPaths(extensionRoot, manifest, add);
  while (pending.length > 0) {
    const snapshot = pending.shift();
    const extension = posix.extname(snapshot.relativePath).toLowerCase();
    if (extension === ".html") {
      for (const reference of htmlReferences(snapshot)) {
        const path = resolvedUrlPath(snapshot.relativePath, reference.value, reference.purpose, reference.executable);
        if (path) add(path, reference.purpose);
      }
    } else if (executableExtensions.has(extension)) {
      for (const reference of javascriptReferences(snapshot)) {
        if (isRemoteUrl(reference.value)) fail(`${reference.purpose} must not use a remote URL: ${reference.value}`);
        if (!reference.value.startsWith(".") && !reference.value.startsWith("/")) fail(`${reference.purpose} must use a relative extension path: ${reference.value}`);
        add(resolvedUrlPath(snapshot.relativePath, reference.value, reference.purpose, true), reference.purpose);
      }
    } else if (extension === ".css") {
      for (const reference of cssReferences(snapshot)) {
        const path = resolvedUrlPath(snapshot.relativePath, reference, "stylesheet runtime reference", false);
        if (path) add(path, "stylesheet runtime reference");
      }
    }
  }
  return [...snapshots.values()].sort((left, right) => left.relativePath.localeCompare(right.relativePath));
}

function stageFiles(staging, snapshots) {
  for (const snapshot of snapshots) {
    const destination = resolve(staging, snapshot.relativePath);
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, snapshot.bytes, { mode: 0o644 });
    utimesSync(destination, normalizedDate, normalizedDate);
  }
}

function createArchive(staging, paths) {
  const stagedArchive = resolve(staging, "archive.zip");
  const zip = spawnSync("/usr/bin/zip", ["-X", "-q", stagedArchive, "-@"], {
    cwd: staging,
    encoding: "utf8",
    env: { ...process.env, TZ: "UTC" },
    input: `${paths.join("\n")}\n`,
  });
  if (zip.status !== 0) fail(`zip failed: ${zip.stderr || zip.stdout}`);
  return stagedArchive;
}

export function packageExtension(extensionDirectory, outputArchive) {
  if (!extensionDirectory || !outputArchive) fail("Usage: node scripts/package-extension.mjs <extension_dir> <output_zip>");
  const output = resolve(outputArchive);
  if (existsSync(output)) fail(`Refusing to overwrite existing output archive: ${output}`);
  const extensionRoot = realpathSync(resolve(extensionDirectory));
  if (!lstatSync(extensionRoot).isDirectory()) fail(`Extension directory is not a directory: ${extensionDirectory}`);
  const manifestSnapshot = snapshotFile(extensionRoot, "manifest.json", "Extension manifest.json");
  const manifest = JSON.parse(manifestSnapshot.bytes.toString("utf8"));
  validateManifestSchema(manifest);
  if (manifest.manifest_version !== 3) fail("Extension manifest must be Manifest V3");
  const expectedVersion = cargoChromeVersion();
  if (chromeVersion(manifest.version, "manifest version") !== expectedVersion) fail(`Extension manifest version ${manifest.version} does not match Cargo version ${expectedVersion}`);
  const snapshots = collectRuntimeFiles(extensionRoot, manifestSnapshot);
  const outputDirectory = dirname(output);
  mkdirSync(outputDirectory, { recursive: true });
  let staging;
  try {
    staging = mkdtempSync(resolve(outputDirectory, ".termkey-store-package-"));
    stageFiles(staging, snapshots);
    const stagedArchive = createArchive(staging, snapshots.map((snapshot) => snapshot.relativePath));
    linkSync(stagedArchive, output);
    unlinkSync(stagedArchive);
  } finally {
    if (staging) rmSync(staging, { recursive: true, force: true });
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    packageExtension(process.argv[2], process.argv[3]);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
