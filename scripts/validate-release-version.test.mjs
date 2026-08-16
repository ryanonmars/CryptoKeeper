import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const validatorPath = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "validate-release-version.mjs",
);

function writeFixtureFile(root, relativePath, contents) {
  const path = resolve(root, relativePath);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, contents);
}

function createFixture(t) {
  const root = mkdtempSync(resolve(tmpdir(), "termkey-release-version-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));

  writeFixtureFile(
    root,
    "Cargo.toml",
    `[workspace]
members = ["apps/*"]
resolver = "2"
`,
  );
  writeFixtureFile(
    root,
    "apps/cli/Cargo.toml",
    `[package]
name = "termkey"
version = "0.2.26"
edition = "2021"
`,
  );
  writeFixtureFile(
    root,
    "Cargo.lock",
    `version = 4

[[package]]
name = "termkey"
version = "0.2.26"
`,
  );
  writeFixtureFile(
    root,
    "package.json",
    JSON.stringify({
      name: "termkey",
      private: true,
      workspaces: ["apps/*", "packages/*"],
    }),
  );
  writeFixtureFile(
    root,
    "apps/extension/package.json",
    JSON.stringify({
      name: "@termkey/extension",
      version: "0.2.26",
      dependencies: {
        "@termkey/core": "0.2.26",
        "@termkey/types": "0.2.26",
      },
    }),
  );
  writeFixtureFile(
    root,
    "packages/core/package.json",
    JSON.stringify({
      name: "@termkey/core",
      version: "0.2.26",
    }),
  );
  writeFixtureFile(
    root,
    "packages/types/package.json",
    JSON.stringify({
      name: "@termkey/types",
      version: "0.2.26",
    }),
  );
  writeFixtureFile(
    root,
    "package-lock.json",
    JSON.stringify({
      name: "termkey",
      lockfileVersion: 3,
      packages: {
        "": {
          name: "termkey",
          workspaces: ["apps/*", "packages/*"],
        },
        "apps/extension": {
          name: "@termkey/extension",
          version: "0.2.26",
          dependencies: {
            "@termkey/core": "0.2.26",
            "@termkey/types": "0.2.26",
          },
        },
        "packages/core": {
          name: "@termkey/core",
          version: "0.2.26",
        },
        "packages/types": {
          name: "@termkey/types",
          version: "0.2.26",
        },
      },
    }),
  );
  writeFixtureFile(
    root,
    "apps/extension/manifest.json",
    JSON.stringify({ version: "0.2.26" }),
  );
  writeFixtureFile(
    root,
    "apps/extension/public/manifest.json",
    JSON.stringify({ version: "0.2.26" }),
  );

  return root;
}

function runValidator(root) {
  return spawnSync(
    process.execPath,
    [validatorPath, "v0.2.26", "--root", root],
    { encoding: "utf8" },
  );
}

test("accepts matching workspace, lockfile, dependency, and manifest versions", (t) => {
  const root = createFixture(t);

  const result = runValidator(root);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Release versions match 0\.2\.26\./);
});

test("accepts a fourth-component Chrome Store manifest revision", (t) => {
  const root = createFixture(t);
  writeFixtureFile(
    root,
    "apps/extension/manifest.json",
    JSON.stringify({ version: "0.2.26.1", version_name: "0.2.26" }),
  );
  writeFixtureFile(
    root,
    "apps/extension/public/manifest.json",
    JSON.stringify({ version: "0.2.26.1", version_name: "0.2.26" }),
  );

  const result = runValidator(root);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Release versions match 0\.2\.26\./);
});

test("rejects an npm workspace package version mismatch", (t) => {
  const root = createFixture(t);
  writeFixtureFile(
    root,
    "packages/core/package.json",
    JSON.stringify({
      name: "@termkey/core",
      version: "0.2.25",
    }),
  );

  const result = runValidator(root);

  assert.equal(result.status, 1);
  assert.match(
    result.stderr,
    /packages\/core\/package\.json: expected 0\.2\.26, found 0\.2\.25/,
  );
});

test("rejects a source-less Cargo.lock workspace version mismatch", (t) => {
  const root = createFixture(t);
  writeFixtureFile(
    root,
    "Cargo.lock",
    `version = 4

[[package]]
name = "termkey"
version = "0.2.25"
`,
  );

  const result = runValidator(root);

  assert.equal(result.status, 1);
  assert.match(
    result.stderr,
    /Cargo\.lock:termkey: expected 0\.2\.26, found 0\.2\.25/,
  );
});

test("rejects a package-lock local workspace dependency mismatch", (t) => {
  const root = createFixture(t);
  const packageLock = {
    name: "termkey",
    lockfileVersion: 3,
    packages: {
      "": {
        name: "termkey",
        workspaces: ["apps/*", "packages/*"],
      },
      "apps/extension": {
        name: "@termkey/extension",
        version: "0.2.26",
        dependencies: {
          "@termkey/core": "0.2.25",
          "@termkey/types": "0.2.26",
        },
      },
      "packages/core": {
        name: "@termkey/core",
        version: "0.2.26",
      },
      "packages/types": {
        name: "@termkey/types",
        version: "0.2.26",
      },
    },
  };
  writeFixtureFile(root, "package-lock.json", JSON.stringify(packageLock));

  const result = runValidator(root);

  assert.equal(result.status, 1);
  assert.match(
    result.stderr,
    /package-lock\.json:apps\/extension:dependencies\.@termkey\/core: expected 0\.2\.26, found 0\.2\.25/,
  );
});
