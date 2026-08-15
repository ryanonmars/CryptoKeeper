import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';

const versionPattern = /^\d+\.\d+\.\d+$/;
const sha256Pattern = /^[a-f0-9]{64}$/;

function renderFormula(version, sha256) {
  return `# typed: false
# frozen_string_literal: true

class Termkey < Formula
  desc "CLI encrypted storage for private keys and seed phrases"
  homepage "https://github.com/ryanonmars/termkey"
  url "https://github.com/ryanonmars/termkey/releases/download/v${version}/termkey-macos-aarch64.zip"
  version "${version}"
  sha256 "${sha256}"
  license "MIT"
  depends_on :macos
  depends_on arch: :arm64

  def install
    bin.install "termkey"
    libexec.install "termkey-native-host"
    pkgshare.install "browser-extension"
  end

  test do
    assert_match "Encrypted storage for", shell_output("#{bin}/termkey --help")
  end
end
`;
}

async function main() {
  const [version, sha256, outputPath] = process.argv.slice(2);
  if (!versionPattern.test(version ?? '')) {
    throw new Error(`invalid version: ${version ?? ''}`);
  }
  if (!sha256Pattern.test(sha256 ?? '')) {
    throw new Error(`invalid sha256: ${sha256 ?? ''}`);
  }
  if (!outputPath) {
    throw new Error('missing output path');
  }

  await mkdir(path.dirname(path.resolve(outputPath)), { recursive: true });
  await writeFile(outputPath, renderFormula(version, sha256));
}

main().catch((error) => {
  console.error(`render-homebrew-formula: ${error.message}`);
  process.exitCode = 1;
});
