import type {
  PopupSiteMatch,
  PopupCapturedLoginResponse,
  PopupCapturedLoginStepResponse,
  PopupFillResultResponse,
  PopupGeneratedPasswordResponse,
  PopupPendingLoginResponse,
  PopupPageContextResponse,
  PopupSaveResultResponse,
  PopupToBackgroundMessage,
  PopupToBackgroundResponse,
} from "@termkey/types";

document.body.innerHTML = `
  <style>
    :root {
      color-scheme: dark;
      font-family: "IBM Plex Sans", "Segoe UI", sans-serif;
    }

    * {
      box-sizing: border-box;
    }

    body {
      margin: 0;
      min-width: 360px;
      background:
        radial-gradient(circle at top, rgba(34, 197, 94, 0.18), transparent 42%),
        linear-gradient(180deg, #121923 0%, #0b0f14 100%);
      color: #f3f4f6;
    }

    button,
    input {
      font: inherit;
    }

    .popup {
      display: grid;
      gap: 12px;
      padding: 14px;
    }

    .header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
    }

    .eyebrow {
      margin: 0;
      color: #7dd3fc;
      font-size: 13px;
      font-weight: 600;
      letter-spacing: -0.03em;
    }

    .connection {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 6px 9px;
      border: 1px solid rgba(148, 163, 184, 0.14);
      border-radius: 999px;
      background: rgba(15, 23, 42, 0.72);
      color: #cbd5e1;
      font-size: 12px;
      white-space: nowrap;
    }

    .status-dot {
      width: 10px;
      height: 10px;
      border-radius: 999px;
      background: #ef4444;
      box-shadow: 0 0 0 3px rgba(239, 68, 68, 0.12);
      transition: background 140ms ease, box-shadow 140ms ease;
    }

    .status-dot--online {
      background: #22c55e;
      box-shadow: 0 0 0 3px rgba(34, 197, 94, 0.18);
    }

    .panel {
      padding: 12px;
      border: 1px solid rgba(148, 163, 184, 0.14);
      border-radius: 16px;
      background: rgba(15, 23, 42, 0.74);
      box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.03);
    }

    .panel[hidden] {
      display: none;
    }

    .site-row {
      display: grid;
      gap: 12px;
    }

    .site-actions {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: 8px;
      width: 100%;
    }

    .site-label {
      display: block;
      margin-bottom: 4px;
      color: #7dd3fc;
      font-size: 11px;
      font-weight: 600;
      letter-spacing: 0.08em;
      text-transform: uppercase;
    }

    .site-hostname {
      display: block;
      font-size: 15px;
      font-weight: 600;
      line-height: 1.3;
      word-break: break-word;
    }

    .site-summary {
      display: block;
      margin-top: 3px;
      color: #93a4b8;
      font-size: 12px;
      line-height: 1.4;
    }

    .match-picker {
      display: grid;
      gap: 10px;
      margin-top: 14px;
      padding-top: 14px;
      border-top: 1px solid rgba(148, 163, 184, 0.14);
    }

    .match-picker[hidden] {
      display: none;
    }

    .match-picker-header {
      display: flex;
      align-items: baseline;
      justify-content: space-between;
      gap: 12px;
    }

    .match-list-label {
      color: #7dd3fc;
      font-size: 11px;
      font-weight: 600;
      letter-spacing: 0.08em;
      text-transform: uppercase;
    }

    .match-list-summary {
      color: #93a4b8;
      font-size: 12px;
    }

    .match-list {
      display: grid;
      gap: 8px;
    }

    .match-option {
      display: grid;
      gap: 8px;
      width: 100%;
      padding: 12px;
      border: 1px solid rgba(148, 163, 184, 0.16);
      border-radius: 14px;
      background: rgba(2, 6, 23, 0.52);
      color: #f8fafc;
      text-align: left;
      transition:
        border-color 140ms ease,
        background 140ms ease,
        transform 140ms ease;
    }

    .match-option:hover {
      transform: translateY(-1px);
      border-color: rgba(125, 211, 252, 0.4);
      background: rgba(15, 23, 42, 0.82);
    }

    .match-option-main {
      display: flex;
      align-items: flex-start;
      justify-content: space-between;
      gap: 10px;
    }

    .match-option-name {
      font-size: 14px;
      font-weight: 600;
      line-height: 1.3;
    }

    .match-option-detail {
      display: block;
      margin-top: 4px;
      color: #93a4b8;
      font-size: 12px;
      line-height: 1.4;
      word-break: break-word;
    }

    .match-option-side {
      display: grid;
      justify-items: end;
      flex-shrink: 0;
    }

    .fill-button,
    .generate-button,
    .save-button,
    .unlock-button {
      border: 0;
      border-radius: 12px;
      font-weight: 700;
      cursor: pointer;
      transition: transform 140ms ease, opacity 140ms ease;
    }

    .fill-button {
      background: linear-gradient(180deg, #22c55e 0%, #16a34a 100%);
      color: #03120a;
      padding: 10px 14px;
    }

    .generate-button {
      padding: 10px 14px;
      background: linear-gradient(180deg, #7dd3fc 0%, #38bdf8 100%);
      color: #062033;
    }

    .save-button {
      padding: 10px 14px;
      background: rgba(15, 23, 42, 0.9);
      color: #dbeafe;
      border: 1px solid rgba(125, 211, 252, 0.26);
    }

    .fill-button,
    .generate-button,
    .save-button {
      flex: 1 1 calc(50% - 4px);
      min-width: 0;
      white-space: nowrap;
    }

    .match-fill-button {
      min-width: 92px;
      padding: 8px 12px;
      font-size: 12px;
    }

    .unlock-button {
      background: linear-gradient(180deg, #22c55e 0%, #16a34a 100%);
      color: #03120a;
      padding: 11px 14px;
    }

    .fill-button:hover:not(:disabled),
    .generate-button:hover:not(:disabled),
    .save-button:hover:not(:disabled),
    .unlock-button:hover:not(:disabled) {
      transform: translateY(-1px);
    }

    .fill-button:disabled,
    .generate-button:disabled,
    .save-button:disabled,
    .unlock-button:disabled {
      opacity: 0.45;
      cursor: not-allowed;
      transform: none;
    }

    .unlock-label {
      display: block;
      color: #cbd5e1;
      font-size: 12px;
      font-weight: 600;
      margin-bottom: 8px;
    }

    .unlock-row {
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 10px;
    }

    .password-stack {
      display: grid;
      gap: 10px;
    }

    .secondary-password-group[hidden] {
      display: none;
    }

    .save-fields {
      display: grid;
      gap: 10px;
    }

    .password-input {
      width: 100%;
      padding: 11px 12px;
      border: 1px solid rgba(148, 163, 184, 0.18);
      border-radius: 12px;
      background: rgba(2, 6, 23, 0.72);
      color: #f8fafc;
      outline: none;
    }

    .password-input::placeholder {
      color: #64748b;
    }

    .password-input:focus {
      border-color: rgba(34, 197, 94, 0.6);
      box-shadow: 0 0 0 3px rgba(34, 197, 94, 0.14);
    }

    .checkbox-row {
      display: inline-flex;
      align-items: center;
      gap: 10px;
      color: #cbd5e1;
      font-size: 13px;
      font-weight: 600;
    }

    .checkbox-row input {
      width: 16px;
      height: 16px;
      accent-color: #22c55e;
    }

    .save-actions {
      display: flex;
      justify-content: flex-end;
      gap: 8px;
    }

    .cancel-button {
      border: 1px solid rgba(148, 163, 184, 0.2);
      border-radius: 12px;
      background: rgba(15, 23, 42, 0.72);
      color: #cbd5e1;
      font-weight: 600;
      padding: 11px 14px;
      cursor: pointer;
      transition: transform 140ms ease, opacity 140ms ease;
    }

    .cancel-button:hover:not(:disabled) {
      transform: translateY(-1px);
    }

    .cancel-button:disabled {
      opacity: 0.45;
      cursor: not-allowed;
      transform: none;
    }

    .hint,
    .message {
      margin: 0;
      font-size: 13px;
      line-height: 1.5;
    }

    .hint {
      margin-top: 9px;
      color: #93a4b8;
    }

    .message {
      padding: 0 2px;
      color: #cbd5e1;
    }

    .message[data-tone="success"] {
      color: #86efac;
    }

    .message[data-tone="error"] {
      color: #fca5a5;
    }
  </style>
  <main class="popup">
    <header class="header">
      <div>
        <p class="eyebrow">TermKey</p>
      </div>
      <div class="connection">
        <span id="backend-dot" class="status-dot" aria-hidden="true"></span>
        <span id="backend-label">Checking backend</span>
      </div>
    </header>

    <section id="site-panel" class="panel">
      <div class="site-row">
        <div>
          <span class="site-label">Site</span>
          <strong id="site-hostname" class="site-hostname">Waiting for page...</strong>
          <span id="site-summary" class="site-summary">Checking the current tab.</span>
        </div>
        <div class="site-actions">
          <button id="generate-password" class="generate-button" disabled>Generate</button>
          <button id="save-login" class="save-button" disabled>Save</button>
          <button id="fill-best-match" class="fill-button" disabled>Fill</button>
        </div>
      </div>
      <div id="match-picker" class="match-picker" hidden>
        <div class="match-picker-header">
          <span class="match-list-label">Saved logins</span>
          <span id="match-list-summary" class="match-list-summary"></span>
        </div>
        <div
          id="match-list"
          class="match-list"
          role="listbox"
          aria-label="Saved logins for the current site"
        ></div>
      </div>
    </section>

    <section id="save-section" class="panel" hidden>
      <label class="save-fields">
        <span id="save-panel-label" class="unlock-label">Save login</span>
        <input
          id="save-entry-name"
          class="password-input"
          type="text"
          placeholder="Entry name"
          autocomplete="off"
        />
        <input
          id="save-username"
          class="password-input"
          type="text"
          placeholder="Username (optional)"
          autocomplete="username"
        />
        <label class="checkbox-row">
          <input id="save-use-secondary-password" type="checkbox" />
          <span>Protect this login with a secondary password</span>
        </label>
        <div id="save-secondary-password-group" class="secondary-password-group" hidden>
          <div class="password-stack">
            <input
              id="save-secondary-password"
              class="password-input"
              type="password"
              placeholder="Secondary password"
              autocomplete="off"
            />
            <input
              id="save-secondary-password-confirm"
              class="password-input"
              type="password"
              placeholder="Confirm secondary password"
              autocomplete="off"
            />
          </div>
        </div>
      </label>
      <div class="save-actions">
        <button id="cancel-save" class="cancel-button" type="button">Cancel</button>
        <button id="submit-save" class="unlock-button" type="button">Save login</button>
      </div>
      <p id="save-panel-hint" class="hint">
        Reads the username and password currently typed into this page for this save request.
      </p>
    </section>

    <section id="unlock-section" class="panel">
      <label>
        <span id="password-panel-label" class="unlock-label">Master password</span>
        <div class="unlock-row">
          <div class="password-stack">
            <input
              id="master-password"
              class="password-input"
              type="password"
              placeholder="Enter your master password"
              autocomplete="current-password"
            />
            <div id="secondary-password-group" class="secondary-password-group" hidden>
              <input
                id="secondary-password"
                class="password-input"
                type="password"
                placeholder="Enter the secondary password"
                autocomplete="off"
              />
            </div>
          </div>
          <button id="unlock-vault" class="unlock-button">Authenticate</button>
        </div>
      </label>
      <p id="password-panel-hint" class="hint">Your password is only used for this fill request.</p>
    </section>

    <p id="recovery-notice" class="message" data-tone="error" hidden></p>
    <p id="native-host-status" class="message">Checking TermKey status...</p>
  </main>
`;

