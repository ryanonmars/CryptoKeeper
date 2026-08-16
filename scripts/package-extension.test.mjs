import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { packageExtension } from "./package-extension.mjs";

const scriptsDirectory = dirname(fileURLToPath(import.meta.url));
const packagerPath = resolve(scriptsDirectory, "package-extension.mjs");
const validPng = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR4nGNgAAIAAAUAAXpeqz8AAAAASUVORK5CYII=", "base64");

function writeFixtureFile(root, relativePath, contents = "") {
  const path = resolve(root, relativePath);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, contents);
  return path;
}

function createFixture(t) {
  const root = mkdtempSync(resolve(tmpdir(), "termkey-extension-package-"));
  const extension = resolve(root, "extension");
  t.after(() => rmSync(root, { recursive: true, force: true }));

  writeFixtureFile(
    extension,
    "manifest.json",
    JSON.stringify({
      manifest_version: 3,
      name: "Fixture extension",
      version: "1.0.2",
      background: { service_worker: "dist/background.js", type: "module" },
      content_scripts: [{ matches: ["https://example.test/*"], js: ["dist/content.js"] }],
      icons: { 128: "public/icon128.png" },
      action: { default_popup: "popup.html" },
      web_accessible_resources: [
        { resources: ["prompt.html"], matches: ["https://example.test/*"] },
      ],
    }),
  );
  writeFixtureFile(
    extension,
    "popup.html",
    '<!doctype html><script type="module" src="dist/popup.js"></script>',
  );
  writeFixtureFile(
    extension,
    "prompt.html",
    '<!doctype html><script type="module" src="dist/prompt.js"></script>',
  );
  writeFixtureFile(extension, "dist/background.js", "export const background = true;\n");
  writeFixtureFile(extension, "dist/content.js", "export const content = true;\n");
  writeFixtureFile(extension, "dist/popup.js", "export const popup = true;\n");
  writeFixtureFile(extension, "dist/prompt.js", "export const prompt = true;\n");
  writeFixtureFile(
    extension,
    "public/icon128.png",
    validPng,
  );

  return { root, extension };
}

function readManifest(extension) {
  return JSON.parse(readFileSync(resolve(extension, "manifest.json"), "utf8"));
}

function writeManifest(extension, manifest) {
  writeFixtureFile(extension, "manifest.json", JSON.stringify(manifest));
}

function runPackager(extension, output) {
  return spawnSync(process.execPath, [packagerPath, extension, output], {
    encoding: "utf8",
  });
}

function run(command, args) {
  return spawnSync(command, args, { encoding: "utf8" });
}

function digest(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function failure(result) {
  return `${result.stdout}\n${result.stderr}`;
}

test("packages only manifest runtime files into a reproducible normalized archive", (t) => {
  const { root, extension } = createFixture(t);
  writeFixtureFile(extension, "source.ts", "export const source = true;\n");
  writeFixtureFile(extension, ".env", "SECRET=not-for-store\n");
  writeFixtureFile(extension, "dist/debug.map", "{}\n");
  writeFixtureFile(extension, "notes.md", "do not ship\n");
  const firstArchive = resolve(root, "first.zip");
  const secondArchive = resolve(root, "second.zip");

  const first = runPackager(extension, firstArchive);
  const second = runPackager(extension, secondArchive);

  assert.equal(first.status, 0, failure(first));
  assert.equal(second.status, 0, failure(second));
  assert.equal(digest(firstArchive), digest(secondArchive));

  const entriesResult = run("unzip", ["-Z1", firstArchive]);
  assert.equal(entriesResult.status, 0, failure(entriesResult));
  const entries = entriesResult.stdout.trim().split("\n").filter(Boolean);
  assert.deepEqual(entries, [
    "dist/background.js",
    "dist/content.js",
    "dist/popup.js",
    "dist/prompt.js",
    "manifest.json",
    "popup.html",
    "prompt.html",
    "public/icon128.png",
  ].sort());
  assert.equal(entries.some((entry) => /\.env|\.map$|\.ts$|\.md$/i.test(entry)), false);
  assert.deepEqual(entries, [...entries].sort());

  const integrity = run("unzip", ["-t", firstArchive]);
  assert.equal(integrity.status, 0, failure(integrity));
  const extracted = resolve(root, "extracted");
  const extraction = run("unzip", ["-q", firstArchive, "-d", extracted]);
  assert.equal(extraction.status, 0, failure(extraction));
  for (const entry of entries) {
    assert.equal(statSync(resolve(extracted, entry)).mode & 0o777, 0o644, entry);
  }
  const metadata = run("zipinfo", ["-v", firstArchive]);
  assert.equal(metadata.status, 0, failure(metadata));
  assert.match(metadata.stdout, /file last modified on \(DOS date\/time\):\s+2000 Jan 1 00:00:00/);
});

test("omits a development manifest key from the Chrome Web Store archive", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.key = "development-only-public-key";
  writeManifest(extension, manifest);
  const archive = resolve(root, "archive.zip");

  const result = runPackager(extension, archive);

  assert.equal(result.status, 0, failure(result));
  const extracted = resolve(root, "extracted");
  const extraction = run("unzip", ["-q", archive, "-d", extracted]);
  assert.equal(extraction.status, 0, failure(extraction));
  assert.equal(JSON.parse(readFileSync(resolve(extracted, "manifest.json"), "utf8")).key, undefined);
  assert.equal(readManifest(extension).key, "development-only-public-key");
});

