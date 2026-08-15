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
const runtimeExtensions = new Set([".html", ".js", ".mjs", ".cjs", ".css", ".json", ".png"]);
const textExtensions = new Set([".html", ".js", ".mjs", ".cjs", ".css", ".json"]);
const sensitiveSegment = /(?:^|[-_.])(credential|credentials|secret|secrets|token|auth|private|private-key|id_rsa|id_ecdsa|id_ed25519)(?:$|[-_.])/i;
const supportedManifestFields = new Set([
  "manifest_version", "name", "version", "version_name", "description", "short_name", "key",
  "author", "homepage_url", "minimum_chrome_version", "update_url", "offline_enabled", "incognito",
  "permissions", "optional_permissions", "host_permissions", "optional_host_permissions", "content_scripts",
  "web_accessible_resources", "background", "icons", "action", "options_page", "options_ui", "devtools_page",
  "side_panel", "chrome_url_overrides", "sandbox", "declarative_net_request", "storage",
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

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function validatePng(bytes, purpose) {
  if (!hasPrefix(bytes, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])) fail(`${purpose} does not match its allowed .png runtime format`);
  let offset = 8;
  let hasIhdr = false;
  let hasIdat = false;
  while (offset < bytes.length) {
    if (offset + 12 > bytes.length) fail(`${purpose} has a truncated PNG chunk`);
    const length = bytes.readUInt32BE(offset);
    const typeStart = offset + 4;
    const dataStart = offset + 8;
    const end = dataStart + length;
    if (!Number.isSafeInteger(end) || end + 4 > bytes.length) fail(`${purpose} has an oversized PNG chunk`);
    const type = bytes.subarray(typeStart, dataStart).toString("ascii");
    if (!/^[A-Za-z]{4}$/.test(type)) fail(`${purpose} has an invalid PNG chunk type`);
    if (bytes.readUInt32BE(end) !== crc32(bytes.subarray(typeStart, end))) fail(`${purpose} has an invalid PNG chunk CRC`);
    if (!hasIhdr) {
      if (type !== "IHDR" || length !== 13) fail(`${purpose} must begin with one PNG IHDR chunk`);
      if (bytes.readUInt32BE(dataStart) === 0 || bytes.readUInt32BE(dataStart + 4) === 0) fail(`${purpose} has an invalid PNG IHDR size`);
      hasIhdr = true;
    }
    if (type === "IDAT") hasIdat = true;
    offset = end + 4;
    if (type === "IEND") {
      if (length !== 0 || !hasIdat || offset !== bytes.length) fail(`${purpose} has an incomplete PNG payload`);
      return;
    }
  }
  fail(`${purpose} is missing PNG IEND`);
}

function validateRuntimeContents(relativePath, bytes, purpose) {
  const extension = posix.extname(relativePath).toLowerCase();
  if (textExtensions.has(extension)) {
    const text = textContents(bytes, purpose);
    if (extension === ".json") {
      if (!/(declarative net request rule resource|managed storage schema|Extension manifest\.json)/.test(purpose)) {
        fail(`${purpose} cannot include arbitrary JSON: ${relativePath}`);
      }
      try {
        JSON.parse(text);
      } catch {
        fail(`${purpose} must contain valid JSON: ${relativePath}`);
      }
    }
    return;
  }
  if (extension === ".png") return validatePng(bytes, purpose);
  fail(`${purpose} is not a permitted Store runtime file: ${relativePath}`);
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

function snapshotFile(extensionRoot, relativePath, purpose, hooks = {}) {
  const path = resolve(extensionRoot, relativePath);
  if (!path.startsWith(`${extensionRoot}${sep}`)) fail(`${purpose} is outside the extension: ${relativePath}`);
  const canonicalRoot = realpathSync(extensionRoot);
  let canonicalPath;
  try {
    canonicalPath = realpathSync(path);
  } catch {
    fail(`${purpose} is missing: ${relativePath}`);
  }
  if (!canonicalPath.startsWith(`${canonicalRoot}${sep}`)) fail(`${purpose} is outside the canonical extension root: ${relativePath}`);
  const descriptors = [];
  try {
    const observed = lstatSync(canonicalPath);
    if (observed.isSymbolicLink() || !observed.isFile()) fail(`${purpose} must be a regular non-symlink file: ${relativePath}`);
    const rootDescriptor = openSync(canonicalRoot, constants.O_RDONLY | constants.O_DIRECTORY | constants.O_NOFOLLOW);
    descriptors.push(rootDescriptor);
    if (!fstatSync(rootDescriptor).isDirectory()) fail(`${purpose} extension root is not a directory`);
    hooks.onRootOpened?.({ relativePath });
    const snapshot = readAnchoredSnapshot(rootDescriptor, relativePath, purpose);
    if (!sameFile(observed, snapshot.before)) fail(`${purpose} changed before it could be opened: ${relativePath}`);
    if (fileIdentity(snapshot.before) !== fileIdentity(snapshot.after)) fail(`${purpose} changed while being read: ${relativePath}`);
    const { bytes } = snapshot;
    if (isNativeBinary(bytes)) fail(`${purpose} must not be a native binary: ${relativePath}`);
    validateRuntimeContents(relativePath, bytes, purpose);
    return { relativePath, bytes };
  } catch (error) {
    if (error?.code === "ELOOP") fail(`${purpose} must not be a symlink: ${relativePath}`);
    throw error;
  } finally {
    for (const descriptor of descriptors.reverse()) closeSync(descriptor);
  }
}

// Node does not expose openat(2), and macOS does not support descending through
// /dev/fd/<directory-fd>/child (it reports ENOENT).  The installed Python 3
// standard library does expose the same kernel primitive via os.open(dir_fd=).
// Descriptor 3 is the already-open canonical root; every component is opened
// relative to it with O_NOFOLLOW, so an ancestor replacement cannot redirect us.
function readAnchoredSnapshot(rootDescriptor, relativePath, purpose) {
  const program = String.raw`
import json, os, stat, sys

root_fd = 3
opened = []
try:
    parent = root_fd
    parts = sys.argv[1].split("/")
    for part in parts[:-1]:
        descriptor = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
        opened.append(descriptor)
        if not stat.S_ISDIR(os.fstat(descriptor).st_mode):
            raise RuntimeError("ancestor is not a directory")
        parent = descriptor
    descriptor = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW, dir_fd=parent)
    opened.append(descriptor)
    before = os.fstat(descriptor)
    if not stat.S_ISREG(before.st_mode):
        raise RuntimeError("final path is not a regular file")
    chunks = []
    while True:
        chunk = os.read(descriptor, 65536)
        if not chunk:
            break
        chunks.append(chunk)
    after = os.fstat(descriptor)
    metadata = {"before": [before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns / 1_000_000, before.st_ctime_ns / 1_000_000], "after": [after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns / 1_000_000, after.st_ctime_ns / 1_000_000]}
    sys.stderr.write("SNAPSHOT:" + json.dumps(metadata, separators=(",", ":")))
    sys.stdout.buffer.write(b"".join(chunks))
except Exception as error:
    sys.stderr.write("SNAPSHOT_ERROR:" + str(error))
    sys.exit(1)
finally:
    for descriptor in reversed(opened):
        os.close(descriptor)
`;
  const result = spawnSync("python3", ["-c", program, relativePath], {
    stdio: ["ignore", "pipe", "pipe", rootDescriptor],
  });
  const stderr = result.stderr.toString("utf8");
  if (result.status !== 0) {
    const detail = stderr.replace(/^SNAPSHOT_ERROR:/, "");
    if (/Not a directory|Too many levels of symbolic links/i.test(detail)) {
      fail(`${purpose} encountered a symlink or non-directory ancestor: ${relativePath}`);
    }
    fail(`${purpose} could not be opened safely: ${detail}`);
  }
  const metadata = stderr.match(/^SNAPSHOT:(.+)$/)?.[1];
  if (!metadata) fail(`${purpose} safe snapshot did not return file metadata`);
  let parsed;
  try {
    parsed = JSON.parse(metadata);
  } catch {
    fail(`${purpose} safe snapshot returned invalid file metadata`);
  }
  const statFrom = (values) => ({
    dev: values[0], ino: values[1], size: values[2], mtimeMs: values[3], ctimeMs: values[4], isFile: () => true,
  });
  return { bytes: result.stdout, before: statFrom(parsed.before), after: statFrom(parsed.after) };
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
  if (typeof manifest.manifest_version !== "number" || typeof manifest.name !== "string" || typeof manifest.version !== "string") {
    fail("manifest must contain numeric manifest_version and string name/version");
  }
  for (const field of ["version_name", "description", "short_name", "key", "author", "homepage_url", "minimum_chrome_version", "update_url"]) {
    if (manifest[field] !== undefined && typeof manifest[field] !== "string") fail(`manifest ${field} must be a string`);
  }
  for (const field of ["offline_enabled"]) {
    if (manifest[field] !== undefined && typeof manifest[field] !== "boolean") fail(`manifest ${field} must be a boolean`);
  }
  if (manifest.incognito !== undefined && typeof manifest.incognito !== "string") fail("manifest incognito must be a string");
  for (const field of ["permissions", "optional_permissions", "host_permissions", "optional_host_permissions"]) {
    if (manifest[field] !== undefined) validateStringArray(manifest[field], field);
  }
  if (manifest.background !== undefined) {
    const background = object(manifest.background, "background");
    validateKeys(background, new Set(["service_worker", "type"]), "background");
    if (background.service_worker !== undefined && typeof background.service_worker !== "string") fail("background service worker must be a path");
    if (background.type !== undefined && background.type !== "module") fail("background type must be module");
  }
  if (manifest.content_scripts !== undefined) {
    for (const [index, contentScript] of array(manifest.content_scripts, "content scripts").entries()) {
      const script = object(contentScript, `content script ${index}`);
      validateKeys(script, new Set(["matches", "exclude_matches", "include_globs", "exclude_globs", "css", "js", "run_at", "all_frames", "match_about_blank", "match_origin_as_fallback", "world"]), `content script`);
      for (const field of ["matches", "exclude_matches", "include_globs", "exclude_globs", "css", "js"]) if (script[field] !== undefined) validateStringArray(script[field], `content script ${index} ${field}`);
      for (const field of ["all_frames", "match_about_blank", "match_origin_as_fallback"]) if (script[field] !== undefined && typeof script[field] !== "boolean") fail(`content script ${index} ${field} must be a boolean`);
      for (const field of ["run_at", "world"]) if (script[field] !== undefined && typeof script[field] !== "string") fail(`content script ${index} ${field} must be a string`);
    }
  }
  if (manifest.icons !== undefined) {
    const icons = object(manifest.icons, "icons");
    for (const [size, path] of Object.entries(icons)) {
      if (!/^\d+$/.test(size) || typeof path !== "string") fail("icons must map numeric sizes to paths");
    }
  }
  if (manifest.action !== undefined) {
    const action = object(manifest.action, "action");
    validateKeys(action, new Set(["default_icon", "default_popup", "default_title", "default_badge_text", "default_badge_color", "default_popup_height", "default_popup_width"]), "action");
    if (action.default_popup !== undefined && typeof action.default_popup !== "string") fail("action popup must be a path");
    if (action.default_icon !== undefined && typeof action.default_icon !== "string" && (!action.default_icon || typeof action.default_icon !== "object" || Array.isArray(action.default_icon))) fail("action default icon must be a path or icon map");
    for (const field of ["default_title", "default_badge_text"]) if (action[field] !== undefined && typeof action[field] !== "string") fail(`action ${field} must be a string`);
    if (action.default_badge_color !== undefined && typeof action.default_badge_color !== "string" && !Array.isArray(action.default_badge_color)) fail("action default badge color must be a string or array");
    for (const field of ["default_popup_height", "default_popup_width"]) if (action[field] !== undefined && typeof action[field] !== "number") fail(`action ${field} must be a number`);
  }
  if (manifest.options_page !== undefined && typeof manifest.options_page !== "string") fail("options page must be a path");
  if (manifest.devtools_page !== undefined && typeof manifest.devtools_page !== "string") fail("DevTools page must be a path");
  if (manifest.options_ui !== undefined) {
    const options = object(manifest.options_ui, "options UI");
    validateKeys(options, new Set(["page", "open_in_tab"]), "options UI");
    if (options.page !== undefined && typeof options.page !== "string") fail("options UI page must be a path");
    if (options.open_in_tab !== undefined && typeof options.open_in_tab !== "boolean") fail("options UI open_in_tab must be a boolean");
  }
  if (manifest.side_panel !== undefined) {
    const sidePanel = object(manifest.side_panel, "side panel");
    validateKeys(sidePanel, new Set(["default_path"]), "side panel");
    if (sidePanel.default_path !== undefined && typeof sidePanel.default_path !== "string") fail("side panel default path must be a path");
  }
  if (manifest.chrome_url_overrides !== undefined) {
    const overrides = object(manifest.chrome_url_overrides, "chrome URL overrides");
    validateKeys(overrides, new Set(["newtab", "bookmarks", "history"]), "chrome URL overrides");
    for (const path of Object.values(overrides)) if (typeof path !== "string") fail("chrome URL overrides must contain paths");
  }
  if (manifest.sandbox !== undefined) {
    const sandbox = object(manifest.sandbox, "sandbox");
    validateKeys(sandbox, new Set(["pages", "content_security_policy"]), "sandbox");
    if (sandbox.pages !== undefined) validateStringArray(sandbox.pages, "sandbox pages");
    if (sandbox.content_security_policy !== undefined && typeof sandbox.content_security_policy !== "string") fail("sandbox content security policy must be a string");
  }
  if (manifest.web_accessible_resources !== undefined) {
    for (const [index, resource] of array(manifest.web_accessible_resources, "web accessible resources").entries()) {
      const entry = object(resource, `web accessible resource ${index}`);
      validateKeys(entry, new Set(["resources", "matches", "extension_ids", "use_dynamic_url", "match_origin_as_fallback"]), "web accessible resource");
      validateStringArray(entry.resources, `web accessible resource ${index} resources`);
      if (entry.matches !== undefined) validateStringArray(entry.matches, `web accessible resource ${index} matches`);
      if (entry.extension_ids !== undefined) validateStringArray(entry.extension_ids, `web accessible resource ${index} extension ids`);
      for (const field of ["use_dynamic_url", "match_origin_as_fallback"]) if (entry[field] !== undefined && typeof entry[field] !== "boolean") fail(`web accessible resource ${index} ${field} must be a boolean`);
    }
  }
  if (manifest.storage !== undefined) {
    const storage = object(manifest.storage, "storage");
    validateKeys(storage, new Set(["managed_schema"]), "storage");
    if (storage.managed_schema !== undefined && typeof storage.managed_schema !== "string") fail("managed storage schema must be a path");
  }
  if (manifest.declarative_net_request !== undefined) {
    const dnr = object(manifest.declarative_net_request, "declarative net request");
    validateKeys(dnr, new Set(["rule_resources"]), "declarative net request");
    if (dnr.rule_resources !== undefined) {
      for (const [index, resource] of array(dnr.rule_resources, "declarative net request rule resources").entries()) {
        const entry = object(resource, `declarative net request rule resource ${index}`);
        validateKeys(entry, new Set(["id", "enabled", "path"]), "declarative net request rule resource");
        if (typeof entry.id !== "string") fail(`declarative net request rule resource ${index} id must be a string`);
        if (typeof entry.enabled !== "boolean") fail(`declarative net request rule resource ${index} enabled must be a boolean`);
        if (typeof entry.path !== "string") fail(`declarative net request rule resource ${index} path must be a string`);
      }
    }
  }
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
  const sourceText = snapshot.bytes.toString("utf8");
  const compilerOptions = { allowJs: true, noLib: true, target: ts.ScriptTarget.Latest };
  const host = ts.createCompilerHost(compilerOptions);
  const source = ts.createSourceFile(snapshot.relativePath, sourceText, ts.ScriptTarget.Latest, true);
  const defaultGetSourceFile = host.getSourceFile.bind(host);
  host.getSourceFile = (fileName, languageVersion, onError, shouldCreateNewSourceFile) => (
    fileName === snapshot.relativePath ? source : defaultGetSourceFile(fileName, languageVersion, onError, shouldCreateNewSourceFile)
  );
  host.fileExists = (fileName) => fileName === snapshot.relativePath;
  host.readFile = (fileName) => fileName === snapshot.relativePath ? sourceText : undefined;
  const checker = ts.createProgram([snapshot.relativePath], compilerOptions, host).getTypeChecker();
  const constants = new Map();
  const globalObjectAliases = new Set();
  const importScriptsAliases = new Set();
  const declarations = [];
  const symbolAt = (node) => ts.isIdentifier(node) ? checker.getSymbolAtLocation(node) : undefined;
  const isLocalIdentifier = (node) => symbolAt(node)?.declarations?.some((declaration) => declaration.getSourceFile() === source) ?? false;
  const constant = (node) => constantString(node, constants, symbolAt);
  const isGlobalObject = (rawNode) => {
    const node = unwrapJsExpression(rawNode);
    if (!node) return false;
    if (!ts.isIdentifier(node)) return false;
    const symbol = symbolAt(node);
    return (["globalThis", "self", "window"].includes(node.text) && !isLocalIdentifier(node)) || (symbol && globalObjectAliases.has(symbol));
  };
  const isGlobalImportScripts = (rawNode) => {
    const node = unwrapJsExpression(rawNode);
    if (!node) return false;
    if (ts.isIdentifier(node)) {
      const symbol = symbolAt(node);
      return (
        (node.text === "importScripts" && isReferenceIdentifier(node) && !isLocalIdentifier(node)) ||
        (symbol && importScriptsAliases.has(symbol) && isReferenceIdentifier(node))
      );
    }
    return (
      (ts.isPropertyAccessExpression(node) && isGlobalObject(node.expression) && node.name.text === "importScripts") ||
      (ts.isElementAccessExpression(node) && isGlobalObject(node.expression) && constant(node.argumentExpression) === "importScripts")
    );
  };
  const collectDeclarations = (node) => {
    if (ts.isVariableDeclaration(node)) declarations.push(node);
    ts.forEachChild(node, collectDeclarations);
  };
  collectDeclarations(source);
  let changed;
  do {
    changed = false;
    for (const declaration of declarations) {
      if (!isConstDeclaration(declaration)) continue;
      if (ts.isIdentifier(declaration.name)) {
        const symbol = symbolAt(declaration.name);
        if (!symbol) continue;
        const value = constant(declaration.initializer);
        if (value !== undefined && !constants.has(symbol)) {
          constants.set(symbol, value);
          changed = true;
        }
        if (isGlobalObject(declaration.initializer) && !globalObjectAliases.has(symbol)) {
          globalObjectAliases.add(symbol);
          changed = true;
        }
        if (isGlobalImportScripts(declaration.initializer) && !importScriptsAliases.has(symbol)) {
          importScriptsAliases.add(symbol);
          changed = true;
        }
      } else if (ts.isObjectBindingPattern(declaration.name) && isGlobalObject(declaration.initializer)) {
        for (const element of declaration.name.elements) {
          if (bindingPropertyName(element, constant) !== "importScripts") continue;
          if (!ts.isIdentifier(element.name)) fail(`Unsupported importScripts alias in ${snapshot.relativePath}`);
          const symbol = symbolAt(element.name);
          if (symbol && !importScriptsAliases.has(symbol)) {
            importScriptsAliases.add(symbol);
            changed = true;
          }
        }
      }
    }
  } while (changed);
  const staticValue = (node, purpose) => {
    const value = constant(node);
    if (value === undefined) fail(`${purpose} must use a constant string in ${snapshot.relativePath}`);
    return value;
  };
  const visit = (node) => {
    if (ts.isVariableDeclaration(node) && !isConstDeclaration(node)) {
      if (isGlobalObject(node.initializer) || isGlobalImportScripts(node.initializer)) {
        fail(`Mutable global importScripts alias is not permitted in ${snapshot.relativePath}`);
      }
      if (ts.isObjectBindingPattern(node.name) && isGlobalObject(node.initializer)) {
        for (const element of node.name.elements) {
          if (bindingPropertyName(element, constant) === "importScripts") fail(`Mutable importScripts alias is not permitted in ${snapshot.relativePath}`);
        }
      }
    }
    if (ts.isBinaryExpression(node) && node.operatorToken.kind >= ts.SyntaxKind.FirstAssignment && node.operatorToken.kind <= ts.SyntaxKind.LastAssignment) {
      if (assignedSymbols(node.left, symbolAt).some((symbol) => globalObjectAliases.has(symbol) || importScriptsAliases.has(symbol))) {
        fail(`Reassignment of a tracked importScripts alias is not permitted in ${snapshot.relativePath}`);
      }
      if (isGlobalObject(node.right) || isGlobalImportScripts(node.right)) fail(`Mutable global importScripts alias is not permitted in ${snapshot.relativePath}`);
    }
    if ((ts.isPrefixUnaryExpression(node) || ts.isPostfixUnaryExpression(node)) && [ts.SyntaxKind.PlusPlusToken, ts.SyntaxKind.MinusMinusToken].includes(node.operator)) {
      if (assignedSymbols(node.operand, symbolAt).some((symbol) => globalObjectAliases.has(symbol) || importScriptsAliases.has(symbol))) {
        fail(`Mutation of a tracked importScripts alias is not permitted in ${snapshot.relativePath}`);
      }
    }
    if (isGlobalImportScripts(node)) fail(`importScripts is not permitted in ${snapshot.relativePath}`);
    if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier) references.push({ value: staticValue(node.moduleSpecifier, "module specifier"), purpose: "JavaScript import" });
    if (ts.isCallExpression(node)) {
      if (node.expression.kind === ts.SyntaxKind.ImportKeyword) references.push({ value: staticValue(node.arguments[0], "dynamic import"), purpose: "dynamic import" });
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return references;
}

function unwrapJsExpression(node) {
  while (node && (
    ts.isParenthesizedExpression(node) || ts.isAsExpression(node) ||
    ts.isTypeAssertionExpression(node) || ts.isNonNullExpression(node)
  )) node = node.expression;
  return node;
}

function isConstDeclaration(node) {
  return ts.isVariableDeclarationList(node.parent) && (node.parent.flags & ts.NodeFlags.Const) !== 0;
}

function bindingPropertyName(element, constant) {
  if (element.dotDotDotToken) return undefined;
  if (!element.propertyName) return ts.isIdentifier(element.name) ? element.name.text : undefined;
  if (ts.isComputedPropertyName(element.propertyName)) return constant(element.propertyName.expression);
  return element.propertyName.text;
}

function assignedSymbols(node, symbolAt) {
  const symbols = [];
  const collect = (target) => {
    target = unwrapJsExpression(target);
    if (ts.isIdentifier(target)) {
      const symbol = symbolAt(target);
      if (symbol) symbols.push(symbol);
    } else if (ts.isArrayLiteralExpression(target)) {
      for (const element of target.elements) collect(element);
    } else if (ts.isObjectLiteralExpression(target)) {
      for (const property of target.properties) {
        if (ts.isShorthandPropertyAssignment(property)) collect(property.name);
        else if (ts.isPropertyAssignment(property)) collect(property.initializer);
        else if (ts.isSpreadAssignment(property)) collect(property.expression);
      }
    }
  };
  collect(node);
  return symbols;
}

function constantString(node, constants, symbolAt) {
  if (!node) return undefined;
  node = unwrapJsExpression(node);
  if (ts.isStringLiteralLike(node) || ts.isNoSubstitutionTemplateLiteral(node)) return node.text;
  if (ts.isIdentifier(node)) return constants.get(symbolAt(node));
  if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.PlusToken) {
    const left = constantString(node.left, constants, symbolAt);
    const right = constantString(node.right, constants, symbolAt);
    return left === undefined || right === undefined ? undefined : `${left}${right}`;
  }
  return undefined;
}

function isReferenceIdentifier(node) {
  const parent = node.parent;
  return !(
    (ts.isPropertyAccessExpression(parent) && parent.name === node) ||
    (ts.isPropertyAssignment(parent) && parent.name === node) ||
    (ts.isMethodDeclaration(parent) && parent.name === node) ||
    (ts.isVariableDeclaration(parent) && parent.name === node) ||
    (ts.isBindingElement(parent) && parent.name === node) ||
    (ts.isParameter(parent) && parent.name === node) ||
    ((ts.isFunctionDeclaration(parent) || ts.isFunctionExpression(parent) || ts.isClassDeclaration(parent) || ts.isClassExpression(parent)) && parent.name === node)
  );
}

function cssReferences(snapshot) {
  const text = textContents(snapshot.bytes, `stylesheet ${snapshot.relativePath}`);
  const references = [];
  let index = 0;
  while (index < text.length) {
    if (text.startsWith("/*", index)) {
      const end = text.indexOf("*/", index + 2);
      if (end < 0) fail(`stylesheet ${snapshot.relativePath} has an unterminated comment`);
      index = end + 2;
      continue;
    }
    if (text[index] === "'" || text[index] === '"') {
      index = consumeCssString(text, index).next;
      continue;
    }
    if (text[index] === "@") {
      const atRule = consumeCssIdentifier(text, index + 1);
      if (!atRule || atRule.value.toLowerCase() !== "import") {
        index = atRule?.next ?? index + 1;
        continue;
      }
      index = skipCssSpaceAndComments(text, atRule.next);
      const functionName = consumeCssIdentifier(text, index);
      if (functionName?.value.toLowerCase() === "url" && text[functionName.next] === "(") {
        const url = consumeCssUrl(text, functionName.next);
        references.push({ value: url.value, purpose: "stylesheet import", executable: true });
        index = url.next;
      } else if (text[index] === "'" || text[index] === '"') {
        const value = consumeCssString(text, index);
        references.push({ value: value.value, purpose: "stylesheet import", executable: true });
        index = value.next;
      } else fail(`stylesheet ${snapshot.relativePath} has a malformed @import`);
      continue;
    }
    const identifier = consumeCssIdentifier(text, index);
    if (identifier) {
      if (identifier.value.toLowerCase() === "url" && text[identifier.next] === "(") {
        const url = consumeCssUrl(text, identifier.next);
        references.push({ value: url.value, purpose: "stylesheet URL", executable: false });
        index = url.next;
      } else index = identifier.next;
      continue;
    }
    index += 1;
  }
  return references;
}

function skipCssSpaceAndComments(text, start) {
  let index = start;
  while (index < text.length) {
    if (isCssWhitespace(text[index])) index += 1;
    else if (text.startsWith("/*", index)) {
      const end = text.indexOf("*/", index + 2);
      if (end < 0) fail("stylesheet has an unterminated comment");
      index = end + 2;
    } else break;
  }
  return index;
}

function consumeCssString(text, start) {
  const quote = text[start];
  let value = "";
  let index = start + 1;
  while (index < text.length) {
    if (text[index] === quote) return { value, next: index + 1 };
    if (text[index] === "\\") {
      const escaped = consumeCssEscape(text, index, true);
      value += escaped.value;
      index = escaped.next;
    } else {
      if (isCssNewline(text[index])) fail("stylesheet has an unescaped newline in a string");
      value += text[index];
      index += 1;
    }
  }
  fail("stylesheet has an unterminated string");
}

function isCssWhitespace(character) {
  return character === " " || character === "\t" || isCssNewline(character);
}

function isCssNewline(character) {
  return character === "\n" || character === "\r" || character === "\f";
}

function consumeCssEscape(text, start, allowLineContinuation = false) {
  let index = start + 1;
  if (index >= text.length) fail("stylesheet has an incomplete escape");
  if (isCssNewline(text[index])) {
    if (!allowLineContinuation) fail("stylesheet identifier has an escaped newline");
    if (text[index] === "\r" && text[index + 1] === "\n") index += 2;
    else index += 1;
    return { value: "", next: index };
  }
  const hex = text.slice(index).match(/^[0-9a-f]{1,6}/i)?.[0];
  if (hex) {
    index += hex.length;
    if (isCssWhitespace(text[index])) {
      if (text[index] === "\r" && text[index + 1] === "\n") index += 2;
      else index += 1;
    }
    const codePoint = Number.parseInt(hex, 16);
    const value = codePoint === 0 || codePoint > 0x10ffff || (codePoint >= 0xd800 && codePoint <= 0xdfff)
      ? "\ufffd"
      : String.fromCodePoint(codePoint);
    return { value, next: index };
  }
  const codePoint = text.codePointAt(index);
  const value = String.fromCodePoint(codePoint);
  return { value, next: index + value.length };
}

function isCssNameStart(character) {
  return character === "_" || /[A-Za-z]/.test(character ?? "") || (character?.codePointAt(0) ?? 0) >= 0x80;
}

function isCssNameCharacter(character) {
  return isCssNameStart(character) || /[0-9-]/.test(character ?? "");
}

function consumeCssIdentifier(text, start) {
  const first = text[start];
  const second = text[start + 1];
  if (first === "-") {
    if (!(isCssNameStart(second) || second === "-" || second === "\\")) return undefined;
  } else if (!(isCssNameStart(first) || first === "\\")) return undefined;
  let value = "";
  let index = start;
  while (index < text.length) {
    if (isCssNameCharacter(text[index])) {
      const codePoint = text.codePointAt(index);
      const character = String.fromCodePoint(codePoint);
      value += character;
      index += character.length;
    } else if (text[index] === "\\") {
      const escaped = consumeCssEscape(text, index);
      value += escaped.value;
      index = escaped.next;
    } else break;
  }
  return { value, next: index };
}

function consumeCssUrl(text, openParen) {
  let index = skipCssSpaceAndComments(text, openParen + 1);
  let value;
  if (text[index] === "'" || text[index] === '"') {
    const quoted = consumeCssString(text, index);
    value = quoted.value;
    index = skipCssSpaceAndComments(text, quoted.next);
  } else {
    value = "";
    while (index < text.length && text[index] !== ")") {
      if (isCssWhitespace(text[index])) fail("stylesheet URL must quote whitespace");
      if (text[index] === "\\") {
        const escaped = consumeCssEscape(text, index);
        value += escaped.value;
        index = escaped.next;
      } else {
        value += text[index];
        index += 1;
      }
    }
  }
  if (text[index] !== ")") fail("stylesheet has an unterminated url()");
  if (!value) fail("stylesheet has an empty url()");
  return { value, next: index + 1 };
}

function collectRuntimeFiles(extensionRoot, manifestSnapshot, hooks) {
  const snapshots = new Map([["manifest.json", manifestSnapshot]]);
  const pending = [];
  const add = (value, purpose) => {
    const relativePath = localPath(value, purpose);
    if (!snapshots.has(relativePath)) {
      const snapshot = snapshotFile(extensionRoot, relativePath, purpose, hooks);
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
        const path = resolvedUrlPath(snapshot.relativePath, reference.value, reference.purpose, reference.executable);
        if (path) add(path, reference.purpose);
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

export function packageExtension(extensionDirectory, outputArchive, hooks = {}) {
  if (!extensionDirectory || !outputArchive) fail("Usage: node scripts/package-extension.mjs <extension_dir> <output_zip>");
  const output = resolve(outputArchive);
  if (existsSync(output)) fail(`Refusing to overwrite existing output archive: ${output}`);
  const extensionRoot = realpathSync(resolve(extensionDirectory));
  if (!lstatSync(extensionRoot).isDirectory()) fail(`Extension directory is not a directory: ${extensionDirectory}`);
  const manifestSnapshot = snapshotFile(extensionRoot, "manifest.json", "Extension manifest.json", hooks);
  const manifest = JSON.parse(manifestSnapshot.bytes.toString("utf8"));
  validateManifestSchema(manifest);
  if (manifest.manifest_version !== 3) fail("Extension manifest must be Manifest V3");
  const expectedVersion = cargoChromeVersion();
  if (chromeVersion(manifest.version, "manifest version") !== expectedVersion) fail(`Extension manifest version ${manifest.version} does not match Cargo version ${expectedVersion}`);
  const snapshots = collectRuntimeFiles(extensionRoot, manifestSnapshot, hooks);
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