const backendDotEl = document.querySelector<HTMLSpanElement>("#backend-dot");
const backendLabelEl = document.querySelector<HTMLSpanElement>("#backend-label");
const sitePanelEl = document.querySelector<HTMLElement>("#site-panel");
const generatePasswordButtonEl =
  document.querySelector<HTMLButtonElement>("#generate-password");
const saveLoginButtonEl =
  document.querySelector<HTMLButtonElement>("#save-login");
const fillBestMatchButton =
  document.querySelector<HTMLButtonElement>("#fill-best-match");
const saveSectionEl =
  document.querySelector<HTMLElement>("#save-section");
const savePanelLabelEl =
  document.querySelector<HTMLSpanElement>("#save-panel-label");
const saveEntryNameInputEl =
  document.querySelector<HTMLInputElement>("#save-entry-name");
const saveUsernameInputEl =
  document.querySelector<HTMLInputElement>("#save-username");
const saveUseSecondaryPasswordInputEl =
  document.querySelector<HTMLInputElement>("#save-use-secondary-password");
const saveSecondaryPasswordGroupEl =
  document.querySelector<HTMLElement>("#save-secondary-password-group");
const saveSecondaryPasswordInputEl =
  document.querySelector<HTMLInputElement>("#save-secondary-password");
const saveSecondaryPasswordConfirmInputEl =
  document.querySelector<HTMLInputElement>("#save-secondary-password-confirm");