test("rejects a missing manifest before creating the output", (t) => {
  const { root, extension } = createFixture(t);
  rmSync(resolve(extension, "manifest.json"));
  const output = resolve(root, "archive.zip");

  const result = runPackager(extension, output);

  assert.notEqual(result.status, 0);
  assert.match(failure(result), /manifest\.json/i);
  assert.equal(lstatSync(output, { throwIfNoEntry: false }), undefined);
});

test("rejects Manifest V2", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.manifest_version = 2;
  writeManifest(extension, manifest);

  const result = runPackager(extension, resolve(root, "archive.zip"));

  assert.notEqual(result.status, 0);
  assert.match(failure(result), /Manifest V3/i);
});

test("rejects a manifest runtime reference that is missing", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.background.service_worker = "dist/missing.js";
  writeManifest(extension, manifest);

  const result = runPackager(extension, resolve(root, "archive.zip"));

  assert.notEqual(result.status, 0);
  assert.match(failure(result), /missing\.js/i);
});

test("rejects remote executable URLs", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.background.service_worker = "https://example.test/background.js";
  writeManifest(extension, manifest);

  const result = runPackager(extension, resolve(root, "archive.zip"));

  assert.notEqual(result.status, 0);
  assert.match(failure(result), /remote|URL/i);
});

test("rejects paths that traverse out of the extension", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.action.default_popup = "../outside.html";
  writeManifest(extension, manifest);

  const result = runPackager(extension, resolve(root, "archive.zip"));

  assert.notEqual(result.status, 0);
  assert.match(failure(result), /travers|outside/i);
});

test("rejects manifest runtime files outside the allowed roots", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.background.service_worker = "scripts/background.js";
  writeManifest(extension, manifest);
  writeFixtureFile(extension, "scripts/background.js", "export {};\n");

  const result = runPackager(extension, resolve(root, "archive.zip"));

  assert.notEqual(result.status, 0);
  assert.match(failure(result), /allowed roots|not allowed/i);
});

test("resolves web-accessible-resource patterns to concrete runtime files", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.web_accessible_resources = [
    { resources: ["public/images/*.png"], matches: ["https://example.test/*"] },
  ];
  writeManifest(extension, manifest);
  writeFixtureFile(
    extension,
    "public/images/logo.png",
    validPng,
  );
  const output = resolve(root, "archive.zip");

  const result = runPackager(extension, output);

  assert.equal(result.status, 0, failure(result));
  const entries = run("unzip", ["-Z1", output]).stdout.trim().split("\n");
  assert.equal(entries.includes("public/images/logo.png"), true);
});

test("rejects a native executable even without a filename extension", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.web_accessible_resources = [
    { resources: ["dist/native"], matches: ["https://example.test/*"] },
  ];
  writeManifest(extension, manifest);
  writeFixtureFile(extension, "dist/native", Buffer.from([0x7f, 0x45, 0x4c, 0x46]));

  const result = runPackager(extension, resolve(root, "archive.zip"));

  assert.notEqual(result.status, 0);
  assert.match(failure(result), /permitted|native binary/i);
});

test("rejects symlinked runtime files that escape the extension", (t) => {
  const { root, extension } = createFixture(t);
  const outside = writeFixtureFile(root, "outside.js", "export {};\n");
  rmSync(resolve(extension, "dist/background.js"));
  symlinkSync(outside, resolve(extension, "dist/background.js"));

  const result = runPackager(extension, resolve(root, "archive.zip"));

  assert.notEqual(result.status, 0);
  assert.match(failure(result), /symlink|escape|outside/i);
});

