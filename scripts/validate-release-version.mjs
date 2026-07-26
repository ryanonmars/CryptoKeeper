#!/usr/bin/env node

import {
  existsSync,
  readFileSync,
  readdirSync,
  statSync,
} from "node:fs";
import { dirname, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(import.meta.url);
const defaultRepoRoot = resolve(dirname(scriptPath), "..");
const dependencyFields = [
  "dependencies",
  "devDependencies",
  "optionalDependencies",
  "peerDependencies",
];

function toRepoPath(path) {
  return path.split(sep).join("/");
}

function read(root, relativePath) {
  return readFileSync(resolve(root, relativePath), "utf8");
}

function readJson(root, relativePath) {
  return JSON.parse(read(root, relativePath));
}

function tomlSection(contents, sectionName) {
  const escapedName = sectionName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = contents.match(
    new RegExp(
      `^\\[${escapedName}\\]\\s*$([\\s\\S]*?)(?=^\\[|(?![\\s\\S]))`,
      "m",
    ),
  );
  return match?.[1] ?? "";
}

function tomlString(section, key) {
  const escapedKey = key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return section.match(new RegExp(`^${escapedKey}\\s*=\\s*"([^"]+)"`, "m"))?.[1];
}

function tomlStringArray(section, key) {
  const escapedKey = key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = section.match(
    new RegExp(`^${escapedKey}\\s*=\\s*\\[([\\s\\S]*?)\\]`, "m"),
  );
  return match ? [...match[1].matchAll(/"([^"]+)"/g)].map((item) => item[1]) : [];
}

function readCargoPackage(root, relativePath) {
  const packageSection = tomlSection(read(root, relativePath), "package");
  return {
    name: tomlString(packageSection, "name"),
    version: tomlString(packageSection, "version"),
  };
}

function segmentPattern(segment) {
  const escaped = segment.replace(/[.+^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`^${escaped.replaceAll("*", ".*").replaceAll("?", ".")}$`);
}

function expandPattern(root, pattern, manifestName) {
  const segments = pattern.split("/").filter(Boolean);
  let paths = [root];

  for (const segment of segments) {
    const matcher = segmentPattern(segment);
    paths = paths.flatMap((parent) => {
      if (!existsSync(parent) || !statSync(parent).isDirectory()) {
        return [];
      }
      return readdirSync(parent, { withFileTypes: true })
        .filter((entry) => entry.isDirectory() && matcher.test(entry.name))
        .map((entry) => resolve(parent, entry.name));
    });
  }

  return paths
    .filter((path) => existsSync(resolve(path, manifestName)))
    .map((path) => toRepoPath(relative(root, path)));
}

function discoverCargoWorkspace(root) {
  const workspaceManifest = read(root, "Cargo.toml");
  const workspaceSection = tomlSection(workspaceManifest, "workspace");
  const memberPatterns = tomlStringArray(workspaceSection, "members");
  const excluded = new Set(
    tomlStringArray(workspaceSection, "exclude").flatMap((pattern) =>
      expandPattern(root, pattern, "Cargo.toml"),
    ),
  );
  const members = new Set(
    memberPatterns.flatMap((pattern) => expandPattern(root, pattern, "Cargo.toml")),
  );

  if (tomlSection(workspaceManifest, "package")) {
    members.add("");
  }

  return [...members]
    .filter((path) => !excluded.has(path))
    .sort()
    .map((path) => {
      const manifestPath = path ? `${path}/Cargo.toml` : "Cargo.toml";
      return { manifestPath, ...readCargoPackage(root, manifestPath) };
    });
}

function parseCargoLock(root) {
  return read(root, "Cargo.lock")
    .split(/^\[\[package\]\]\s*$/m)
    .slice(1)
    .map((section) => ({
      name: tomlString(section, "name"),
      version: tomlString(section, "version"),
      source: tomlString(section, "source"),
    }));
}

function npmWorkspacePatterns(rootPackage) {
  if (Array.isArray(rootPackage.workspaces)) {
    return rootPackage.workspaces;
  }
  if (Array.isArray(rootPackage.workspaces?.packages)) {
    return rootPackage.workspaces.packages;
  }
  return [];
}

function discoverNpmWorkspace(root) {
  const rootPackage = readJson(root, "package.json");
  return npmWorkspacePatterns(rootPackage)
    .flatMap((pattern) => expandPattern(root, pattern, "package.json"))
    .sort()
    .map((path) => ({
      path,
      manifestPath: `${path}/package.json`,
      package: readJson(root, `${path}/package.json`),
    }));
}

function displayVersion(value) {
  return value ?? "missing";
}

function addVersionMismatch(errors, path, expected, actual) {
  if (actual !== expected) {
    errors.push(
      `${path}: expected ${expected}, found ${displayVersion(actual)}`,
    );
  }
}

export function validateReleaseVersion(root, tag) {
  const errors = [];
  const canonicalPackage = readCargoPackage(root, "apps/cli/Cargo.toml");
  const canonicalVersion = canonicalPackage.version;

  if (!canonicalVersion) {
    return {
      version: undefined,
      errors: ["apps/cli/Cargo.toml: missing package version"],
    };
  }

  addVersionMismatch(
    errors,
    `release tag ${tag}`,
    canonicalVersion,
    tag?.replace(/^v/i, ""),
  );

  const cargoPackages = discoverCargoWorkspace(root);
  const cargoLockPackages = parseCargoLock(root);
  for (const cargoPackage of cargoPackages) {
    addVersionMismatch(
      errors,
      cargoPackage.manifestPath,
      canonicalVersion,
      cargoPackage.version,
    );

    const lockPackage = cargoLockPackages.find(
      (candidate) =>
        candidate.name === cargoPackage.name && candidate.source === undefined,
    );
    addVersionMismatch(
      errors,
      `Cargo.lock:${cargoPackage.name ?? cargoPackage.manifestPath}`,
      cargoPackage.version ?? canonicalVersion,
      lockPackage?.version,
    );
  }

  const npmPackages = discoverNpmWorkspace(root);
  const npmPackagesByName = new Map(
    npmPackages.map((workspace) => [workspace.package.name, workspace]),
  );
  const packageLock = readJson(root, "package-lock.json");
  const packageLockEntries = packageLock.packages ?? {};

  for (const workspace of npmPackages) {
    addVersionMismatch(
      errors,
      workspace.manifestPath,
      canonicalVersion,
      workspace.package.version,
    );

    const lockEntry = packageLockEntries[workspace.path];
    addVersionMismatch(
      errors,
      `package-lock.json:${workspace.path}`,
      workspace.package.version ?? canonicalVersion,
      lockEntry?.version,
    );

    for (const field of dependencyFields) {
      for (const [dependencyName, dependencyVersion] of Object.entries(
        workspace.package[field] ?? {},
      )) {
        const localDependency = npmPackagesByName.get(dependencyName);
        if (!dependencyName.startsWith("@termkey/") || !localDependency) {
          continue;
        }
        addVersionMismatch(
          errors,
          `${workspace.manifestPath}:${field}.${dependencyName}`,
          localDependency.package.version,
          dependencyVersion,
        );
        addVersionMismatch(
          errors,
          `package-lock.json:${workspace.path}:${field}.${dependencyName}`,
          localDependency.package.version,
          lockEntry?.[field]?.[dependencyName],
        );
      }
    }
  }

  for (const manifestPath of [
    "apps/extension/manifest.json",
    "apps/extension/public/manifest.json",
  ]) {
    addVersionMismatch(
      errors,
      manifestPath,
      canonicalVersion,
      readJson(root, manifestPath).version,
    );
  }

  return { version: canonicalVersion, errors };
}

function parseArguments(argv) {
  const args = [...argv];
  const rootIndex = args.indexOf("--root");
  let root = defaultRepoRoot;
  if (rootIndex !== -1) {
    const suppliedRoot = args[rootIndex + 1];
    if (!suppliedRoot) {
      throw new Error("--root requires a path");
    }
    root = resolve(suppliedRoot);
    args.splice(rootIndex, 2);
  }
  return { tag: args[0], root };
}

function run() {
  const { tag, root } = parseArguments(process.argv.slice(2));
  if (!tag) {
    console.error(
      "usage: node scripts/validate-release-version.mjs <tag> [--root <path>]",
    );
    return 1;
  }

  const result = validateReleaseVersion(root, tag);
  if (result.errors.length > 0) {
    console.error(`Release version validation failed:`);
    for (const error of result.errors) {
      console.error(`- ${error}`);
    }
    return 1;
  }

  console.log(`Release versions match ${result.version}.`);
  return 0;
}

if (resolve(process.argv[1] ?? "") === scriptPath) {
  try {
    process.exitCode = run();
  } catch (error) {
    console.error(`Release version validation failed:`);
    console.error(`- ${error instanceof Error ? error.message : error}`);
    process.exitCode = 1;
  }
}
