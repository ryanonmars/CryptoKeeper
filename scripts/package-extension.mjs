#!/usr/bin/env node

import {
  constants,
  chmodSync,
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  utimesSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, posix, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDirectory, "..");
const normalizedDate = new Date("2000-01-01T00:00:00.000Z");
const executableExtensions = new Set([".js", ".mjs", ".cjs"]);
const prohibitedExtensions = new Set([
  ".env",
  ".map",
  ".md",
  ".markdown",
  ".ts",
  ".tsx",
  ".cts",
  ".mts",
  ".node",
  ".dylib",
  ".dll",
  ".exe",
  ".so",
  ".pem",
  ".key",
  ".p12",
  ".pfx",
]);

function fail(message) {
  throw new Error(message);
}

function isRemoteUrl(value) {
  return /^(?:[a-z][a-z\d+.-]*:|\/\/)/i.test(value);
}

function chromeVersion(value, subject) {
  if (typeof value !== "string") {
    fail(`${subject} must be a Chrome numeric dotted version`);
  }
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
  if (!cargoVersion) {
    fail("apps/cli/Cargo.toml version cannot be translated to a Chrome numeric dotted version");
  }
  return chromeVersion(cargoVersion, "apps/cli/Cargo.toml version");
}

function isNativeBinary(path) {
  const header = readFileSync(path).subarray(0, 4);
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

function localPath(extensionRoot, referencedPath, purpose) {
  if (typeof referencedPath !== "string" || !referencedPath) {
    fail(`${purpose} must be a non-empty local path`);
  }
  if (isRemoteUrl(referencedPath)) {
    fail(`${purpose} must not use a remote URL: ${referencedPath}`);
  }
  if (referencedPath.includes("\\") || referencedPath.startsWith("/")) {
    fail(`${purpose} is outside the extension: ${referencedPath}`);
  }
  const segments = referencedPath.split("/");
  if (segments.some((segment) => segment === ".." || segment === "" || segment === ".")) {
    fail(`${purpose} must not traverse outside the extension: ${referencedPath}`);
  }
  const relativePath = posix.normalize(referencedPath);
  if (
    relativePath !== "manifest.json" &&
    relativePath !== "popup.html" &&
    relativePath !== "prompt.html" &&
    !relativePath.startsWith("dist/") &&
    !relativePath.startsWith("public/")
  ) {
    fail(`${purpose} is not in the allowed roots: ${referencedPath}`);
  }
  if (relativePath.split("/").some((segment) => segment.startsWith("."))) {
    fail(`${purpose} must not include hidden files: ${referencedPath}`);
  }
  if (prohibitedExtensions.has(posix.extname(relativePath).toLowerCase())) {
    fail(`${purpose} is not a permitted Store runtime file: ${referencedPath}`);
  }

  const path = resolve(extensionRoot, relativePath);
  const extensionPrefix = `${extensionRoot}${sep}`;
  if (path !== extensionRoot && !path.startsWith(extensionPrefix)) {
    fail(`${purpose} is outside the extension: ${referencedPath}`);
  }
  let current = extensionRoot;
  let stat;
  for (const segment of relativePath.split("/")) {
    current = resolve(current, segment);
    if (!existsSync(current)) {
      fail(`${purpose} is missing: ${referencedPath}`);
    }
    stat = lstatSync(current);
    if (stat.isSymbolicLink()) {
      fail(`${purpose} must not be a symlink: ${referencedPath}`);
    }
  }
  if (!stat?.isFile()) {
    fail(`${purpose} must be a regular file: ${referencedPath}`);
  }
  if (isNativeBinary(path)) {
    fail(`${purpose} must not be a native binary: ${referencedPath}`);
  }
  return relativePath;
}

function runtimePatternFiles(extensionRoot, pattern, purpose) {
  if (typeof pattern !== "string" || !pattern) {
    fail(`${purpose} must be a non-empty local path pattern`);
  }
  if (isRemoteUrl(pattern) || pattern.includes("\\") || pattern.startsWith("/")) {
    fail(`${purpose} must not use a remote or absolute path pattern: ${pattern}`);
  }
  const segments = pattern.split("/");
  if (segments.some((segment) => segment === ".." || segment === "" || segment === ".")) {
    fail(`${purpose} must not traverse outside the extension: ${pattern}`);
  }
  if (segments[0] !== "dist" && segments[0] !== "public") {
    fail(`${purpose} is not in the allowed roots: ${pattern}`);
  }
  const matcher = new RegExp(`^${pattern
    .split(/([*?])/)
    .map((part) => {
      if (part === "*") return "[^/]*";
      if (part === "?") return "[^/]";
      return part.replace(/[|\\{}()[\]^$+?.]/g, "\\$&");
    })
    .join("")}$`);
  const files = [];
  const visit = (directory, prefix = "") => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const relativePath = prefix ? `${prefix}/${entry.name}` : entry.name;
      const absolutePath = resolve(directory, entry.name);
      if (entry.isDirectory()) {
        visit(absolutePath, relativePath);
      } else if ((entry.isFile() || entry.isSymbolicLink()) && matcher.test(relativePath)) {
        files.push(relativePath);
      }
    }
  };
  visit(resolve(extensionRoot, segments[0]), segments[0]);
  if (files.length === 0) {
    fail(`${purpose} does not match a runtime file: ${pattern}`);
  }
  return files.sort();
}