test("rejects a manifest symlink that escapes the extension", (t) => {
  const { root, extension } = createFixture(t);
  const outside = writeFixtureFile(root, "outside-manifest.json", readFileSync(resolve(extension, "manifest.json")));
  rmSync(resolve(extension, "manifest.json"));
  symlinkSync(outside, resolve(extension, "manifest.json"));

  const result = runPackager(extension, resolve(root, "archive.zip"));

  assert.notEqual(result.status, 0);
  assert.match(failure(result), /manifest.*symlink|symlink.*manifest|outside/i);
});

test("accepts a fourth-component Chrome Store revision of Cargo's Chrome version", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.version = "1.0.2.1";
  manifest.version_name = "1.0.2";
  writeManifest(extension, manifest);

  const result = runPackager(extension, resolve(root, "archive.zip"));

  assert.equal(result.status, 0, failure(result));
});

test("rejects extension versions outside Cargo's Chrome release line", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.version = "1.0.3";
  writeManifest(extension, manifest);

  const result = runPackager(extension, resolve(root, "archive.zip"));

  assert.notEqual(result.status, 0);
  assert.match(failure(result), /version/i);
});

test("refuses to overwrite an existing output archive", (t) => {
  const { root, extension } = createFixture(t);
  const output = writeFixtureFile(root, "archive.zip", "keep me\n");

  const result = runPackager(extension, output);

  assert.notEqual(result.status, 0);
  assert.match(failure(result), /already exists|overwrite/i);
  assert.equal(readFileSync(output, "utf8"), "keep me\n");
});

test("rejects remote and inline executable HTML and JavaScript", (t) => {
  const cases = [
    ["unquoted script", (extension) => writeFixtureFile(extension, "popup.html", '<script src=https://example.test/popup.js></script>')],
    ["module preload", (extension) => writeFixtureFile(extension, "popup.html", '<link rel=modulepreload href=https://example.test/popup.js>')],
    ["inline module", (extension) => writeFixtureFile(extension, "popup.html", '<script type=module>import "https://example.test/popup.js"</script>')],
    ["dynamic import", (extension) => writeFixtureFile(extension, "dist/popup.js", 'import("https://example.test/popup.js");')],
    ["importScripts", (extension) => writeFixtureFile(extension, "dist/background.js", 'importScripts("https://example.test/background.js");')],
  ];

  for (const [name, arrange] of cases) {
    const { root, extension } = createFixture(t);
    arrange(extension);
    const result = runPackager(extension, resolve(root, `${name}.zip`));

    assert.notEqual(result.status, 0, name);
    assert.match(failure(result), /remote|inline|URL|importScripts/i, name);
  }
});

test("includes declarative net request rule resources and rejects wrong path types", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.declarative_net_request = {
    rule_resources: [{ id: "rules", enabled: true, path: "dist/rules.json" }],
  };
  writeManifest(extension, manifest);
  writeFixtureFile(extension, "dist/rules.json", "[]\n");
  const output = resolve(root, "rules.zip");

  const included = runPackager(extension, output);

  assert.equal(included.status, 0, failure(included));
  assert.equal(run("unzip", ["-Z1", output]).stdout.split("\n").includes("dist/rules.json"), true);

  const invalid = readManifest(extension);
  invalid.declarative_net_request.rule_resources[0].path = 42;
  writeManifest(extension, invalid);
  const rejected = runPackager(extension, resolve(root, "invalid-rules.zip"));

  assert.notEqual(rejected.status, 0);
  assert.match(failure(rejected), /rule resource.*path|non-empty local path/i);
});

test("rejects sensitive, source, and archive files even when manifest-referenced", (t) => {
  const paths = [
    "dist/source.jsx",
    "dist/credentials.json",
    "dist/private-key.txt",
    "dist/object.o",
    "dist/archive.zip",
    "dist/installer.pkg",
  ];
  for (const path of paths) {
    const { root, extension } = createFixture(t);
    const manifest = readManifest(extension);
    manifest.web_accessible_resources = [{ resources: [path], matches: ["https://example.test/*"] }];
    writeManifest(extension, manifest);
    writeFixtureFile(extension, path, "not for the store\n");

    const result = runPackager(extension, resolve(root, `${path.replaceAll("/", "-")}.zip`));

    assert.notEqual(result.status, 0, path);
    assert.match(failure(result), /permitted|sensitive|source|archive|native/i, path);
  }
});

