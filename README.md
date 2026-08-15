<p align="center">
  <img src="apps/cli/assets/branding/termkey-icon.svg" alt="TermKey" width="420">
</p>

# TermKey

Local-only, encrypted vault for private keys, seed phrases, and passwords. Run `termkey` for the full-screen TUI, or use subcommands for direct terminal workflows. **XChaCha20-Poly1305** + **Argon2id**. Zero cloud. Zero trust.

- **Vault path:** `~/.termkey/`
- **Secret types:** private keys, seed phrases, passwords
- **Networks:** Ethereum, Bitcoin, Solana, or a custom network label
- **Extras:** encrypted backup/import, optional random recovery phrase, strong password generation for password entries, address derivation for supported crypto entries

---

## Security

| | |
|---|---|
| **XChaCha20-Poly1305** | AEAD cipher with a 192-bit nonce for authenticated encryption |
| **Argon2id** | Memory-hard KDF for deriving encryption keys from your master password |
| **Local-only storage** | Vault data lives under `~/.termkey/` with no cloud sync or remote service |

---

## Install

### Apple Silicon macOS 11 or later

TermKey Phase 1 supports Apple Silicon Macs running macOS 11 or later.

**Signed and notarized DMG:** [Download TermKey for Apple Silicon](https://github.com/ryanonmars/termkey/releases/latest/download/termkey-macos-aarch64.dmg). Open the DMG and follow its installation instructions.

**Homebrew:**

```bash
brew install ryanonmars/termkey/termkey
```

Run `termkey` to open the vault. `termkey update` checks the latest TermKey release; for a Homebrew install it runs the required Homebrew upgrade flow.

---

## Chrome Extension Setup (Phase 1)

The Chrome Web Store listing is not available yet. Phase 1 uses a temporary unpacked-extension installation:

1. Run `termkey browser install`.
2. Open `chrome://extensions`.
3. Turn on **Developer mode**.
4. Click **Load unpacked**.
5. Select the exact folder printed by `termkey browser install`.

Useful commands:

```bash
termkey browser install
termkey browser status
termkey browser repair
```

`termkey browser install` copies the bundled extension to `~/Applications/TermKey Browser Extension`, installs the local Chrome native-host manifest, and prints the folder to load. `termkey browser status` reports the integration state, and `termkey browser repair` restores the native-host registration and staged extension when needed.

Read the [Privacy Policy](https://github.com/ryanonmars/termkey/blob/master/PRIVACY.md) before enabling autofill or save prompts.

---

## How It Works

Run `termkey` with no subcommand to open the TUI.

When terminal graphics are supported, TermKey shows a short pre-TUI splash icon before entering the fullscreen app. The current best-effort auto-detected targets are Kitty and Ghostty via the Kitty graphics protocol, plus iTerm2 via its OSC 1337 inline-image protocol. iTerm2 may prompt for permission before displaying inline images. `tmux` is disabled by default because redraws can wipe terminal images. Override detection with `TERMKEY_GRAPHICS=kitty` or `TERMKEY_GRAPHICS=iterm2`, disable it with `TERMKEY_GRAPHICS=off`, and adjust the delay with `TERMKEY_SPLASH_MS=<milliseconds>`.

On first launch, TermKey opens a setup wizard where you:

1. Create your master password
2. Create the local vault
3. Optionally set up a random 24-word recovery phrase

After setup, you land on the login screen and then the dashboard.

The recovery phrase is shown once. Store it offline and never share it. Vaults
upgraded from the legacy format no longer use security-question recovery; after
the first successful upgrade, follow the unlock notice and configure a new
recovery phrase in Settings.

Inside the TUI you can:

- Add private keys, seed phrases, or passwords
- Generate strong passwords while creating password entries
- Search and filter entries in place
- View secrets or copy them to the clipboard
- Edit, rename, and delete entries
- Export an encrypted backup and import it later
- Change your master password
- Open settings and recovery flows

For crypto entries, TermKey supports Ethereum, Bitcoin, Solana, and custom network labels. Public address derivation is available for supported Ethereum, Bitcoin, and Solana private keys and seed phrases.

For password entries, you can also store optional username and URL metadata alongside the secret.
Both the TUI add form and `termkey add` can generate a strong password for you during entry creation.

---

## CLI Commands

The TUI is the default interface, but the command mode is fully available when you want direct operations:

```bash
termkey init
termkey add
termkey list
termkey view <name-or-index>
termkey edit <name-or-index>
termkey rename <old> <new>
termkey delete <name-or-index>
termkey copy <name-or-index>
termkey search <query>
termkey export <directory>
termkey import <path/to/backup.termkey>
termkey passwd
termkey recover
termkey config --show
termkey update
termkey derive <name-or-index>
termkey browser install
termkey browser status
termkey browser repair
```

Notes:

- `termkey export <directory>` prompts for an editable backup name (default: `backup`) and writes an encrypted `<name>.termkey` file into that directory.
- Imports remain compatible with existing `.ck` backup files.
- `termkey recover` uses your configured 24-word recovery phrase.
- `termkey update` checks the latest GitHub release. On Homebrew installs it runs the Homebrew update flow; on installer/manual/source installs it prints the correct release download page.
- `termkey derive` saves a public address for supported Ethereum, Bitcoin, and Solana key or seed entries.

---

## Keyboard Shortcuts

| | |
|---|---|
| **Navigation** | `↑/↓` move, type a number then press `Enter` to jump to that entry, `Esc` back/clear filter, `/` search, `Shift+F` find/filter |
| **Entry** | `Shift+A` add, `Shift+V` view, `Shift+C` copy, `Shift+E` edit, `Shift+D` delete |
| **Vault** | `Shift+X` export, `Shift+I` import, `Shift+P` change password, `Shift+S` settings |
| **Other** | `?` help, `Shift+Q` quit, `Ctrl+C` quit, `Ctrl+Q` quit, `F1` recovery from the login screen |

---

## Links

- [Releases](https://github.com/ryanonmars/termkey/releases)
- [Issues](https://github.com/ryanonmars/termkey/issues)

**License:** MIT · **Rust**