const cancelSaveButtonEl =
  document.querySelector<HTMLButtonElement>("#cancel-save");
const submitSaveButtonEl =
  document.querySelector<HTMLButtonElement>("#submit-save");
const savePanelHintEl =
  document.querySelector<HTMLParagraphElement>("#save-panel-hint");
const unlockSectionEl =
  document.querySelector<HTMLElement>("#unlock-section");
const unlockButton = document.querySelector<HTMLButtonElement>("#unlock-vault");
const passwordInput =
  document.querySelector<HTMLInputElement>("#master-password");
const secondaryPasswordInputEl =
  document.querySelector<HTMLInputElement>("#secondary-password");
const statusEl = document.querySelector<HTMLParagraphElement>("#native-host-status");
const recoveryNoticeEl =
  document.querySelector<HTMLParagraphElement>("#recovery-notice");
const siteHostnameEl =
  document.querySelector<HTMLSpanElement>("#site-hostname");
const siteSummaryEl =
  document.querySelector<HTMLSpanElement>("#site-summary");
const matchPickerEl = document.querySelector<HTMLElement>("#match-picker");
const matchListSummaryEl =
  document.querySelector<HTMLSpanElement>("#match-list-summary");
const matchListEl = document.querySelector<HTMLElement>("#match-list");
const passwordPanelLabelEl =
  document.querySelector<HTMLSpanElement>("#password-panel-label");
const passwordPanelHintEl =
  document.querySelector<HTMLParagraphElement>("#password-panel-hint");
const secondaryPasswordGroupEl =
  document.querySelector<HTMLElement>("#secondary-password-group");

if (
  !backendDotEl ||
  !backendLabelEl ||
  !sitePanelEl ||
  !generatePasswordButtonEl ||
  !saveLoginButtonEl ||
  !fillBestMatchButton ||
  !saveSectionEl ||
  !savePanelLabelEl ||
  !saveEntryNameInputEl ||
  !saveUsernameInputEl ||
  !saveUseSecondaryPasswordInputEl ||
  !saveSecondaryPasswordGroupEl ||
  !saveSecondaryPasswordInputEl ||
  !saveSecondaryPasswordConfirmInputEl ||
  !cancelSaveButtonEl ||
  !submitSaveButtonEl ||
  !savePanelHintEl ||
  !unlockSectionEl ||
  !unlockButton ||
  !passwordInput ||
  !secondaryPasswordInputEl ||
  !statusEl ||
  !recoveryNoticeEl ||
  !siteHostnameEl ||
  !siteSummaryEl ||
  !matchPickerEl ||
  !matchListSummaryEl ||
  !matchListEl ||
  !passwordPanelLabelEl ||
  !passwordPanelHintEl ||
  !secondaryPasswordGroupEl
) {
  throw new Error("Popup UI failed to initialize.");
}

type MessageTone = "neutral" | "success" | "error";

const backendDot = backendDotEl;
const backendLabel = backendLabelEl;
const sitePanel = sitePanelEl;
const generatePasswordButton = generatePasswordButtonEl;
const saveLoginButton = saveLoginButtonEl;
const fillButton = fillBestMatchButton;
const saveSection = saveSectionEl;
const savePanelLabel = savePanelLabelEl;
const saveEntryNameInput = saveEntryNameInputEl;
const saveUsernameInput = saveUsernameInputEl;
const saveUseSecondaryPasswordInput = saveUseSecondaryPasswordInputEl;
const saveSecondaryPasswordGroup = saveSecondaryPasswordGroupEl;
const saveSecondaryPasswordInput = saveSecondaryPasswordInputEl;
const saveSecondaryPasswordConfirmInput = saveSecondaryPasswordConfirmInputEl;
const cancelSaveButton = cancelSaveButtonEl;
const submitSaveButton = submitSaveButtonEl;
const savePanelHint = savePanelHintEl;
const unlockSection = unlockSectionEl;
const unlockVaultButton = unlockButton;
const masterPasswordInput = passwordInput;
const secondaryPasswordInput = secondaryPasswordInputEl;
const statusMessage = statusEl;
const recoveryNotice = recoveryNoticeEl;
const siteHostname = siteHostnameEl;
const siteSummary = siteSummaryEl;
const matchPicker = matchPickerEl;
const matchListSummary = matchListSummaryEl;
const matchList = matchListEl;
const passwordPanelLabel = passwordPanelLabelEl;
const passwordPanelHint = passwordPanelHintEl;
const secondaryPasswordGroup = secondaryPasswordGroupEl;

let currentSiteMatches: PopupSiteMatch[] = [];
let pendingFillMatch: PopupSiteMatch | null = null;
let pendingSaveCandidate: PopupCapturedLoginResponse["candidate"] | null = null;
let pendingLoginCandidate: PopupPendingLoginResponse["candidate"] = null;
let pendingLoginMode: "save" | "update" | null = null;
let fillingEntryId: string | null = null;
let captureInFlight = false;
let generationInFlight = false;
let saveInFlight = false;
let backendConnected = false;
let vaultExists = true;
let vaultLocked = true;
let hasSupportedPage = true;
let pageContext: PopupPageContextResponse["context"] = {
  intent: "unknown",
  visibleUsername: null,
  hasPasswordField: false,
  hasEmptyLoginField: false,
  hasConfirmationPasswordField: false,
  canGeneratePassword: false,
  hasPendingSaveUsername: false,
  pendingUsername: null,
};
let siteDetails = {
  hostname: "Waiting for page...",
  summary: "Checking the current tab.",
};