test("resolves HTML URLs with sibling, parent, root, query, and fragment semantics", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.web_accessible_resources = [
    { resources: ["public/pages/runtime.html"], matches: ["https://example.test/*"] },
  ];
  writeManifest(extension, manifest);
  writeFixtureFile(
    extension,
    "public/pages/runtime.html",
    '<script src=sibling.js?cache=1#main></script><script src=../shared.js></script><script src=/dist/root.js#main></script>',
  );
  writeFixtureFile(extension, "public/pages/sibling.js", "export {};\n");
  writeFixtureFile(extension, "public/shared.js", "export {};\n");
  writeFixtureFile(extension, "dist/root.js", "export {};\n");
  const output = resolve(root, "urls.zip");

  const result = runPackager(extension, output);

  assert.equal(result.status, 0, failure(result));
  const entries = run("unzip", ["-Z1", output]).stdout.split("\n");
  for (const path of ["public/pages/sibling.js", "public/shared.js", "dist/root.js"]) {
    assert.equal(entries.includes(path), true, path);
  }
});

test("rejects control characters in runtime paths before ZIP creation", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.action.default_popup = "popup\n.html";
  writeManifest(extension, manifest);

  const result = runPackager(extension, resolve(root, "control.zip"));

  assert.notEqual(result.status, 0);
  assert.match(failure(result), /control character/i);
});

test("rejects aliases and computed references to importScripts", (t) => {
  const cases = [
    'const load = importScripts; load("https://example.test/remote.js");',
    'self["importScripts"]("https://example.test/remote.js");',
  ];
  for (const source of cases) {
    const { root, extension } = createFixture(t);
    writeFixtureFile(extension, "dist/background.js", source);

    const result = runPackager(extension, resolve(root, `${createHash("sha256").update(source).digest("hex")}.zip`));

    assert.notEqual(result.status, 0);
    assert.match(failure(result), /importScripts/i);
  }
});

test("includes managed storage schema and rejects unknown manifest fields", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.storage = { managed_schema: "public/schema.json" };
  writeManifest(extension, manifest);
  writeFixtureFile(extension, "public/schema.json", "{}\n");
  const output = resolve(root, "schema.zip");

  const included = runPackager(extension, output);

  assert.equal(included.status, 0, failure(included));
  assert.equal(run("unzip", ["-Z1", output]).stdout.split("\n").includes("public/schema.json"), true);

  manifest.future_runtime_asset = "public/schema.json";
  writeManifest(extension, manifest);
  const rejected = runPackager(extension, resolve(root, "unknown.zip"));

  assert.notEqual(rejected.status, 0);
  assert.match(failure(rejected), /unrecognized manifest field/i);
});

test("uses a strict runtime type allowlist and validates claimed file formats", (t) => {
  const cases = [
    ["dist/source.py", "print('not runtime')\n"],
    ["dist/auth-token.json", "{}\n"],
    ["public/renamed.png", Buffer.from("PK\x03\x04")],
    ["dist/renamed.js", Buffer.from([0xcf, 0xfa, 0xed, 0xfe])],
  ];
  for (const [path, contents] of cases) {
    const { root, extension } = createFixture(t);
    const manifest = readManifest(extension);
    manifest.web_accessible_resources = [{ resources: [path], matches: ["https://example.test/*"] }];
    writeManifest(extension, manifest);
    writeFixtureFile(extension, path, contents);

    const result = runPackager(extension, resolve(root, `${path.replaceAll("/", "-")}.zip`));

    assert.notEqual(result.status, 0, path);
    assert.match(failure(result), /permitted|sensitive|format|native/i, path);
  }
});

test("rejects an ancestor symlink that resolves outside the canonical extension root", (t) => {
  const { root, extension } = createFixture(t);
  const outside = resolve(root, "outside");
  mkdirSync(outside, { recursive: true });
  writeFixtureFile(outside, "background.js", "export {};\n");
  rmSync(resolve(extension, "dist"), { recursive: true, force: true });
  symlinkSync(outside, resolve(extension, "dist"));

  const result = runPackager(extension, resolve(root, "ancestor.zip"));

  assert.notEqual(result.status, 0);
  assert.match(failure(result), /outside|symlink/i);
});

