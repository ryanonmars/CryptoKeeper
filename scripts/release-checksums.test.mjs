import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const scriptPath = new URL('./release-checksums.mjs', import.meta.url);
const repositoryRoot = path.resolve(path.dirname(scriptPath.pathname), '..');

async function createFixture() {
  const root = await mkdtemp(path.join(tmpdir(), 'termkey-checksums-'));
  const artifacts = path.join(root, 'artifacts');
  const output = path.join(root, 'SHA256SUMS');
  await mkdir(artifacts);
  return { artifacts, output };
}

async function createMacPackageFixture() {
  const root = await mkdtemp(path.join(tmpdir(), 'termkey-macos-package-'));
  const binaryPath = path.join(root, 'termkey');
  await writeFile(binaryPath, 'fixture binary');
  return binaryPath;
}

function runChecksumScript(input, output, expectedArtifacts) {
  return spawnSync(process.execPath, [
    scriptPath.pathname,
    input,
    output,
    ...expectedArtifacts,
  ], {
    encoding: 'utf8',
  });
}

function runMacPackageScript(binaryPath, environment) {
  return spawnSync('bash', [
    path.join(repositoryRoot, 'apps/cli/packaging/macos/create-pkg.sh'),
    binaryPath,
    '1.0.0',
    'termkey-macos-aarch64-installer',
    path.join(path.dirname(binaryPath), 'output'),
  ], {
    encoding: 'utf8',
    env: {
      ...process.env,
      ...environment,
    },
  });
}

test('writes deterministic SHA-256 lines in release-basename order', async () => {
  const { artifacts, output } = await createFixture();
  await mkdir(path.join(artifacts, 'windows'));
  await mkdir(path.join(artifacts, 'macos'));
  await writeFile(path.join(artifacts, 'windows', 'zeta.exe'), 'alpha');
  await writeFile(path.join(artifacts, 'macos', 'alpha.dmg'), 'beta');
  await writeFile(path.join(artifacts, 'middle.pkg'), 'gamma');
  await writeFile(path.join(artifacts, 'release.zip'), 'delta');
  await writeFile(path.join(artifacts, 'ignored.txt'), 'not an artifact');

  const expectedArtifacts = [
    'alpha.dmg',
    'middle.pkg',
    'release.zip',
    'zeta.exe',
  ];
  const first = runChecksumScript(artifacts, output, expectedArtifacts);
  assert.equal(first.status, 0, first.stderr);
  const firstOutput = await readFile(output, 'utf8');

  const second = runChecksumScript(artifacts, output, expectedArtifacts);
  assert.equal(second.status, 0, second.stderr);
  const secondOutput = await readFile(output, 'utf8');

  const expected = [
    'f44e64e75f3948e9f73f8dfa94721c4ce8cbb4f265c4790c702b2d41cfbf2753  alpha.dmg',
    'be9d587defa1f0c09ef49eb17e206983a5f8f8289e4281860bd0ee5a19592c67  middle.pkg',
    '4f4a9410ffcdf895c4adb880659e9b5c0dd1f23a30790684340b3eaacb045398  release.zip',
    '8ed3f6ad685b959ead7022518e1af76cd816f8e8ec7ccdda1ed4018e8f2223f8  zeta.exe',
    '',
  ].join('\n');

  assert.equal(firstOutput, expected);
  assert.equal(secondOutput, expected);
});

test('fails when the artifact directory is missing', async () => {
  const { artifacts, output } = await createFixture();
  const missing = path.join(artifacts, 'missing');

  const result = runChecksumScript(missing, output, ['release.zip']);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /artifact directory does not exist/i);
});

test('fails when the artifact directory has no supported artifacts', async () => {
  const { artifacts, output } = await createFixture();
  await writeFile(path.join(artifacts, 'notes.txt'), 'not a release artifact');

  const result = runChecksumScript(artifacts, output, ['release.zip']);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /no supported release artifacts/i);
});

test('fails when supported artifacts share a release-visible basename', async () => {
  const { artifacts, output } = await createFixture();
  await mkdir(path.join(artifacts, 'first'));
  await mkdir(path.join(artifacts, 'second'));
  await writeFile(path.join(artifacts, 'first', 'termkey.zip'), 'alpha');
  await writeFile(path.join(artifacts, 'second', 'termkey.zip'), 'beta');

  const result = runChecksumScript(artifacts, output, ['termkey.zip']);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /duplicate artifact basename.*termkey\.zip/i);
});

test('fails closed when an expected release artifact is missing', async () => {
  const { artifacts, output } = await createFixture();
  await writeFile(path.join(artifacts, 'termkey-linux-x86_64.zip'), 'linux');

  const result = runChecksumScript(artifacts, output, [
    'termkey-linux-x86_64.zip',
    'termkey-windows-x86_64.zip',
  ]);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /missing expected artifacts.*termkey-windows-x86_64\.zip/i);
});