function setSiteVisibility(visible: boolean) {
  hasSupportedPage = visible;
  sitePanel.hidden = !visible;
  if (!visible) {
    setPageContext({
      intent: "unknown",
      visibleUsername: null,
      hasPasswordField: false,
      hasEmptyLoginField: false,
      hasConfirmationPasswordField: false,
      canGeneratePassword: false,
      hasPendingSaveUsername: false,
      pendingUsername: null,
    });
    setCurrentSiteMatches([]);
    clearPendingSave();
  }
  updateFillButtonState();
}

function primeCurrentSite() {
  chrome.tabs.query(
    { active: true, currentWindow: true },
    (tabs: Array<{ url?: string }>) => {
      const runtimeError = chrome.runtime.lastError;
      if (runtimeError) {
        setSiteVisibility(false);
        return;
      }

      const url = tabs[0]?.url;
      if (!url) {
        setSiteVisibility(false);
        return;
      }

      try {
        const parsedUrl = new URL(url);
        if (
          (parsedUrl.protocol !== "http:" && parsedUrl.protocol !== "https:") ||
          parsedUrl.username !== "" ||
          parsedUrl.password !== ""
        ) {
          setSiteVisibility(false);
          return;
        }

        setSiteVisibility(true);
        renderSite(parsedUrl.origin, "Checking for a saved login...");
      } catch {
        setSiteVisibility(false);
      }
    }
  );
}

function setBackendStatus(connected: boolean, label: string) {
  backendConnected = connected;
  backendDot.classList.toggle("status-dot--online", connected);
  backendLabel.textContent = label;
  updateFillButtonState();
}

function renderMessage(message: string, tone: MessageTone = "neutral") {
  statusMessage.dataset.tone = tone;
  statusMessage.textContent = message;
}

function renderSite(hostnameText: string, summaryText: string) {
  siteDetails = {
    hostname: hostnameText,
    summary: summaryText,
  };
  siteHostname.textContent = siteDetails.hostname;
  siteSummary.textContent = siteDetails.summary;
}

function suggestEntryName(hostnameText: string, username: string | null) {
  if (username) {
    return `${hostnameText} • ${username}`;
  }

  return hostnameText;
}

function normalizedUsername(value: string | null | undefined) {
  return value?.trim().toLowerCase() ?? "";
}

function hasMatchingSavedUsername(username: string | null | undefined) {
  const normalized = normalizedUsername(username);
  if (!normalized) {
    return false;
  }

  return currentSiteMatches.some(
    (match) => normalizedUsername(match.username) === normalized
  );
}

function getActionableUsername() {
  return pageContext.visibleUsername ?? pageContext.pendingUsername;
}

function setPageContext(context: PopupPageContextResponse["context"]) {
  pageContext = context;
  renderMatchPicker();
  updateFillButtonState();
}

function stageSaveCandidate(
  candidate: PopupCapturedLoginResponse["candidate"],
  hintText: string
) {
  pendingSaveCandidate = candidate;
  pendingLoginCandidate = null;
  pendingLoginMode = null;
  saveEntryNameInput.value = suggestEntryName(
    siteDetails.hostname,
    candidate.username
  );
  saveUsernameInput.value = candidate.username ?? "";
  saveUseSecondaryPasswordInput.checked = false;
  saveSecondaryPasswordInput.value = "";
  saveSecondaryPasswordConfirmInput.value = "";
  savePanelHint.textContent = hintText;
  renderSavePrompt();
}

function stagePendingLogin(
  candidate: NonNullable<PopupPendingLoginResponse["candidate"]>
) {
  pendingSaveCandidate = null;
  pendingLoginCandidate = candidate;
  pendingLoginMode = candidate.mode;
  saveEntryNameInput.value = suggestEntryName(
    siteDetails.hostname,
    candidate.username
  );
  saveUsernameInput.value = candidate.username ?? "";
  saveUseSecondaryPasswordInput.checked = false;
  saveSecondaryPasswordInput.value = "";
  saveSecondaryPasswordConfirmInput.value = "";
  savePanelHint.textContent =
    "The submitted password remains in the extension background until you choose an action.";
  renderSavePrompt();
  updateFillButtonState();
}

function updateFillButtonState() {
  const singleMatch = currentSiteMatches.length === 1;
  const actionableUsername = getActionableUsername();
  const knownUsername = hasMatchingSavedUsername(actionableUsername);
  const showGenerateAction = pageContext.canGeneratePassword;
  const showFillAction =
    pageContext.hasEmptyLoginField &&
    pageContext.intent !== "signup" &&
    pageContext.intent !== "password_change" &&
    singleMatch;
  const showSaveAction =
    vaultExists &&
    (pendingSaveCandidate !== null ||
      pendingLoginCandidate !== null ||
      pageContext.intent === "signup" ||
      (pageContext.intent === "login" &&
        (currentSiteMatches.length === 0 ||
          (Boolean(actionableUsername) && !knownUsername))));

  fillButton.hidden = !showFillAction;
  fillButton.disabled =
    !hasSupportedPage ||
    !backendConnected ||
    !showFillAction ||
    fillingEntryId !== null;
  fillButton.textContent = fillingEntryId !== null ? "Filling..." : "Fill";

  generatePasswordButton.hidden = !showGenerateAction;
  generatePasswordButton.disabled =
    !hasSupportedPage ||
    !backendConnected ||
    !showGenerateAction ||
    captureInFlight ||
    generationInFlight ||
    saveInFlight ||
    fillingEntryId !== null;
  generatePasswordButton.textContent = generationInFlight
    ? "Generating..."
    : "Generate";

  saveLoginButton.hidden = !showSaveAction;
  saveLoginButton.disabled =
    !hasSupportedPage ||
    !backendConnected ||
    !showSaveAction ||
    captureInFlight ||
    generationInFlight ||
    saveInFlight ||
    fillingEntryId !== null;
  saveLoginButton.textContent = captureInFlight
    ? "Reading..."
    : currentSiteMatches.length > 0 && pageContext.intent === "login"
      ? "Save Another"
      : "Save";
}

