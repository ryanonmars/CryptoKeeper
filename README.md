# CryptoKeeper

Local-only, encrypted TUI vault for cryptocurrency private keys and seed phrases. **XChaCha20-Poly1305** + **Argon2id**. Zero cloud. Zero trust.

- **Vault:** `~/.cryptokeeper/` — local storage only, no network, no cloud sync
- **TUI:** Run `cryptokeeper`, use keyboard shortcuts (Shift+letter for actions)

---

## Security

| | |
|---|---|
| **XChaCha20-Poly1305** | AEAD cipher, 192-bit nonce — authenticated, tamper-evident |
| **Argon2id** | Memory-hard KDF — resistant to GPU & ASIC attacks |
| **~/.cryptokeeper/** | Local-only storage — no network access |

---

## Install

### macOS

**Homebrew** (recommended; updates with `brew upgrade`):

```bash
brew install ryanonmars/cryptokeeper/cryptokeeper
```

**Direct download:** [Apple Silicon (ARM64)](https://github.com/ryanonmars/CryptoKeeper/releases/latest/download/cryptokeeper-macos-aarch64.zip) · [Intel (x86_64)](https://github.com/ryanonmars/CryptoKeeper/releases/latest/download/cryptokeeper-macos-x86_64.zip)

```bash
unzip cryptokeeper-macos-*.zip
chmod +x cryptokeeper
sudo mv cryptokeeper /usr/local/bin/
```

**Gatekeeper:** If macOS blocks the app, go to **System Settings → Privacy & Security**, scroll to the **Security** section — the blocked app appears there. Click **Open Anyway**, then confirm with **Open**. Optionally, from the folder where the binary is: `xattr -d com.apple.quarantine ./cryptokeeper` (if you see "No such xattr", skip that step).

### Linux

**Homebrew (Linuxbrew):** [brew.sh](https://brew.sh) then:

```bash
brew install ryanonmars/cryptokeeper/cryptokeeper
```

**Direct download:** [Linux x86_64](https://github.com/ryanonmars/CryptoKeeper/releases/latest/download/cryptokeeper-linux-x86_64.zip)

```bash
unzip cryptokeeper-linux-x86_64.zip
chmod +x cryptokeeper
sudo mv cryptokeeper /usr/local/bin/
```

### Windows

Download: [Windows x86_64](https://github.com/ryanonmars/CryptoKeeper/releases/latest/download/cryptokeeper-windows-x86_64.zip)

Extract the ZIP, place `cryptokeeper.exe` in a folder (e.g. `C:\tools\`), then add it to your **PATH** (System Properties → Environment Variables → Path → Edit → New).  
**SmartScreen:** On first launch click "More info" → "Run anyway", or right-click the .exe → Properties → check **Unblock**.

---

## Usage

1. **Launch:** Run `cryptokeeper`. First launch runs the setup wizard (create vault + master password); after that you get the login screen.
2. **Dashboard:** Entry list is the home screen. **↑/↓** navigate, **/** search/filter, **Enter** view selected entry.
3. **Add:** **Shift+A** — fill the form; the secret field is hidden and never touches shell history.
4. **View / copy:** **Shift+V** reveal in TUI, **Shift+C** copy to clipboard (auto-clears after 10s).
5. **Edit / delete:** **Shift+E** edit, **Shift+D** delete (confirmation required). **Shift+X** export vault, **Shift+I** import backup.
6. **Help:** **?** shows the full shortcut list. **Shift+Q** quit. **Ctrl+C** or **Ctrl+Q** quit from anywhere.

---

## Keyboard shortcuts

| | |
|---|--|
| **Navigation** | ↑/↓ move, Enter select, Esc back/clear filter, / search, **Shift+F** find/filter |
| **Entry** | **Shift+A** add, **Shift+V** view, **Shift+C** copy, **Shift+E** edit, **Shift+D** delete |
| **Vault** | **Shift+X** export, **Shift+I** import, **Shift+P** change password, **Shift+S** settings |
| **Other** | **?** help, **Shift+Q** quit, **F1** recovery (login screen) |

---

## Links

- [Releases](https://github.com/ryanonmars/CryptoKeeper/releases)
- [Issues](https://github.com/ryanonmars/CryptoKeeper/issues)

**License:** MIT · **Rust**
