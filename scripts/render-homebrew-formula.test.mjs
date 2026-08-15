import assert from 'node:assert/strict';
import { access, mkdtemp, readFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';

const scriptPath = new URL('./render-homebrew-formula.mjs', import.meta.url);
const validSha = 'a'.repeat(64);

async function createFixture() {
  const root = await mkdtemp(path.join(tmpdir(), 'termkey-homebrew-formula-'));
  return { root, output: path.join(root, 'Formula', 'termkey.rb') };
}

function render(version, sha256, output) {
  return spawnSync(process.execPath, [scriptPath.pathname, version, sha256, output], {
    encoding: 'utf8',
  });
}

async function outputExists(output) {
  try {
    await access(output);
    return true;
  } catch {
    return false;
  }
}

test('renders a deterministic Apple-Silicon-only formula', async () => {
  const { output } = await createFixture();

  const result = render('1.0.1', validSha, output);

  assert.equal(result.status, 0, result.stderr);
  const formula = await readFile(output, 'utf8');
  assert.match(formula, /url "https:\/\/github\.com\/ryanonmars\/termkey\/releases\/download\/v1\.0\.1\/termkey-macos-aarch64\.zip"/);
  assert.match(formula, /version "1\.0\.1"/);
  assert.match(formula, new RegExp(`sha256 "${validSha}"`));
  assert.match(formula, /license "MIT"/);
  assert.match(formula, /depends_on :macos/);
  assert.match(formula, /depends_on arch: :arm64/);
  assert.match(formula, /bin\.install "termkey"/);
  assert.match(formula, /libexec\.install "termkey-native-host"/);
  assert.doesNotMatch(formula, /browser-extension|pkgshare/);
  assert.match(formula, /assert_match "Encrypted storage for", shell_output\("#\{bin\}\/termkey --help"\)/);
  assert.doesNotMatch(formula, /linux|x86_64|on_intel|on_linux/i);
  assert.match(formula, /\n$/);
});

test('rejects an invalid version before writing the output', async () => {
  const { output } = await createFixture();

  const result = render('v1.0.1', validSha, output);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /invalid version/i);
  assert.equal(await outputExists(output), false);
});

test('rejects an invalid SHA-256 before writing the output', async () => {
  const { output } = await createFixture();

  const result = render('1.0.1', 'A'.repeat(64), output);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /invalid sha256/i);
  assert.equal(await outputExists(output), false);
});