function resetMatches(summaryText: string) {
  setCurrentSiteMatches([]);
  renderSite(siteDetails.hostname, summaryText);
  updateFillButtonState();
}

function clearPendingSave() {
  pendingSaveCandidate = null;
  pendingLoginCandidate = null;
  pendingLoginMode = null;
  saveEntryNameInput.value = "";
  saveUsernameInput.value = "";
  saveUseSecondaryPasswordInput.checked = false;
  saveSecondaryPasswordInput.value = "";
  saveSecondaryPasswordConfirmInput.value = "";
  saveSecondaryPasswordGroup.hidden = true;
  saveSection.hidden = true;
  savePanelLabel.textContent = "Save login";
  savePanelHint.textContent =
    "Reads the username and password currently typed into this page for this save request.";
}

function renderPasswordPrompt() {
  const activeMatch = pendingFillMatch;
  const needsSecondaryPassword = activeMatch?.hasSecondaryPassword === true;
  unlockSection.hidden = !vaultLocked && !needsSecondaryPassword;

  if (unlockSection.hidden) {
    masterPasswordInput.hidden = false;
    masterPasswordInput.disabled = false;
    masterPasswordInput.value = "";
    secondaryPasswordInput.disabled = false;
    secondaryPasswordInput.value = "";
    secondaryPasswordGroup.hidden = true;
    unlockVaultButton.disabled = true;
    unlockVaultButton.textContent = "Unlock";
    return;
  }

  passwordPanelLabel.textContent = vaultLocked
    ? "Unlock TermKey vault"
    : `Secondary password for ${activeMatch?.name ?? "this login"}`;
  passwordPanelHint.textContent = vaultLocked
    ? "Unlock is retained by the native host for this extension session."
    : `Required to fill ${activeMatch?.name ?? "this login"} on ${siteDetails.hostname}.`;
  masterPasswordInput.hidden = !vaultLocked;
  masterPasswordInput.disabled = !vaultLocked || fillingEntryId !== null;
  secondaryPasswordInput.disabled = fillingEntryId !== null;
  secondaryPasswordGroup.hidden = !needsSecondaryPassword;
  unlockVaultButton.disabled = !backendConnected || fillingEntryId !== null;
  unlockVaultButton.textContent = fillingEntryId
    ? "Working..."
    : vaultLocked
      ? "Unlock"
      : "Fill";
}

function renderSavePrompt() {
  const activeSave =
    pendingSaveCandidate !== null || pendingLoginCandidate !== null;
  const isPendingLogin = pendingLoginCandidate !== null;
  saveSection.hidden = !activeSave;
  saveSecondaryPasswordGroup.hidden = !saveUseSecondaryPasswordInput.checked;

  if (!activeSave) {
    submitSaveButton.disabled = true;
    submitSaveButton.textContent = "Save login";
    cancelSaveButton.disabled = false;
    return;
  }

  savePanelLabel.textContent = isPendingLogin
    ? pendingLoginMode === "update"
      ? "Update the saved login for this site?"
      : "Save this login?"
    : "Save login";

  const disabled =
    !backendConnected ||
    vaultLocked ||
    saveInFlight ||
    captureInFlight ||
    generationInFlight;
  saveEntryNameInput.disabled = saveInFlight;
  saveUsernameInput.disabled = saveInFlight;
  saveUseSecondaryPasswordInput.disabled = saveInFlight;
  saveSecondaryPasswordInput.disabled = saveInFlight;
  saveSecondaryPasswordConfirmInput.disabled = saveInFlight;
  cancelSaveButton.disabled = saveInFlight;
  submitSaveButton.disabled = disabled;
  submitSaveButton.textContent = saveInFlight
    ? "Saving..."
    : isPendingLogin && pendingLoginMode === "update"
      ? "Update login"
      : "Save login";
}

function describeMatches(matches: PopupSiteMatch[]) {
  if (matches.length === 0) {
    return "No saved login found for this site.";
  }

  if (matches.length === 1) {
    const match = matches[0];
    const details = [match.name];

    if (match.username) {
      details.push(match.username);
    }

    return details.join(" • ");
  }

  return `${matches.length} saved logins found. Choose one to fill.`;
}

function formatMatchDetail(match: PopupSiteMatch) {
  const suffix = match.hasSecondaryPassword
    ? " • Secondary password required"
    : "";

  if (match.username) {
    return `${match.username}${suffix}`;
  }

  return suffix ? `No username saved${suffix}` : "No username saved";
}

function renderMatchPicker() {
  const multipleMatches =
    pageContext.hasEmptyLoginField &&
    currentSiteMatches.length > 1 &&
    pageContext.intent !== "signup" &&
    pageContext.intent !== "password_change";
  matchPicker.hidden = !multipleMatches;
  matchList.replaceChildren();

  if (!multipleMatches) {
    matchListSummary.textContent = "";
    return;
  }

  matchListSummary.textContent = `${currentSiteMatches.length} matches`;

  const fragment = document.createDocumentFragment();

  currentSiteMatches.forEach((match) => {
    const option = document.createElement("div");
    option.className = "match-option";

    const main = document.createElement("span");
    main.className = "match-option-main";

    const content = document.createElement("span");

    const name = document.createElement("span");
    name.className = "match-option-name";
    name.textContent = match.name;

    const detail = document.createElement("span");
    detail.className = "match-option-detail";
    detail.textContent = formatMatchDetail(match);

    const side = document.createElement("span");
    side.className = "match-option-side";

    const actionButton = document.createElement("button");
    actionButton.type = "button";
    actionButton.className = "fill-button match-fill-button";
    actionButton.disabled = !backendConnected || fillingEntryId !== null;
    actionButton.textContent =
      fillingEntryId === match.id
        ? "Filling..."
        : pendingFillMatch?.id === match.id
          ? "Enter password"
          : "Fill";

    content.append(name, detail);
    side.append(actionButton);
    main.append(content, side);
    option.append(main);

    actionButton.addEventListener("click", () => {
      beginFill(match);
    });

    fragment.append(option);
  });

  matchList.append(fragment);
}