function addIconPaths(add, icons, purpose) {
  if (typeof icons === "string") {
    add(icons, purpose);
  } else if (icons && typeof icons === "object") {
    for (const [size, path] of Object.entries(icons)) {
      add(path, `${purpose} ${size}`);
    }
  }
}

function manifestPaths(extensionRoot, manifest, add) {
  add("popup.html", "required popup HTML");
  add("prompt.html", "required prompt HTML");
  add(manifest.background?.service_worker, "background service worker");
  for (const [index, contentScript] of (manifest.content_scripts ?? []).entries()) {
    for (const [scriptIndex, path] of (contentScript.js ?? []).entries()) {
      add(path, `content script ${index} JavaScript ${scriptIndex}`);
    }
    for (const [styleIndex, path] of (contentScript.css ?? []).entries()) {
      add(path, `content script ${index} stylesheet ${styleIndex}`);
    }
  }
  addIconPaths(add, manifest.icons, "extension icon");
  add(manifest.action?.default_popup, "action popup");
  addIconPaths(add, manifest.action?.default_icon, "action icon");
  add(manifest.options_page, "options page");
  add(manifest.options_ui?.page, "options UI page");
  add(manifest.devtools_page, "DevTools page");
  add(manifest.side_panel?.default_path, "side panel");
  add(manifest.theme, "theme");
  for (const [name, override] of Object.entries(manifest.chrome_url_overrides ?? {})) {
    add(override, `chrome URL override ${name}`);
  }
  for (const [index, page] of (manifest.sandbox?.pages ?? []).entries()) {
    add(page, `sandbox page ${index}`);
  }
  for (const [index, resource] of (manifest.web_accessible_resources ?? []).entries()) {
    const paths = typeof resource === "string" ? [resource] : resource?.resources;
    for (const [resourceIndex, path] of (paths ?? []).entries()) {
      if (path.includes("*") || path.includes("?")) {
        for (const resolvedPath of runtimePatternFiles(
          extensionRoot,
          path,
          `web accessible resource ${index}:${resourceIndex}`,
        )) {
          add(resolvedPath, `web accessible resource ${index}:${resourceIndex}`);
        }
        continue;
      }
      add(path, `web accessible resource ${index}:${resourceIndex}`);
    }
  }
}