test('fails closed when an unexpected release artifact is present', async () => {
  const { artifacts, output } = await createFixture();
  await writeFile(path.join(artifacts, 'termkey-linux-x86_64.zip'), 'linux');
  await writeFile(path.join(artifacts, 'unreviewed-installer.exe'), 'extra');

  const result = runChecksumScript(artifacts, output, [
    'termkey-linux-x86_64.zip',
  ]);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /unexpected artifacts.*unreviewed-installer\.exe/i);
});

test('macOS packaging accepts only arm64 and validates its release contract', async () => {
  const binaryPath = await createMacPackageFixture();

  const unsupportedArchitecture = runMacPackageScript(binaryPath, {
    TERMKEY_MACOS_ARCH: 'x86_64',
    MACOSX_DEPLOYMENT_TARGET: '11.0',
    TERMKEY_RELEASE_SIGNING: 'disabled',
  });
  assert.notEqual(unsupportedArchitecture.status, 0);
  assert.match(unsupportedArchitecture.stderr, /unsupported macOS architecture: x86_64/);

  const unsupportedDeploymentTarget = runMacPackageScript(binaryPath, {
    TERMKEY_MACOS_ARCH: 'arm64',
    MACOSX_DEPLOYMENT_TARGET: '12.0',
    TERMKEY_RELEASE_SIGNING: 'disabled',
  });
  assert.notEqual(unsupportedDeploymentTarget.status, 0);
  assert.match(unsupportedDeploymentTarget.stderr, /MACOSX_DEPLOYMENT_TARGET must be 11\.0/);

  const invalidSigningMode = runMacPackageScript(binaryPath, {
    TERMKEY_MACOS_ARCH: 'arm64',
    MACOSX_DEPLOYMENT_TARGET: '11.0',
    TERMKEY_RELEASE_SIGNING: 'unexpected',
  });
  assert.notEqual(invalidSigningMode.status, 0);
  assert.match(invalidSigningMode.stderr, /invalid TERMKEY_RELEASE_SIGNING value: unexpected/);
});

test('macOS packaging requires Developer ID signing identities', async () => {
  const binaryPath = await createMacPackageFixture();

  const invalidApplicationIdentity = runMacPackageScript(binaryPath, {
    TERMKEY_MACOS_ARCH: 'arm64',
    MACOSX_DEPLOYMENT_TARGET: '11.0',
    TERMKEY_RELEASE_SIGNING: 'required',
    APPLE_APPLICATION_SIGNING_IDENTITY: 'Apple Development: TermKey (ABCDE12345)',
    APPLE_INSTALLER_SIGNING_IDENTITY: 'Developer ID Installer: TermKey (ABCDE12345)',
  });
  assert.notEqual(invalidApplicationIdentity.status, 0);
  assert.match(
    invalidApplicationIdentity.stderr,
    /APPLE_APPLICATION_SIGNING_IDENTITY must be a Developer ID Application identity/,
  );

  const invalidInstallerIdentity = runMacPackageScript(binaryPath, {
    TERMKEY_MACOS_ARCH: 'arm64',
    MACOSX_DEPLOYMENT_TARGET: '11.0',
    TERMKEY_RELEASE_SIGNING: 'required',
    APPLE_APPLICATION_SIGNING_IDENTITY: 'Developer ID Application: TermKey (ABCDE12345)',
    APPLE_INSTALLER_SIGNING_IDENTITY: 'Mac Developer Installer: TermKey (ABCDE12345)',
  });
  assert.notEqual(invalidInstallerIdentity.status, 0);
  assert.match(
    invalidInstallerIdentity.stderr,
    /APPLE_INSTALLER_SIGNING_IDENTITY must be a Developer ID Installer identity/,
  );
});

test('release workflow requires the exact artifact manifest', async () => {
  const releaseWorkflow = await readFile(
    path.join(repositoryRoot, '.github/workflows/release.yml'),
    'utf8',
  );
  const expectedArtifacts = [
    'termkey-linux-x86_64.zip',
    'termkey-macos-x86_64.zip',
    'termkey-macos-x86_64-installer.pkg',
    'termkey-macos-x86_64.dmg',
    'termkey-macos-aarch64.zip',
    'termkey-macos-aarch64-installer.pkg',
    'termkey-macos-aarch64.dmg',
    'termkey-windows-x86_64.zip',
  ];

  for (const artifact of expectedArtifacts) {
    assert.match(releaseWorkflow, new RegExp(`\\b${artifact.replaceAll('.', '\\.')}\\b`));
  }
  assert.equal(
    [...releaseWorkflow.matchAll(/uses: actions\/upload-artifact@/g)].length,
    [...releaseWorkflow.matchAll(/if-no-files-found: error/g)].length,
  );
  assert.match(releaseWorkflow, /fail_on_unmatched_files: true/);
  assert.match(
    releaseWorkflow,
    /node scripts\/release-checksums\.mjs artifacts artifacts\/SHA256SUMS[\s\S]*termkey-linux-x86_64\.zip[\s\S]*termkey-windows-x86_64\.zip/,
  );
});