function setCurrentSiteMatches(matches: PopupSiteMatch[]) {
  currentSiteMatches = matches;
  if (
    pendingFillMatch &&
    !matches.some((match) => match.id === pendingFillMatch?.id)
  ) {
    pendingFillMatch = null;
  }
  renderMatchPicker();
  renderPasswordPrompt();
  renderSavePrompt();
  updateFillButtonState();
}

function beginFill(match: PopupSiteMatch) {
  if (!backendConnected) {
    renderMessage("Reconnect the extension backend before autofill.", "error");
    return;
  }

  if (pendingLoginCandidate) {
    dismissPendingLogin(() => beginFill(match));
    return;
  }

  clearPendingSave();
  pendingFillMatch = match;
  renderMatchPicker();
  renderPasswordPrompt();
  renderMessage(
    match.hasSecondaryPassword
      ? `Enter the secondary password to fill ${match.name}.`
      : `Filling ${match.name}...`
  );
  if (!vaultLocked && !match.hasSecondaryPassword) {
    submitPendingFill();
    return;
  }
  const input = vaultLocked ? masterPasswordInput : secondaryPasswordInput;
  input.focus();
  input.select();
}

function beginSave() {
  if (!backendConnected) {
    renderMessage("Reconnect the extension backend before saving a login.", "error");
    return;
  }

  if (!vaultExists) {
    renderMessage("Create your vault before saving a login.", "error");
    return;
  }

  if (pendingLoginCandidate) {
    dismissPendingLogin(beginSave);
    return;
  }

  pendingFillMatch = null;
  renderPasswordPrompt();
  captureInFlight = true;
  clearPendingSave();
  updateFillButtonState();
  renderMessage("Reading the current login fields from this page...");

  sendMessage(
    {
      type: "termkey.content.captureVisibleCredentials",
    },
    (response) => {
      captureInFlight = false;
      updateFillButtonState();

      if (!response.ok) {
        renderSavePrompt();
        renderMessage(`Could not read this login yet: ${response.error}`, "error");
        return;
      }

      if (response.response.type === "captured_login_step") {
        const partialCapture = response.response as PopupCapturedLoginStepResponse;
        renderSavePrompt();
        renderMessage(
          `Saved ${partialCapture.username} for this sign-in step. Continue to the password page, then click Save again.`,
          "success"
        );
        return;
      }

      if (response.response.type !== "captured_login") {
        renderSavePrompt();
        renderMessage(
          "Background returned the wrong response type for login capture.",
          "error"
        );
        return;
      }

      const captured = response.response as PopupCapturedLoginResponse;
      stageSaveCandidate(
        captured.candidate,
        `Saving for ${siteDetails.hostname}. The password is taken from the current page only for this request.`
      );
      renderMessage(
        captured.usedStoredUsername
          ? `Ready to save a new login for ${siteDetails.hostname}. Username restored from the previous sign-in step.`
          : `Ready to save a new login for ${siteDetails.hostname}.`
      );
      saveEntryNameInput.focus();
      saveEntryNameInput.select();
    }
  );
}

function formatGeneratedPasswordMessage(filledPasswordFields: number) {
  if (filledPasswordFields >= 2) {
    return "Generated a password and filled both password fields.";
  }

  return "Generated a password and filled the password field.";
}

function beginGeneratedPasswordFlow() {
  if (!backendConnected) {
    renderMessage("Reconnect the extension backend before generating a password.", "error");
    return;
  }

  if (pendingLoginCandidate) {
    dismissPendingLogin(beginGeneratedPasswordFlow);
    return;
  }

  pendingFillMatch = null;
  renderPasswordPrompt();
  clearPendingSave();
  generationInFlight = true;
  updateFillButtonState();
  renderMessage("Generating a strong password for this page...");

  sendMessage(
    {
      type: "termkey.passwords.generateForPage",
    },
    (response) => {
      generationInFlight = false;
      updateFillButtonState();

      if (!response.ok) {
        renderSavePrompt();
        renderMessage(`Password generation failed: ${response.error}`, "error");
        return;
      }

      if (response.response.type !== "generated_password") {
        renderSavePrompt();
        renderMessage(
          "Background returned the wrong response type for password generation.",
          "error"
        );
        return;
      }

      const generated = response.response as PopupGeneratedPasswordResponse;

      if (vaultExists) {
        stageSaveCandidate(
          generated.candidate,
          `Saving for ${siteDetails.hostname}. This generated password is already staged for this save request.`
        );
        renderMessage(
          `${formatGeneratedPasswordMessage(
            generated.filledPasswordFields
          )} ${vaultLocked ? "Unlock your vault, then save it." : "It is ready to save."}`,
          "success"
        );
        saveEntryNameInput.focus();
        saveEntryNameInput.select();
        return;
      }

      renderMessage(
        `${formatGeneratedPasswordMessage(
          generated.filledPasswordFields
        )} Create your vault to save it.`,
        "success"
      );
    }
  );
}