function referencesFromHtml(contents) {
  const references = [];
  const attributePattern = /<(script|link|img)\b[^>]*?\b(src|href)\s*=\s*["']([^"']+)["'][^>]*>/gi;
  for (const match of contents.matchAll(attributePattern)) {
    const tag = match[1].toLowerCase();
    const value = match[3];
    if (tag === "script" || tag === "img" || /\brel\s*=\s*["']?stylesheet/i.test(match[0])) {
      references.push({ value, executable: tag === "script" });
    }
  }
  return references;
}

function referencesFromJavaScript(contents) {
  const references = [];
  const staticPattern = /\b(?:import|export)\s+(?:[^"'`]*?\s+from\s+)?["']([^"']+)["']/g;
  const dynamicPattern = /\bimport\s*\(\s*["']([^"']+)["']\s*\)/g;
  for (const pattern of [staticPattern, dynamicPattern]) {
    for (const match of contents.matchAll(pattern)) {
      references.push(match[1]);
    }
  }
  return references;
}

function referencesFromCss(contents) {
  const references = [];
  const pattern = /(?:@import\s+(?:url\()?|url\()\s*["']?([^"')\s]+)["']?\s*\)?/gi;
  for (const match of contents.matchAll(pattern)) {
    references.push(match[1]);
  }
  return references;
}

function resolveRelativeReference(parent, reference, purpose) {
  if (isRemoteUrl(reference)) {
    fail(`${purpose} must not use a remote URL: ${reference}`);
  }
  return reference.startsWith(".")
    ? posix.normalize(posix.join(posix.dirname(parent), reference))
    : reference;
}

function collectRuntimeFiles(extensionRoot, manifest) {
  const files = new Set(["manifest.json"]);
  const pending = [];
  const add = (path, purpose) => {
    if (path === undefined) {
      return;
    }
    const local = localPath(extensionRoot, path, purpose);
    if (!files.has(local)) {
      files.add(local);
      pending.push(local);
    }
  };

  manifestPaths(extensionRoot, manifest, add);
  while (pending.length > 0) {
    const path = pending.shift();
    const contents = readFileSync(resolve(extensionRoot, path), "utf8");
    const extension = posix.extname(path).toLowerCase();
    if (extension === ".html") {
      for (const { value, executable } of referencesFromHtml(contents)) {
        if (isRemoteUrl(value)) {
          if (executable) {
            fail(`HTML script must not use a remote URL: ${value}`);
          }
          continue;
        }
        add(resolveRelativeReference(path, value, "HTML runtime reference"), "HTML runtime reference");
      }
    } else if (executableExtensions.has(extension)) {
      for (const reference of referencesFromJavaScript(contents)) {
        add(resolveRelativeReference(path, reference, "JavaScript import"), "JavaScript import");
      }
    } else if (extension === ".css") {
      for (const reference of referencesFromCss(contents)) {
        if (isRemoteUrl(reference)) {
          continue;
        }
        add(resolveRelativeReference(path, reference, "stylesheet runtime reference"), "stylesheet runtime reference");
      }
    }
  }
  return [...files].sort();
}

function stageFiles(extensionRoot, paths) {
  const staging = mkdtempSync(resolve(tmpdir(), "termkey-store-package-"));
  for (const path of paths) {
    const source = resolve(extensionRoot, path);
    const destination = resolve(staging, path);
    mkdirSync(dirname(destination), { recursive: true });
    copyFileSync(source, destination, constants.COPYFILE_FICLONE);
    chmodSync(destination, 0o644);
    utimesSync(destination, normalizedDate, normalizedDate);
  }
  return staging;
}

function createArchive(staging, paths, output) {
  const stagedArchive = resolve(staging, "archive.zip");
  const zip = spawnSync(
    "/usr/bin/zip",
    ["-X", "-q", stagedArchive, "-@"],
    {
      cwd: staging,
      encoding: "utf8",
      env: { ...process.env, TZ: "UTC" },
      input: `${paths.join("\n")}\n`,
    },
  );
  if (zip.status !== 0) {
    fail(`zip failed: ${zip.stderr || zip.stdout}`);
  }
  mkdirSync(dirname(output), { recursive: true });
  copyFileSync(stagedArchive, output, constants.COPYFILE_EXCL);
}

export function packageExtension(extensionDirectory, outputArchive) {
  if (!extensionDirectory || !outputArchive) {
    fail("Usage: node scripts/package-extension.mjs <extension_dir> <output_zip>");
  }
  const output = resolve(outputArchive);
  if (existsSync(output)) {
    fail(`Refusing to overwrite existing output archive: ${output}`);
  }
  const extensionRoot = realpathSync(resolve(extensionDirectory));
  if (!lstatSync(extensionRoot).isDirectory()) {
    fail(`Extension directory is not a directory: ${extensionDirectory}`);
  }
  const manifestPath = resolve(extensionRoot, "manifest.json");
  if (!existsSync(manifestPath)) {
    fail(`Extension manifest.json is missing: ${manifestPath}`);
  }
  const manifestStat = lstatSync(manifestPath);
  if (manifestStat.isSymbolicLink() || !manifestStat.isFile()) {
    fail("Extension manifest.json must be a regular non-symlink file");
  }
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  if (manifest.manifest_version !== 3) {
    fail("Extension manifest must be Manifest V3");
  }
  const expectedVersion = cargoChromeVersion();
  if (chromeVersion(manifest.version, "manifest version") !== expectedVersion) {
    fail(`Extension manifest version ${manifest.version} does not match Cargo version ${expectedVersion}`);
  }

  const paths = collectRuntimeFiles(extensionRoot, manifest);
  let staging;
  try {
    staging = stageFiles(extensionRoot, paths);
    createArchive(staging, paths, output);
  } finally {
    if (staging) {
      rmSync(staging, { recursive: true, force: true });
    }
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
