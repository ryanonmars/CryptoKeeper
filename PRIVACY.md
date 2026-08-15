# TermKey Privacy Policy

**Effective date: 2026-08-15**

TermKey is a local vault and browser-extension integration. This policy describes the current Apple Silicon Phase 1 application and extension.

## Local vault and browser integration

Your vault is stored locally under `~/.termkey/`. The Chrome extension communicates with the local `com.ryanonmars.termkey` native messaging host to unlock the vault, find matching entries, generate passwords, save an entry you approve, and fill a selected entry. Native messaging is communication between Chrome and the local TermKey host; it is not a connection to a TermKey server.

When you choose to save a login, the extension reads the visible username and password fields associated with that submitted login and sends them to the local native host for the save or update you approve. When you choose autofill, it receives the selected entry from the local host and places the username and password in the matching visible page fields. It also inspects visible login, sign-up, and password-change fields to decide whether those user-directed actions are available. It does not read page fields for advertising, analytics, or sale.

## Extension data and retention

The current extension does not call `chrome.storage.local`, `chrome.storage.sync`, or `chrome.storage.session`; it stores no data in Chrome storage. The manifest declares the `storage` permission, but the current extension does not use that API.

While the extension service worker is running, it keeps limited working state in memory: short-lived match grants (up to 30 seconds) and a pending login selected for a save or update (up to 120 seconds). That pending login can include the submitted username and password. This state is cleared when its operation finishes or expires, and it is not written through the Chrome storage API.

Saved vault entries remain in your local vault until you delete the entry or remove the vault. Uninstalling the extension removes its browser integration but does not remove your vault. To remove local vault data, delete its entries in TermKey or remove `~/.termkey/` after making any backup you intend to keep.

## Network use and disclosures

The extension has no production-code use of `fetch`, `XMLHttpRequest`, WebSocket, or another direct web-network API. Its HTTP and HTTPS site access lets its content script operate on eligible pages for the user-directed login, save, autofill, and password-generation features described above; it does not send page credentials to those sites.

The desktop application separately checks GitHub's latest-release API when it performs an update check, including during normal application startup and `termkey update`. That request identifies the application version in its User-Agent and asks GitHub for release metadata; it is not sent by the extension or native messaging host and does not include vault secrets or page credentials. Opening release or download links also connects you to GitHub. GitHub and your network provider may receive ordinary connection information such as your IP address under their own policies.

TermKey does not sell personal information. It does not use analytics or advertising SDKs. Vault secrets, master passwords, secondary passwords, generated passwords, and page login credentials are not transmitted to the developer or to third parties.

## Chrome permission purposes

- `storage` is declared in the manifest but is not used by the current extension.
- `activeTab` identifies the active page for an action you initiate from the extension.
- `scripting` can restore the TermKey content script on that active page when Chrome has not already loaded it.
- `nativeMessaging` connects Chrome to the local TermKey native host.
- HTTP and HTTPS host access lets the content script find relevant visible login fields, offer user-directed save/autofill prompts, and fill selected credentials on supported web pages.

## Contact

For privacy questions or requests, open an issue at [GitHub Issues](https://github.com/ryanonmars/termkey/issues).