test("recursively resolves local CSS imports, images, and fonts", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.content_scripts[0].css = ["dist/styles.css"];
  writeManifest(extension, manifest);
  writeFixtureFile(
    extension,
    "dist/styles.css",
    '@import url("nested.css"); .hero { background: url("../public/image.png?cache=1"); }',
  );
  writeFixtureFile(extension, "dist/nested.css", ".nested {}\n");
  writeFixtureFile(extension, "public/image.png", validPng);
  const output = resolve(root, "css.zip");

  const result = runPackager(extension, output);

  assert.equal(result.status, 0, failure(result));
  const entries = run("unzip", ["-Z1", output]).stdout.split("\n");
  for (const path of ["dist/styles.css", "dist/nested.css", "public/image.png"]) {
    assert.equal(entries.includes(path), true, path);
  }
});

test("allows unrelated importScripts properties but rejects constant global references", (t) => {
  const { root, extension } = createFixture(t);
  writeFixtureFile(extension, "dist/background.js", 'const importScripts = () => {}; importScripts("./local.js"); const helper = { importScripts() {} }; helper.importScripts("./local.js");');
  writeFixtureFile(extension, "dist/local.js", "export {};\n");

  const allowed = runPackager(extension, resolve(root, "allowed.zip"));

  assert.equal(allowed.status, 0, failure(allowed));
  for (const source of [
    "const key = 'import' + 'Scripts'; self[key]('https://example.test/remote.js');",
    "const load = globalThis.importScripts; load('https://example.test/remote.js');",
  ]) {
    writeFixtureFile(extension, "dist/background.js", source);
    const rejected = runPackager(extension, resolve(root, `${createHash("sha256").update(source).digest("hex")}.zip`));
    assert.notEqual(rejected.status, 0);
    assert.match(failure(rejected), /importScripts/i);
  }
});

test("rejects importScripts calls through global object and destructured aliases", (t) => {
  const cases = [
    'const g = globalThis; g.importScripts("https://example.test/remote.js");',
    'const { importScripts: load } = self; load("https://example.test/remote.js");',
    'const g = window; const load = g.importScripts; load("https://example.test/remote.js");',
  ];
  for (const source of cases) {
    const { root, extension } = createFixture(t);
    writeFixtureFile(extension, "dist/background.js", source);

    const rejected = runPackager(extension, resolve(root, `${createHash("sha256").update(source).digest("hex")}.zip`));

    assert.notEqual(rejected.status, 0);
    assert.match(failure(rejected), /importScripts/i);
  }
});

test("keeps importScripts alias analysis lexical and fails closed on mutable aliases", (t) => {
  const { root, extension } = createFixture(t);
  writeFixtureFile(
    extension,
    "dist/background.js",
    'const helper = { importScripts() {} }; { const globalThis = helper; const g = globalThis; const { importScripts: load } = g; load("./local.js"); }',
  );

  const allowed = runPackager(extension, resolve(root, "shadowed-alias.zip"));

  assert.equal(allowed.status, 0, failure(allowed));
  for (const source of [
    'let g = globalThis; g.importScripts("https://example.test/remote.js");',
    'const g = globalThis; g = {};',
  ]) {
    writeFixtureFile(extension, "dist/background.js", source);
    const rejected = runPackager(extension, resolve(root, `${createHash("sha256").update(source).digest("hex")}.zip`));
    assert.notEqual(rejected.status, 0);
    assert.match(failure(rejected), /alias|importScripts/i);
  }
});

test("rejects unrecognized nested manifest fields and Phase 1 localization", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.content_scripts[0].future_script_path = "dist/future.js";
  writeManifest(extension, manifest);

  const nested = runPackager(extension, resolve(root, "nested.zip"));

  assert.notEqual(nested.status, 0);
  assert.match(failure(nested), /unrecognized content script field/i);
  delete manifest.content_scripts[0].future_script_path;
  manifest.default_locale = "en";
  writeManifest(extension, manifest);
  const locale = runPackager(extension, resolve(root, "locale.zip"));
  assert.notEqual(locale.status, 0);
  assert.match(failure(locale), /unrecognized manifest field/i);
});