function submitPendingFill() {
  if (!backendConnected) {
    renderMessage("Reconnect the extension backend first.", "error");
    return;
  }

  if (vaultLocked) {
    const password = masterPasswordInput.value;
    if (!password) {
      renderMessage("Enter your master password to unlock TermKey.", "error");
      masterPasswordInput.focus();
      return;
    }

    unlockVaultButton.disabled = true;
    unlockVaultButton.textContent = "Unlocking...";
    sendMessage(
      {
        type: "termkey.nativeHost.unlock",
        password,
      },
      (response) => {
        if (!response.ok) {
          renderPasswordPrompt();
          renderMessage(`Unlock failed: ${response.error}`, "error");
          masterPasswordInput.focus();
          masterPasswordInput.select();
          return;
        }
        if (response.response.type !== "unlock") {
          renderPasswordPrompt();
          renderMessage(
            "Native host returned the wrong response type for unlock.",
            "error"
          );
          return;
        }
        vaultLocked = false;
        masterPasswordInput.value = "";
        renderPasswordPrompt();
        renderSavePrompt();
        updateFillButtonState();
        if (response.response.recoveryNotice) {
          recoveryNotice.hidden = false;
          recoveryNotice.textContent = response.response.recoveryNotice;
        }
        renderMessage("Vault unlocked.", "success");
        inspectPageContext();
        findSiteMatches();
      }
    );
    return;
  }

  if (!pendingFillMatch) {
    renderMessage("Choose a saved login before filling.", "error");
    return;
  }

  const secondaryPassword = pendingFillMatch.hasSecondaryPassword
    ? secondaryPasswordInput.value
    : "";
  if (pendingFillMatch.hasSecondaryPassword && !secondaryPassword) {
    renderMessage("Enter the secondary password for this login.", "error");
    secondaryPasswordInput.focus();
    return;
  }

  fillingEntryId = pendingFillMatch.id;
  renderMatchPicker();
  renderPasswordPrompt();
  updateFillButtonState();
  renderMessage(`Filling ${pendingFillMatch.name} into the current page...`);

  sendMessage(
    {
      type: "termkey.autofill.fillSelectedMatch",
      grantId: pendingFillMatch.grantId,
      entryId: pendingFillMatch.id,
      secondaryPassword: secondaryPassword || undefined,
    },
    (response) => {
      fillingEntryId = null;

      if (!response.ok) {
        renderMatchPicker();
        renderPasswordPrompt();
        updateFillButtonState();
        renderMessage(`Autofill failed: ${response.error}`, "error");
        if (pendingFillMatch?.hasSecondaryPassword) {
          secondaryPasswordInput.focus();
          secondaryPasswordInput.select();
        }
        return;
      }

      if (response.response.type !== "fill_result") {
        renderMatchPicker();
        renderPasswordPrompt();
        updateFillButtonState();
        renderMessage(
          "Background returned the wrong response type for autofill.",
          "error"
        );
        return;
      }

      pendingFillMatch = null;
      secondaryPasswordInput.value = "";
      renderMatchPicker();
      renderPasswordPrompt();
      updateFillButtonState();

      const result: PopupFillResultResponse = response.response;
      renderMessage(formatFillResultMessage(result), "success");
      inspectPageContext();
    }
  );
}

function submitPendingSave() {
  const manualCandidate = pendingSaveCandidate;
  const submittedLogin = pendingLoginCandidate;
  if (!manualCandidate && !submittedLogin) {
    renderMessage("Capture a login from the current page before saving it.", "error");
    return;
  }

  if (!backendConnected) {
    renderMessage("Reconnect the extension backend before saving a login.", "error");
    return;
  }

  const name = saveEntryNameInput.value.trim();
  if (!name) {
    renderMessage("Enter a name for this saved login.", "error");
    saveEntryNameInput.focus();
    return;
  }

  if (vaultLocked) {
    renderMessage("Unlock your vault before saving this login.", "error");
    masterPasswordInput.focus();
    return;
  }

  const secondaryPassword = saveUseSecondaryPasswordInput.checked
    ? saveSecondaryPasswordInput.value
    : "";
  const secondaryPasswordConfirm = saveUseSecondaryPasswordInput.checked
    ? saveSecondaryPasswordConfirmInput.value
    : "";

  if (saveUseSecondaryPasswordInput.checked && !secondaryPassword) {
    renderMessage("Enter a secondary password for this login.", "error");
    saveSecondaryPasswordInput.focus();
    return;
  }

  if (
    saveUseSecondaryPasswordInput.checked &&
    secondaryPassword !== secondaryPasswordConfirm
  ) {
    renderMessage("Secondary passwords do not match.", "error");
    saveSecondaryPasswordConfirmInput.focus();
    return;
  }

  saveInFlight = true;
  renderSavePrompt();
  updateFillButtonState();
  renderMessage(
    `${submittedLogin?.mode === "update" ? "Updating" : "Saving"} ${name} to your vault...`
  );

  let message: PopupToBackgroundMessage;
  if (submittedLogin) {
    message = {
      type: "termkey.pendingLogin.save",
      name,
      username: saveUsernameInput.value.trim() || undefined,
      secondaryPassword: secondaryPassword || undefined,
    };
  } else if (manualCandidate) {
    message = {
      type: "termkey.nativeHost.savePasswordEntry",
      name,
      username: saveUsernameInput.value.trim() || undefined,
      password: manualCandidate.password,
      url: manualCandidate.url,
      secondaryPassword: secondaryPassword || undefined,
    };
  } else {
    return;
  }

  sendMessage(
    message,
    (response) => {
      saveInFlight = false;

      if (!response.ok) {
        renderSavePrompt();
        updateFillButtonState();
        renderMessage(`Save failed: ${response.error}`, "error");
        return;
      }

      if (response.response.type !== "save_entry_result") {
        renderSavePrompt();
        updateFillButtonState();
        renderMessage(
          "Background returned the wrong response type for save.",
          "error"
        );
        return;
      }

      const result: PopupSaveResultResponse = response.response;
      clearPendingSave();
      renderSavePrompt();
      updateFillButtonState();
      renderMessage(`Saved ${result.entryName}.`, "success");
      findSiteMatches();
    },
    () => {
      saveInFlight = false;
      renderSavePrompt();
      updateFillButtonState();
    }
  );
}

function formatFillResultMessage(result: PopupFillResultResponse) {
  if (result.filledUsername && result.filledPassword) {
    return `Filled ${result.entryName}. Username and password updated.`;
  }

  if (result.filledUsername) {
    return `Filled username for ${result.entryName}. Password field is not visible yet.`;
  }

  if (result.filledPassword) {
    return `Filled password for ${result.entryName}.`;
  }

  return `Filled ${result.entryName}. ${result.filledFields} fields updated.`;
}

function sendMessage(
  message: PopupToBackgroundMessage,
  onSuccess: (response: PopupToBackgroundResponse) => void,
  onTransportFailure?: () => void
) {
  chrome.runtime.sendMessage(
    message,
    (response: PopupToBackgroundResponse | undefined) => {
      const runtimeError = chrome.runtime.lastError;
      if (runtimeError) {
        setBackendStatus(false, "Disconnected");
        renderMessage(`Background message failed: ${runtimeError.message}`, "error");
        onTransportFailure?.();
        return;
      }

      if (!response) {
        setBackendStatus(false, "Disconnected");
        renderMessage("No response received from the extension background.", "error");
        onTransportFailure?.();
        return;
      }

      onSuccess(response);
    }
  );
}

