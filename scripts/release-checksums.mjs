import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, readdir, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';

const supportedExtensions = new Set(['.zip', '.exe', '.pkg', '.dmg']);

async function collectArtifacts(directory) {
  const artifacts = [];
  const entries = await readdir(directory, { withFileTypes: true });

  for (const entry of entries) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      artifacts.push(...await collectArtifacts(entryPath));
    } else if (
      entry.isFile()
      && supportedExtensions.has(path.extname(entry.name).toLowerCase())
    ) {
      artifacts.push(entryPath);
    }
  }

  return artifacts;
}

async function sha256(filePath) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(filePath)) {
    hash.update(chunk);
  }
  return hash.digest('hex');
}

async function main() {
  const [inputDirectory, outputFile, ...expectedArtifacts] = process.argv.slice(2);
  if (!inputDirectory || !outputFile || expectedArtifacts.length === 0) {
    throw new Error(
      'usage: node scripts/release-checksums.mjs <artifact-directory> <output-file> <expected-artifact...>',
    );
  }

  const expectedBasenames = new Set();
  for (const expectedArtifact of expectedArtifacts) {
    if (
      !expectedArtifact
      || expectedArtifact.includes('/')
      || expectedArtifact.includes('\\')
      || !supportedExtensions.has(path.extname(expectedArtifact).toLowerCase())
    ) {
      throw new Error(`invalid expected artifact basename: ${expectedArtifact}`);
    }
    if (expectedBasenames.has(expectedArtifact)) {
      throw new Error(`duplicate expected artifact basename: ${expectedArtifact}`);
    }
    expectedBasenames.add(expectedArtifact);
  }

  let inputStat;
  try {
    inputStat = await stat(inputDirectory);
  } catch (error) {
    if (error.code === 'ENOENT') {
      throw new Error(`artifact directory does not exist: ${inputDirectory}`);
    }
    throw error;
  }

  if (!inputStat.isDirectory()) {
    throw new Error(`artifact input is not a directory: ${inputDirectory}`);
  }

  const artifacts = await collectArtifacts(inputDirectory);
  if (artifacts.length === 0) {
    throw new Error(`no supported release artifacts found in: ${inputDirectory}`);
  }

  const byBasename = new Map();
  for (const artifact of artifacts) {
    const basename = path.basename(artifact);
    if (byBasename.has(basename)) {
      throw new Error(`duplicate artifact basename: ${basename}`);
    }
    byBasename.set(basename, artifact);
  }

  const actualBasenames = new Set(byBasename.keys());
  const missingArtifacts = [...expectedBasenames]
    .filter((basename) => !actualBasenames.has(basename))
    .sort();
  const unexpectedArtifacts = [...actualBasenames]
    .filter((basename) => !expectedBasenames.has(basename))
    .sort();
  if (missingArtifacts.length > 0 || unexpectedArtifacts.length > 0) {
    const problems = [];
    if (missingArtifacts.length > 0) {
      problems.push(`missing expected artifacts: ${missingArtifacts.join(', ')}`);
    }
    if (unexpectedArtifacts.length > 0) {
      problems.push(`unexpected artifacts: ${unexpectedArtifacts.join(', ')}`);
    }
    throw new Error(problems.join('; '));
  }

  const basenames = [...byBasename.keys()].sort();
  const lines = [];
  for (const basename of basenames) {
    lines.push(`${await sha256(byBasename.get(basename))}  ${basename}`);
  }

  await mkdir(path.dirname(path.resolve(outputFile)), { recursive: true });
  await writeFile(outputFile, `${lines.join('\n')}\n`);
}

main().catch((error) => {
  console.error(`release-checksums: ${error.message}`);
  process.exitCode = 1;
});
