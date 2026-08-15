import assert from 'node:assert/strict';
import { chmod, mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import test from 'node:test';

const scriptPath = new URL('./release-workflow.test.mjs', import.meta.url);
const repositoryRoot = path.resolve(path.dirname(scriptPath.pathname), '..');
const workflowPath = path.join(repositoryRoot, '.github/workflows/release.yml');
const packageJsonPath = path.join(repositoryRoot, 'package.json');
const packageScriptPath = path.join(
  repositoryRoot,
  'apps/cli/packaging/macos/create-pkg.sh',
);
const extensionManifestPath = path.join(repositoryRoot, 'apps/extension/manifest.json');
const publicExtensionManifestPath = path.join(
  repositoryRoot,
  'apps/extension/public/manifest.json',
);
const nativeManifestTemplatePath = path.join(
  repositoryRoot,
  'apps/cli/native-messaging/com.ryanonmars.termkey.template.json',
);
const browserCommandPath = path.join(
  repositoryRoot,
  'apps/cli/src/commands/browser.rs',
);
const actionReleasePin = 'softprops/action-gh-release@3bb12739c298aeb8a4eeaf626c5b8d85266b0e65';
const appleSecrets = [
  'APPLE_APPLICATION_SIGNING_IDENTITY',
  'APPLE_CERTIFICATE_BASE64',
  'APPLE_CERTIFICATE_PASSWORD',
  'APPLE_INSTALLER_SIGNING_IDENTITY',
  'APPLE_NOTARY_ISSUER_ID',
  'APPLE_NOTARY_KEY_BASE64',
  'APPLE_NOTARY_KEY_ID',
  'APPLE_TEAM_ID',
];

function parseWorkflow() {
  const ruby = [
    "require 'yaml'",
    "require 'json'",
    'workflow = YAML.safe_load(File.read(ARGV.fetch(0)), aliases: true)',
    "workflow['on'] = workflow.delete(true) if workflow.key?(true)",
    'puts JSON.generate(workflow)',
  ].join('; ');
  const result = spawnSync('ruby', ['-e', ruby, workflowPath], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  return JSON.parse(result.stdout);
}

const workflow = parseWorkflow();

const fixtures = {
  production: {
    github: {
      event_name: 'push',
      ref: 'refs/tags/v1.0.0',
      ref_name: 'v1.0.0',
    },
    inputs: { trusted_build: false },
    mode: 'tag-production',
    trusted: 'true',
    publish: 'true',
  },
  trusted: {
    github: {
      event_name: 'workflow_dispatch',
      ref: 'refs/heads/release-validation',
      ref_name: 'release-validation',
    },
    inputs: { trusted_build: true },
    mode: 'trusted-validation',
    trusted: 'true',
    publish: 'false',
  },
  unsigned: {
    github: {
      event_name: 'workflow_dispatch',
      ref: 'refs/heads/feature',
      ref_name: 'feature',
    },
    inputs: { trusted_build: false },
    mode: 'unsigned-dry-run',
    trusted: 'false',
    publish: 'false',
  },
};

function expressionBody(value) {
  assert.equal(typeof value, 'string');
  const match = value.trim().match(/^\$\{\{([\s\S]*)\}\}$/);
  return match ? match[1].trim() : value.trim();
}

function evaluateExpression(value, fixture, env = {}) {
  const body = expressionBody(value);
  const evaluate = Function(
    'github',
    'inputs',
    'env',
    'startsWith',
    'always',
    `"use strict"; return (${body});`,
  );
  return evaluate(
    fixture.github,
    fixture.inputs,
    env,
    (valueToCheck, prefix) => String(valueToCheck).startsWith(prefix),
    () => true,
  );
}

function evaluatedWorkflowEnv(fixture) {
  return Object.fromEntries(
    ['TERMKEY_TRUSTED_BUILD', 'TERMKEY_PUBLISH_RELEASE', 'TERMKEY_RELEASE_MODE'].map(
      (name) => [name, String(evaluateExpression(workflow.env[name], fixture))],
    ),
  );
}

function evaluateCondition(value, fixture, env = evaluatedWorkflowEnv(fixture)) {
  if (value === undefined) {
    return true;
  }
  return Boolean(evaluateExpression(value, fixture, env));
}

function renderTemplate(value, fixture, env = evaluatedWorkflowEnv(fixture)) {
  return value.replace(/\$\{\{([\s\S]*?)\}\}/g, (expression) =>
    String(evaluateExpression(expression, fixture, env)),
  );
}

function findStep(jobName, stepName) {
  const job = workflow.jobs[jobName];
  assert.ok(job, `missing workflow job: ${jobName}`);
  const step = job.steps.find((candidate) => candidate.name === stepName);
  assert.ok(step, `missing workflow step: ${jobName} / ${stepName}`);
  return step;
}

function lines(value) {
  return String(value)
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean);
}

function secretNames(step) {
  return Object.values(step.env ?? {}).flatMap((value) => {
    const match = String(value).match(/^\$\{\{\s*secrets\.([A-Z0-9_]+)\s*\}\}$/);
    return match ? [match[1]] : [];
  });
}

function runBlock(run, environment) {
  return spawnSync('bash', ['--noprofile', '--norc', '-e', '-o', 'pipefail', '-c', run], {
    encoding: 'utf8',
    env: { ...process.env, ...environment },
  });
}

async function writeExecutable(filePath, body) {
  await writeFile(filePath, `${body}\n`);
  await chmod(filePath, 0o755);
}

test('manual input and mode expressions separate trust from publication', () => {
  const dispatch = workflow.on.workflow_dispatch;
  assert.deepEqual(dispatch.inputs.trusted_build, {
    description: 'Sign, notarize, and verify without publishing',
    required: true,
    default: false,
    type: 'boolean',
  });

  for (const fixture of Object.values(fixtures)) {
    const env = evaluatedWorkflowEnv(fixture);
    assert.equal(env.TERMKEY_TRUSTED_BUILD, fixture.trusted);
    assert.equal(env.TERMKEY_PUBLISH_RELEASE, fixture.publish);
    assert.equal(env.TERMKEY_RELEASE_MODE, fixture.mode);
  }
});

test('trusted builds use the protected validation environment and unsigned dispatches run no secret-bearing step', () => {
  const build = workflow.jobs.build;
  assert.ok(build);
  const environmentName =
    typeof build.environment === 'string' ? build.environment : build.environment.name;
  assert.equal(evaluateExpression(environmentName, fixtures.production), 'apple-release-validation');
  assert.equal(evaluateExpression(environmentName, fixtures.trusted), 'apple-release-validation');
  assert.equal(evaluateExpression(environmentName, fixtures.unsigned), 'unsigned-dry-run');

  const configuredAppleSecrets = new Set(
    build.steps.flatMap((step) => secretNames(step)).filter((name) => name.startsWith('APPLE_')),
  );
  assert.deepEqual([...configuredAppleSecrets].sort(), appleSecrets);

  for (const [jobName, job] of Object.entries(workflow.jobs)) {
    const jobRunsUnsigned = evaluateCondition(job.if, fixtures.unsigned);
    for (const step of job.steps) {
      const secrets = secretNames(step);
      if (jobRunsUnsigned && evaluateCondition(step.if, fixtures.unsigned)) {
        assert.deepEqual(
          secrets,
          [],
          `unsigned dispatch reaches secrets in ${jobName} / ${step.name ?? step.uses}`,
        );
      }
    }
  }

  assert.deepEqual(
    workflow.jobs['validate-tap'].steps.flatMap((step) => secretNames(step)),
    [],
  );
});

test('job, artifact, and summary labels identify all three release modes', async () => {
  const build = workflow.jobs.build;
  const upload = findStep('build', 'Upload release artifacts');
  const summary = findStep('build', 'Summarize release mode');
  const expectedPaths = [
    'termkey-macos-aarch64.dmg',
    'termkey-macos-aarch64.zip',
  ];
  assert.deepEqual(lines(upload.with.path), expectedPaths);

  for (const fixture of Object.values(fixtures)) {
    assert.match(renderTemplate(build.name, fixture), new RegExp(`${fixture.mode}$`));
    assert.equal(
      renderTemplate(upload.with.name, fixture),
      `termkey-macos-aarch64-${fixture.mode}`,
    );

    const root = await mkdtemp(path.join(tmpdir(), 'termkey-release-summary-'));
    const summaryPath = path.join(root, 'summary.md');
    const result = runBlock(summary.run, {
      GITHUB_STEP_SUMMARY: summaryPath,
      TERMKEY_RELEASE_MODE: fixture.mode,
    });
    assert.equal(result.status, 0, result.stderr);
    assert.match(await readFile(summaryPath, 'utf8'), new RegExp(`^## Release mode: ${fixture.mode}$`, 'm'));
  }

  assert.match(workflow.jobs.release.name, /tag-production/);
  assert.match(workflow.jobs['update-tap'].name, /tag-production/);
  assert.match(renderTemplate(workflow.jobs['validate-tap'].name, fixtures.unsigned), /unsigned-dry-run/);
  assert.match(renderTemplate(workflow.jobs['validate-tap'].name, fixtures.trusted), /trusted-validation/);
});

test('production packages exclude the unpacked extension while Store ZIP generation remains validated', async () => {
  const validate = findStep('validate', 'Validate');
  const createZip = findStep('build', 'Create ZIP from release payload');
  const buildCommands = workflow.jobs.build.steps
    .map((step) => step.run ?? '')
    .join('\n');
  const packageScript = await readFile(packageScriptPath, 'utf8');
  const packageJson = JSON.parse(await readFile(packageJsonPath, 'utf8'));

  assert.equal(validate.run, 'npm run validate:phase1');
  assert.match(packageJson.scripts['validate:phase1'], /npm run package:extension/);
  assert.match(createZip.run, /release_dir\/termkey" "\$staging_dir\/termkey"/);
  assert.match(
    createZip.run,
    /release_dir\/termkey-native-host" "\$staging_dir\/termkey-native-host"/,
  );
  assert.doesNotMatch(buildCommands, /browser-extension|TERMKEY_EXTENSION_DIR|apps\/extension/);
  assert.doesNotMatch(packageScript, /browser-extension|TERMKEY_EXTENSION_DIR|apps\/extension/);
});

test('the Store key, extension ID, native origin, and Store URL stay consistent', async () => {
  const manifest = JSON.parse(await readFile(extensionManifestPath, 'utf8'));
  const publicManifest = JSON.parse(
    await readFile(publicExtensionManifestPath, 'utf8'),
  );
  const nativeTemplate = JSON.parse(
    await readFile(nativeManifestTemplatePath, 'utf8'),
  );
  const browserCommand = await readFile(browserCommandPath, 'utf8');
  const digest = createHash('sha256')
    .update(Buffer.from(manifest.key, 'base64'))
    .digest()
    .subarray(0, 16);
  const extensionId = [...digest]
    .map((byte) =>
      String.fromCharCode(97 + (byte >> 4), 97 + (byte & 0x0f)),
    )
    .join('');

  assert.equal(extensionId, 'dancadidkgcdlfdlfpbmmiokkeedpini');
  assert.equal(publicManifest.key, manifest.key);
  assert.deepEqual(nativeTemplate.allowed_origins, [
    `chrome-extension://${extensionId}/`,
  ]);
  assert.match(
    browserCommand,
    new RegExp(`CHROME_EXTENSION_ID: &str = "${extensionId}"`),
  );
  assert.match(
    browserCommand,
    new RegExp(`https://chromewebstore\\.google\\.com/detail/${extensionId}`),
  );
});

test('only v-tag pushes can create a release or update the tap', () => {
  for (const fixture of Object.values(fixtures)) {
    assert.equal(evaluateCondition(workflow.jobs.release.if, fixture), fixture.publish === 'true');
    assert.equal(
      evaluateCondition(workflow.jobs['update-tap'].if, fixture),
      fixture.publish === 'true',
    );
  }
  assert.equal(workflow.jobs['update-tap'].needs, 'release');
});

test('release action creates a draft with the exact public asset manifest', () => {
  const step = findStep('release', 'Create draft Release');
  assert.equal(step.id, 'create-draft');
  assert.equal(step.uses, actionReleasePin);
  assert.equal(step.with.draft, true);
  assert.equal(step.with.generate_release_notes, true);
  assert.equal(step.with.fail_on_unmatched_files, true);
  assert.deepEqual(lines(step.with.files).sort(), [
    'artifacts/SHA256SUMS',
    'artifacts/termkey-macos-aarch64/termkey-macos-aarch64.dmg',
    'artifacts/termkey-macos-aarch64/termkey-macos-aarch64.zip',
  ]);
});

async function createGhFixture({ releaseTags = '', release }) {
  const root = await mkdtemp(path.join(tmpdir(), 'termkey-release-api-'));
  const bin = path.join(root, 'bin');
  const log = path.join(root, 'gh.log');
  const releaseJson = path.join(root, 'release.json');
  await mkdir(bin);
  await writeFile(log, '');
  await writeFile(releaseJson, `${JSON.stringify(release ?? {})}\n`);
  await writeExecutable(
    path.join(bin, 'gh'),
    [
      '#!/usr/bin/env bash',
      'set -euo pipefail',
      'printf "gh" >> "$TERMKEY_TEST_GH_LOG"',
      'printf "\\t%s" "$@" >> "$TERMKEY_TEST_GH_LOG"',
      'printf "\\n" >> "$TERMKEY_TEST_GH_LOG"',
      'if [[ " $* " == *" --paginate "* ]]; then',
      '  printf "%s" "${TERMKEY_TEST_RELEASE_TAGS:-}"',
      'elif [[ " $* " == *" --method PATCH "* ]]; then',
      '  printf "%s\\n" false',
      'elif [[ " $* " == *"/releases/tags/"* ]]; then',
      '  echo "drafts are not available through the tag endpoint" >&2',
      '  exit 44',
      'elif [[ " $* " == *"/releases/$RELEASE_ID "* ]]; then',
      '  cat "$TERMKEY_TEST_RELEASE_JSON"',
      'else',
      '  exit 64',
      'fi',
    ].join('\n'),
  );
  return {
    root,
    log,
    environment: {
      GH_TOKEN: 'fixture-token',
      GITHUB_REF_NAME: 'v1.0.0',
      GITHUB_REPOSITORY: 'owner/repository',
      GITHUB_RUN_ID: '1234',
      PATH: `${bin}:${process.env.PATH}`,
      RELEASE_ID: '42',
      RUNNER_TEMP: root,
      TERMKEY_TEST_GH_LOG: log,
      TERMKEY_TEST_RELEASE_JSON: releaseJson,
      TERMKEY_TEST_RELEASE_TAGS: releaseTags,
    },
  };
}

test('release preflight fails when any existing release or draft has the tag', async () => {
  const step = findStep('release', 'Fail if release tag already exists');
  const collision = await createGhFixture({ releaseTags: 'v0.9.0\nv1.0.0\n' });
  const rejected = runBlock(step.run, collision.environment);
  assert.notEqual(rejected.status, 0);
  assert.match(rejected.stderr, /already exists/i);

  const available = await createGhFixture({ releaseTags: 'v0.9.0\n' });
  const accepted = runBlock(step.run, available.environment);
  assert.equal(accepted.status, 0, accepted.stderr);
});

const exactRelease = {
  id: 42,
  tag_name: 'v1.0.0',
  draft: true,
  assets: [
    { name: 'termkey-macos-aarch64.zip' },
    { name: 'SHA256SUMS' },
    { name: 'termkey-macos-aarch64.dmg' },
  ],
};

test('draft verification publishes only an exact three-asset draft', async () => {
  const step = findStep('release', 'Verify and publish draft Release');
  const exact = await createGhFixture({ release: exactRelease });
  const published = runBlock(step.run, exact.environment);
  assert.equal(published.status, 0, published.stderr);
  assert.equal((await readFile(exact.log, 'utf8')).match(/--method\tPATCH/g)?.length, 1);

  const mismatched = await createGhFixture({
    release: {
      ...exactRelease,
      assets: [{ name: 'termkey-macos-aarch64.zip' }, { name: 'SHA256SUMS' }],
    },
  });
  const rejectedAssets = runBlock(step.run, mismatched.environment);
  assert.notEqual(rejectedAssets.status, 0);
  assert.doesNotMatch(await readFile(mismatched.log, 'utf8'), /--method\tPATCH/);

  const alreadyPublished = await createGhFixture({
    release: { ...exactRelease, draft: false },
  });
  const rejectedPublished = runBlock(step.run, alreadyPublished.environment);
  assert.notEqual(rejectedPublished.status, 0);
  assert.doesNotMatch(await readFile(alreadyPublished.log, 'utf8'), /--method\tPATCH/);
});

async function createSecurityFixture(originalKeychains) {
  const root = await mkdtemp(path.join(tmpdir(), 'termkey-keychain-'));
  const bin = path.join(root, 'bin');
  const log = path.join(root, 'security.log');
  const githubEnv = path.join(root, 'github.env');
  const keychains = path.join(root, 'security-list.txt');
  await mkdir(bin);
  await writeFile(log, '');
  await writeFile(githubEnv, '');
  await writeFile(
    keychains,
    originalKeychains.map((keychain) => `    "${keychain}"`).join('\n') +
      (originalKeychains.length > 0 ? '\n' : ''),
  );
  await writeExecutable(
    path.join(bin, 'security'),
    [
      '#!/usr/bin/env bash',
      'set -euo pipefail',
      'printf "security" >> "$TERMKEY_TEST_SECURITY_LOG"',
      'printf "\\t%s" "$@" >> "$TERMKEY_TEST_SECURITY_LOG"',
      'printf "\\n" >> "$TERMKEY_TEST_SECURITY_LOG"',
      'case "$1" in',
      '  list-keychains)',
      '    if [[ " $* " != *" -s "* && " ${*: -1} " != " -s " ]]; then',
      '      cat "$TERMKEY_TEST_SECURITY_LIST"',
      '    fi',
      '    ;;',
      '  find-identity)',
      '    if [[ " $* " == *" -p codesigning "* ]]; then',
      '      printf "%s\\n" "  1) AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA \\"Developer ID Application: TermKey (TEAM123456)\\""',
      '    else',
      '      printf "%s\\n" "  1) BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB \\"Developer ID Installer: TermKey (TEAM123456)\\""',
      '    fi',
      '    ;;',
      '  create-keychain|set-keychain-settings|unlock-keychain|import|set-key-partition-list|delete-keychain) ;;',
      '  *) exit 64 ;;',
      'esac',
    ].join('\n'),
  );
  return {
    root,
    log,
    githubEnv,
    environment: {
      APPLE_APPLICATION_SIGNING_IDENTITY:
        'Developer ID Application: TermKey (TEAM123456)',
      APPLE_CERTIFICATE_BASE64: Buffer.from('certificate').toString('base64'),
      APPLE_CERTIFICATE_PASSWORD: 'certificate-password',
      APPLE_INSTALLER_SIGNING_IDENTITY: 'Developer ID Installer: TermKey (TEAM123456)',
      APPLE_NOTARY_KEY_BASE64: Buffer.from('notary-key').toString('base64'),
      APPLE_NOTARY_KEY_ID: 'NOTARY1234',
      APPLE_TEAM_ID: 'TEAM123456',
      GITHUB_ENV: githubEnv,
      PATH: `${bin}:${process.env.PATH}`,
      RUNNER_TEMP: root,
      TERMKEY_TEST_SECURITY_LIST: keychains,
      TERMKEY_TEST_SECURITY_LOG: log,
    },
  };
}

async function readGithubEnv(filePath) {
  const entries = (await readFile(filePath, 'utf8'))
    .split('\n')
    .filter(Boolean)
    .map((line) => {
      const separator = line.indexOf('=');
      return [line.slice(0, separator), line.slice(separator + 1)];
    });
  return Object.fromEntries(entries);
}

test('credential setup trims original keychains, persists them, and appends the temporary keychain', async () => {
  const step = findStep('build', 'Configure ephemeral Apple credentials');
  const originals = [
    '/Users/runner/Library/Keychains/login.keychain-db',
    '/Users/runner/Library/Keychains/Work Keychain.keychain-db',
  ];
  const fixture = await createSecurityFixture(originals);
  const result = runBlock(step.run, fixture.environment);
  assert.equal(result.status, 0, result.stderr);

  const exported = await readGithubEnv(fixture.githubEnv);
  assert.equal(path.dirname(exported.APPLE_ORIGINAL_KEYCHAINS_PATH), fixture.root);
  assert.equal(
    await readFile(exported.APPLE_ORIGINAL_KEYCHAINS_PATH, 'utf8'),
    `${originals.join('\n')}\n`,
  );
  const log = await readFile(fixture.log, 'utf8');
  assert.match(
    log,
    new RegExp(
      `^security\\tlist-keychains\\t-d\\tuser\\t-s\\t${originals[0]}\\t${originals[1]}\\t${exported.APPLE_KEYCHAIN_PATH}$`,
      'm',
    ),
  );
});

test('always cleanup restores original keychains before deleting the temporary keychain', async () => {
  const step = findStep('build', 'Remove Apple credentials');
  const originals = [
    '/Users/runner/Library/Keychains/login.keychain-db',
    '/Users/runner/Library/Keychains/Work Keychain.keychain-db',
  ];
  const fixture = await createSecurityFixture([]);
  const originalList = path.join(fixture.root, 'termkey-original-keychains.txt');
  const temporaryKeychain = path.join(fixture.root, 'termkey-signing.keychain-db');
  const certificate = path.join(fixture.root, 'termkey-signing.p12');
  const notaryKey = path.join(fixture.root, 'AuthKey_NOTARY1234.p8');
  await writeFile(originalList, `${originals.join('\n')}\n`);
  await writeFile(certificate, 'certificate');
  await writeFile(notaryKey, 'notary-key');

  const result = runBlock(step.run, {
    ...fixture.environment,
    APPLE_CERTIFICATE_PATH: certificate,
    APPLE_KEYCHAIN_PATH: temporaryKeychain,
    APPLE_ORIGINAL_KEYCHAINS_PATH: originalList,
  });
  assert.equal(result.status, 0, result.stderr);
  const logLines = lines(await readFile(fixture.log, 'utf8'));
  const restore = [
    'security',
    'list-keychains',
    '-d',
    'user',
    '-s',
    ...originals,
  ].join('\t');
  const deletion = ['security', 'delete-keychain', temporaryKeychain].join('\t');
  assert.ok(logLines.indexOf(restore) >= 0, 'original keychain search list was not restored');
  assert.ok(logLines.indexOf(deletion) > logLines.indexOf(restore), 'temporary keychain deleted before restoration');
});

test('zero original keychains are persisted and restored as an empty search list', async () => {
  const configure = findStep('build', 'Configure ephemeral Apple credentials');
  const cleanup = findStep('build', 'Remove Apple credentials');
  const fixture = await createSecurityFixture([]);
  const configured = runBlock(configure.run, fixture.environment);
  assert.equal(configured.status, 0, configured.stderr);
  const exported = await readGithubEnv(fixture.githubEnv);
  assert.equal(await readFile(exported.APPLE_ORIGINAL_KEYCHAINS_PATH, 'utf8'), '');

  const cleaned = runBlock(cleanup.run, { ...fixture.environment, ...exported });
  assert.equal(cleaned.status, 0, cleaned.stderr);
  const log = await readFile(fixture.log, 'utf8');
  assert.match(log, /^security\tlist-keychains\t-d\tuser\t-s$/m);
});