function handleSiteMatchFailure(error: string) {
  setCurrentSiteMatches([]);

  if (error === "Current tab is not a supported web page.") {
    setSiteVisibility(false);
    renderMessage(error, "error");
    return;
  }

  setSiteVisibility(true);
  renderSite(siteDetails.hostname, "Could not read saved logins for this page.");
  renderMessage(error, "error");
}

function inspectPageContext() {
  if (!hasSupportedPage) {
    setPageContext({
      intent: "unknown",
      visibleUsername: null,
      hasPasswordField: false,
      hasEmptyLoginField: false,
      hasConfirmationPasswordField: false,
      canGeneratePassword: false,
      hasPendingSaveUsername: false,
      pendingUsername: null,
    });
    return;
  }

  sendMessage(
    {
      type: "termkey.content.inspectPageContext",
    },
    (response) => {
      if (!response.ok) {
        return;
      }

      if (response.response.type !== "page_context") {
        return;
      }

      setPageContext(response.response.context);
    }
  );
}

function findSiteMatches() {
  if (!hasSupportedPage) {
    setCurrentSiteMatches([]);
    return;
  }

  renderSite(siteDetails.hostname, "Checking for a saved login...");

  const message: PopupToBackgroundMessage = {
    type: "termkey.nativeHost.findSiteMatches",
  };

  sendMessage(message, (response) => {
    if (!response.ok) {
      handleSiteMatchFailure(response.error);
      return;
    }

    if (response.response.type !== "site_matches") {
      handleSiteMatchFailure(
        "Native host returned the wrong response type for site matches."
      );
      return;
    }

    setCurrentSiteMatches(response.response.matches);
    setSiteVisibility(true);
    const matchSummary = describeMatches(response.response.matches);
    renderSite(
      response.response.siteOrigin,
      response.response.siteOrigin.startsWith("http://")
        ? `Unencrypted HTTP site. ${matchSummary}`
        : matchSummary
    );

    if (response.response.matches.length === 0) {
      renderMessage(`No saved login found for ${response.response.siteHostname}.`);
    } else if (response.response.matches.length === 1) {
      renderMessage(
        `Ready to fill ${response.response.matches[0].name}.`,
        "success"
      );
    } else {
      renderMessage("Use Fill on the saved login you want for this site.");
    }

    loadPendingLogin();
  });
}

function loadPendingLogin() {
  if (!hasSupportedPage || vaultLocked || pendingSaveCandidate !== null) {
    return;
  }

  sendMessage({ type: "termkey.pendingLogin.get" }, (response) => {
    if (!response.ok || response.response.type !== "pending_login") {
      return;
    }

    if (response.response.candidate) {
      stagePendingLogin(response.response.candidate);
    }
  });
}

function refreshStatus() {
  setBackendStatus(false, "Checking backend");
  renderMessage("Checking TermKey status...");

  const message: PopupToBackgroundMessage = { type: "termkey.nativeHost.status" };
  sendMessage(message, (response) => {
    if (!response.ok) {
      pendingFillMatch = null;
      clearPendingSave();
      renderPasswordPrompt();
      resetMatches("Backend unavailable.");
      setBackendStatus(false, "Disconnected");
      renderMessage(response.error, "error");
      return;
    }

    if (response.response.type !== "status") {
      pendingFillMatch = null;
      clearPendingSave();
      renderPasswordPrompt();
      resetMatches("Status check failed.");
      setBackendStatus(false, "Disconnected");
      renderMessage(
        "Native host returned the wrong response type for status.",
        "error"
      );
      return;
    }

    setBackendStatus(true, "Connected");
    vaultExists = response.response.vaultExists;
    vaultLocked = response.response.locked;

    if (!vaultExists) {
      pendingFillMatch = null;
      clearPendingSave();
      renderPasswordPrompt();
      resetMatches("Create your vault to save logins for this site.");
      renderMessage("Vault not found. Run `termkey init` first.", "error");
      return;
    }

    renderPasswordPrompt();
    renderSavePrompt();
    if (vaultLocked) {
      resetMatches("Unlock your vault to check saved logins.");
      renderMessage("Unlock TermKey to use saved logins.");
      inspectPageContext();
      return;
    }

    renderMessage("Checking this site...");
    inspectPageContext();
    findSiteMatches();
  });
}

fillButton.addEventListener("click", () => {
  const singleMatch = currentSiteMatches[0];
  if (!singleMatch) {
    renderMessage("No current-site match is available to fill.", "error");
    return;
  }

  beginFill(singleMatch);
});

generatePasswordButton.addEventListener("click", beginGeneratedPasswordFlow);
saveLoginButton.addEventListener("click", beginSave);
cancelSaveButton.addEventListener("click", cancelPendingSave);
submitSaveButton.addEventListener("click", submitPendingSave);
unlockVaultButton.addEventListener("click", submitPendingFill);

function cancelPendingSave() {
  if (pendingLoginCandidate) {
    dismissPendingLogin(() => {
      clearPendingSave();
      renderSavePrompt();
      updateFillButtonState();
      renderMessage("Save login cancelled.");
    });
    return;
  }

  clearPendingSave();
  renderSavePrompt();
  updateFillButtonState();
  renderMessage("Save login cancelled.");
}

function dismissPendingLogin(onDismissed: () => void) {
  sendMessage({ type: "termkey.pendingLogin.dismiss" }, (response) => {
    if (!response.ok) {
      renderMessage(`Could not dismiss this login: ${response.error}`, "error");
      return;
    }

    clearPendingSave();
    onDismissed();
  });
}

saveUseSecondaryPasswordInput.addEventListener("change", () => {
  if (!saveUseSecondaryPasswordInput.checked) {
    saveSecondaryPasswordInput.value = "";
    saveSecondaryPasswordConfirmInput.value = "";
  }
  renderSavePrompt();
});

saveEntryNameInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    submitPendingSave();
  }
});

saveUsernameInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    submitPendingSave();
  }
});

saveSecondaryPasswordInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    submitPendingSave();
  }
});

saveSecondaryPasswordConfirmInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    submitPendingSave();
  }
});

masterPasswordInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    submitPendingFill();
  }
});

secondaryPasswordInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    submitPendingFill();
  }
});

clearPendingSave();
renderPasswordPrompt();
renderSavePrompt();
updateFillButtonState();
primeCurrentSite();
refreshStatus();