test("rejects arbitrary JSON and malformed Phase 1 PNG payloads", (t) => {
  const cases = [
    ["public/data.json", "{}\n"],
    ["public/header.png", validPng.subarray(0, 8)],
    ["public/appended.png", Buffer.concat([validPng, Buffer.from("MZ")])],
  ];
  for (const [path, contents] of cases) {
    const { root, extension } = createFixture(t);
    const manifest = readManifest(extension);
    manifest.web_accessible_resources = [{ resources: [path], matches: ["https://example.test/*"] }];
    writeManifest(extension, manifest);
    writeFixtureFile(extension, path, contents);
    const result = runPackager(extension, resolve(root, `${path.replaceAll("/", "-")}.zip`));
    assert.notEqual(result.status, 0, path);
    assert.match(failure(result), /JSON|png|runtime format|permitted/i, path);
  }
});

test("anchors snapshots when an ancestor is replaced after the root descriptor opens", (t) => {
  const { root, extension } = createFixture(t);
  const outside = resolve(root, "outside");
  const original = resolve(root, "original-dist");
  mkdirSync(outside, { recursive: true });
  writeFixtureFile(outside, "background.js", "export {};\n");

  assert.throws(
    () => packageExtension(extension, resolve(root, "anchored.zip"), {
      onRootOpened({ relativePath }) {
        if (relativePath !== "dist/background.js") return;
        renameSync(resolve(extension, "dist"), original);
        symlinkSync(outside, resolve(extension, "dist"));
      },
    }),
    /outside|symlink|changed/i,
  );
});

test("tokenizes CSS URLs with comments, strings, spaces, parentheses, and escapes", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.content_scripts[0].css = ["dist/styles.css"];
  writeManifest(extension, manifest);
  writeFixtureFile(
    extension,
    "dist/styles.css",
    '/* @import "ignored.css"; url("ignored.png") */ .copy { content: "url(ignored.png)"; } @import url("nested (one).css"); .hero { background: url("../public/image\\ space.png"); }',
  );
  writeFixtureFile(extension, "dist/nested (one).css", ".nested {}\n");
  writeFixtureFile(extension, "public/image space.png", validPng);
  const output = resolve(root, "css-tokenized.zip");

  const result = runPackager(extension, output);

  assert.equal(result.status, 0, failure(result));
  const entries = run("unzip", ["-Z1", output]).stdout.split("\n");
  assert.equal(entries.includes("dist/nested (one).css"), true);
  assert.equal(entries.includes("public/image space.png"), true);
  assert.equal(entries.includes("dist/ignored.css"), false);
});

test("decodes escaped CSS url and import identifiers", (t) => {
  const { root, extension } = createFixture(t);
  const manifest = readManifest(extension);
  manifest.content_scripts[0].css = ["dist/styles.css"];
  writeManifest(extension, manifest);
  writeFixtureFile(
    extension,
    "dist/styles.css",
    '@\\69mport "nested\\' + '\n.css"; .hero { background: u\\72l("../public/image.png"); }',
  );
  writeFixtureFile(extension, "dist/nested.css", ".nested {}\n");
  writeFixtureFile(extension, "public/image.png", validPng);
  const output = resolve(root, "escaped-css.zip");

  const result = runPackager(extension, output);

  assert.equal(result.status, 0, failure(result));
  const entries = run("unzip", ["-Z1", output]).stdout.split("\n");
  assert.equal(entries.includes("dist/nested.css"), true);
  assert.equal(entries.includes("public/image.png"), true);
});

test("rejects remote CSS imports with escaped identifiers", (t) => {
  const cases = [
    '@\\69mport "https://example.test/remote.css";',
    '@import u\\72l("https://example.test/remote.css");',
  ];
  for (const source of cases) {
    const { root, extension } = createFixture(t);
    const manifest = readManifest(extension);
    manifest.content_scripts[0].css = ["dist/styles.css"];
    writeManifest(extension, manifest);
    writeFixtureFile(extension, "dist/styles.css", source);

    const rejected = runPackager(extension, resolve(root, `${createHash("sha256").update(source).digest("hex")}.zip`));

    assert.notEqual(rejected.status, 0);
    assert.match(failure(rejected), /remote URL/i);
  }
});

test("rejects malformed CSS identifier escapes", (t) => {
  for (const source of ["@\\", 'u\\' + '\nl("../public/image.png")']) {
    const { root, extension } = createFixture(t);
    const manifest = readManifest(extension);
    manifest.content_scripts[0].css = ["dist/styles.css"];
    writeManifest(extension, manifest);
    writeFixtureFile(extension, "dist/styles.css", source);

    const rejected = runPackager(extension, resolve(root, `${createHash("sha256").update(source).digest("hex")}.zip`));

    assert.notEqual(rejected.status, 0);
    assert.match(failure(rejected), /escape|identifier/i);
  }
});
