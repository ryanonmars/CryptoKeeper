import type {
  PendingLoginPromptMetadata,
  PendingLoginPromptMode,
  PendingLoginPromptToBackgroundMessage,
  PopupToBackgroundResponse,
} from "@termkey/types";

const candidateId = new URLSearchParams(
  window.location.hash.replace(/^#/, "")
).get("candidate");

const candidateIdPattern = /^[a-f0-9]{64}$/;
const promptModes: ReadonlySet<string> = new Set([
  "save",
  "update",
  "unlock",
  "protected-update",
  "resolve",
]);
const fallbackInstruction =
  "Click the TermKey toolbar icon to continue. This login is still available.";

const primaryLabel: Record<PendingLoginPromptMode, string> = {
  save: "Save",
  update: "Update",
  unlock: "Unlock & Save",
  "protected-update": "Update",
  resolve: "Retry",
};

function sendPromptMessage(
  message: PendingLoginPromptToBackgroundMessage
): Promise<PopupToBackgroundResponse> {
  return chrome.runtime.sendMessage(message);
}

function createElement<K extends keyof HTMLElementTagNameMap>(
  tagName: K,
  className?: string,
  text?: string
) {
  const element = document.createElement(tagName);
  if (className) {
    element.className = className;
  }
  if (text !== undefined) {
    element.textContent = text;
  }
  return element;
}

function installStyles() {
  const style = document.createElement("style");
  style.textContent = `
    :root {
      color-scheme: dark;
      font-family: "IBM Plex Sans", "Segoe UI", system-ui, sans-serif;
      background: transparent;
      color: #f3f4f6;
    }

    * {
      box-sizing: border-box;
    }

    html,
    body {
      width: 100%;
      min-width: 0;
      min-height: 100%;
      margin: 0;
      overflow-x: hidden;
      background: transparent;
    }

    body {
      padding: 10px;
    }

    button {
      font: inherit;
    }

    .prompt {
      display: grid;
      gap: 10px;
      width: 100%;
      max-width: 360px;
      margin-left: auto;
      padding: 13px;
      overflow: hidden;
      border: 1px solid rgba(148, 163, 184, 0.28);
      border-radius: 16px;
      background: #0f172a;
      box-shadow: 0 12px 30px rgba(2, 6, 23, 0.34);
      animation: prompt-enter 150ms ease-out both;
    }

    .prompt__header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      min-width: 0;
    }

    .prompt__identity {
      display: flex;
      align-items: center;
      gap: 8px;
      min-width: 0;
    }

    .prompt__mark {
      display: grid;
      width: 24px;
      height: 24px;
      flex: 0 0 auto;
      place-items: center;
      border: 1px solid rgba(125, 211, 252, 0.35);
      border-radius: 8px;
      background: #111d2f;
      color: #7dd3fc;
      font-size: 12px;
      font-weight: 800;
      letter-spacing: -0.04em;
    }

    .prompt__brand {
      margin: 0;
      color: #7dd3fc;
      font-size: 13px;
      font-weight: 700;
      letter-spacing: -0.02em;
    }

    .prompt__state {
      margin: 1px 0 0;
      overflow: hidden;
      color: #93a4b8;
      font-size: 10px;
      line-height: 1.2;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .prompt__dismiss {
      display: grid;
      width: 30px;
      height: 30px;
      flex: 0 0 auto;
      padding: 0;
      place-items: center;
      border: 1px solid transparent;
      border-radius: 9px;
      background: transparent;
      color: #93a4b8;
      cursor: pointer;
      font-size: 21px;
      line-height: 1;
      transition:
        border-color 140ms ease,
        background-color 140ms ease,
        color 140ms ease;
    }

    .prompt__dismiss:hover {
      border-color: rgba(148, 163, 184, 0.24);
      background: #162033;
      color: #f3f4f6;
    }

    .prompt__dismiss:focus-visible,
    .button:focus-visible {
      outline: 2px solid #7dd3fc;
      outline-offset: 2px;
    }

    .prompt__title {
      margin: 0;
      color: #f8fafc;
      font-size: 17px;
      font-weight: 700;
      line-height: 1.25;
      letter-spacing: -0.02em;
    }

    .prompt__context {
      display: grid;
      grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
      gap: 8px;
    }

    .context-item {
      min-width: 0;
      padding: 8px 9px;
      border: 1px solid rgba(148, 163, 184, 0.16);
      border-radius: 10px;
      background: #0b1220;
    }

    .context-item__label {
      display: block;
      margin-bottom: 3px;
      color: #7890a8;
      font-size: 9px;
      font-weight: 700;
      letter-spacing: 0.08em;
      line-height: 1;
      text-transform: uppercase;
    }

    .context-item__value {
      display: block;
      overflow: hidden;
      color: #dbe5f0;
      font-size: 12px;
      line-height: 1.3;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .prompt__notice,
    .prompt__status {
      margin: 0;
      color: #a9b7c7;
      font-size: 11px;
      line-height: 1.35;
    }

    .prompt__notices {
      display: grid;
      gap: 2px;
    }

    .prompt__notice--http {
      color: #fbbf24;
    }

    .prompt__status {
      min-height: 15px;
      color: #cbd5e1;
    }

    .prompt__status--error {
      color: #fca5a5;
    }

    .prompt__status--success {
      color: #86efac;
      animation: success-settle 160ms ease-out both;
    }

    .prompt__actions {
      display: flex;
      align-items: center;
      gap: 8px;
      min-width: 0;
    }

    .button {
      min-height: 34px;
      border-radius: 10px;
      cursor: pointer;
      font-weight: 700;
      transition:
        background-color 140ms ease,
        border-color 140ms ease,
        color 140ms ease,
        transform 140ms ease;
    }

    .button:hover:not(:disabled) {
      transform: translateY(-1px);
    }

    .button:disabled {
      cursor: default;
      opacity: 0.58;
    }

    .button--primary {
      min-width: 104px;
      padding: 7px 14px;
      border: 1px solid #22c55e;
      background: #22c55e;
      color: #052e16;
    }

    .button--primary:hover:not(:disabled) {
      border-color: #4ade80;
      background: #4ade80;
    }

    .button--quiet {
      padding: 7px 10px;
      border: 1px solid transparent;
      background: transparent;
      color: #a9b7c7;
    }

    .button--quiet:hover:not(:disabled) {
      border-color: rgba(148, 163, 184, 0.22);
      background: #162033;
      color: #f3f4f6;
    }

    [hidden] {
      display: none !important;
    }

    @keyframes prompt-enter {
      from {
        opacity: 0;
        transform: translateY(-5px);
      }
      to {
        opacity: 1;
        transform: translateY(0);
      }
    }

    @keyframes success-settle {
      from {
        opacity: 0.5;
        transform: translateY(2px);
      }
      to {
        opacity: 1;
        transform: translateY(0);
      }
    }

    @media (prefers-reduced-motion: reduce) {
      *,
      *::before,
      *::after {
        scroll-behavior: auto !important;
        animation-duration: 0.01ms !important;
        animation-iteration-count: 1 !important;
        transition-duration: 0.01ms !important;
      }
    }
  `;
  document.head.append(style);
}

function renderUnavailable() {
  const card = createElement("main", "prompt");
  card.setAttribute("role", "status");

  const brand = createElement("p", "prompt__brand", "TermKey");
  const title = createElement(
    "p",
    "prompt__title",
    "This login is no longer available"
  );
  card.append(brand, title);
  document.body.replaceChildren(card);
}

function isPromptMetadata(value: unknown): value is PendingLoginPromptMetadata {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.candidateId === "string" &&
    candidateIdPattern.test(candidate.candidateId) &&
    typeof candidate.origin === "string" &&
    typeof candidate.hostname === "string" &&
    (typeof candidate.username === "string" || candidate.username === null) &&
    typeof candidate.defaultName === "string" &&
    typeof candidate.mode === "string" &&
    promptModes.has(candidate.mode) &&
    typeof candidate.isHttp === "boolean"
  );
}

function renderPrompt(metadata: PendingLoginPromptMetadata) {
  const card = createElement("main", "prompt");
  card.setAttribute("role", "dialog");
  card.setAttribute("aria-labelledby", "prompt-title");

  const header = createElement("header", "prompt__header");
  const identity = createElement("div", "prompt__identity");
  const mark = createElement("span", "prompt__mark", "TK");
  mark.setAttribute("aria-hidden", "true");
  const identityCopy = createElement("div");
  const brand = createElement("p", "prompt__brand", "TermKey");
  const state = createElement("p", "prompt__state", "Login detected");
  identityCopy.append(brand, state);
  identity.append(mark, identityCopy);

  const dismiss = createElement("button", "prompt__dismiss", "×");
  dismiss.type = "button";
  dismiss.setAttribute("aria-label", "Dismiss");
  header.append(identity, dismiss);

  const titleText =
    metadata.mode === "update" || metadata.mode === "protected-update"
      ? "Update existing login"
      : metadata.mode === "resolve"
        ? "TermKey needs attention"
        : "Save this login?";
  const title = createElement("h1", "prompt__title", titleText);
  title.id = "prompt-title";

  const context = createElement("div", "prompt__context");
  const site = createElement("div", "context-item");
  const siteLabel = createElement("span", "context-item__label", "Site");
  const siteValue = createElement(
    "span",
    "context-item__value",
    metadata.hostname
  );
  site.append(siteLabel, siteValue);

  const account = createElement("div", "context-item");
  const accountLabel = createElement(
    "span",
    "context-item__label",
    "Username"
  );
  const accountValue = createElement(
    "span",
    "context-item__value",
    metadata.username ?? "Not detected"
  );
  account.append(accountLabel, accountValue);
  context.append(site, account);

  const notices = createElement("div", "prompt__notices");
  if (metadata.isHttp) {
    notices.append(
      createElement(
        "p",
        "prompt__notice prompt__notice--http",
        "Unencrypted HTTP site"
      )
    );
  }
  if (metadata.mode === "protected-update") {
    notices.append(
      createElement(
        "p",
        "prompt__notice",
        "The toolbar popup will request the secondary password."
      )
    );
  } else if (metadata.mode === "unlock") {
    notices.append(
      createElement(
        "p",
        "prompt__notice",
        "Continue in the toolbar popup to unlock TermKey."
      )
    );
  } else if (metadata.mode === "resolve") {
    notices.append(
      createElement(
        "p",
        "prompt__notice",
        "Continue in the toolbar popup to retry."
      )
    );
  }
  if (!notices.hasChildNodes()) {
    notices.hidden = true;
  }

  const status = createElement("p", "prompt__status");
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");

  const actions = createElement("div", "prompt__actions");
  const primary = createElement(
    "button",
    "button button--primary",
    primaryLabel[metadata.mode]
  );
  primary.type = "button";
  primary.dataset.action = "primary";

  const moreOptions = createElement(
    "button",
    "button button--quiet",
    "More options"
  );
  moreOptions.type = "button";
  moreOptions.dataset.action = "more-options";
  actions.append(primary, moreOptions);

  card.append(header, title, context, notices, status, actions);
  document.body.replaceChildren(card);

  const actionControls = [primary, moreOptions, dismiss];
  let busy = false;
  let complete = false;

  function setBusy(nextBusy: boolean) {
    busy = nextBusy;
    for (const control of actionControls) {
      control.disabled = nextBusy || complete;
    }
    card.setAttribute("aria-busy", String(nextBusy));
  }

  function setStatus(message: string, kind?: "error" | "success") {
    status.textContent = message;
    status.classList.toggle("prompt__status--error", kind === "error");
    status.classList.toggle("prompt__status--success", kind === "success");
  }

  async function runAction(message: PendingLoginPromptToBackgroundMessage) {
    if (busy || complete) {
      return;
    }
    setBusy(true);
    setStatus("");
    try {
      const result = await sendPromptMessage(message);
      if (!result.ok) {
        setStatus(result.error, "error");
        return;
      }
      if (result.response.type !== "pending_login_prompt_result") {
        setStatus("TermKey could not complete this action. Try again.", "error");
        return;
      }
      switch (result.response.outcome) {
        case "saved":
          complete = true;
          setStatus("Saved", "success");
          break;
        case "updated":
          complete = true;
          setStatus("Updated", "success");
          break;
        case "dismissed":
          complete = true;
          setStatus("Dismissed");
          break;
        case "popup-opened":
          setStatus("Continue in the TermKey toolbar popup.");
          break;
        case "popup-required":
          setStatus(
            result.response.fallbackInstruction ?? fallbackInstruction,
            "error"
          );
          break;
      }
    } catch {
      setStatus("TermKey could not complete this action. Try again.", "error");
    } finally {
      setBusy(false);
    }
  }

  primary.addEventListener("click", () => {
    const message: PendingLoginPromptToBackgroundMessage =
      metadata.mode === "save" || metadata.mode === "update"
        ? {
            type: "termkey.pendingLoginPrompt.save",
            candidateId: metadata.candidateId,
          }
        : {
            type: "termkey.pendingLoginPrompt.openPopup",
            candidateId: metadata.candidateId,
            reason:
              metadata.mode === "unlock"
                ? "unlock"
                : metadata.mode === "protected-update"
                  ? "secondary-password"
                  : "retry",
          };
    void runAction(message);
  });

  moreOptions.addEventListener("click", () => {
    void runAction({
      type: "termkey.pendingLoginPrompt.openPopup",
      candidateId: metadata.candidateId,
      reason: "more-options",
    });
  });

  dismiss.addEventListener("click", () => {
    void runAction({
      type: "termkey.pendingLoginPrompt.dismiss",
      candidateId: metadata.candidateId,
    });
  });
}

async function initialize() {
  installStyles();
  if (candidateId === null || !candidateIdPattern.test(candidateId)) {
    renderUnavailable();
    return;
  }

  try {
    const result = await sendPromptMessage({
      type: "termkey.pendingLoginPrompt.get",
      candidateId,
    });
    if (
      !result.ok ||
      result.response.type !== "pending_login_prompt" ||
      result.response.candidate === null ||
      !isPromptMetadata(result.response.candidate) ||
      result.response.candidate.candidateId !== candidateId
    ) {
      renderUnavailable();
      return;
    }
    renderPrompt(result.response.candidate);
  } catch {
    renderUnavailable();
  }
}

void initialize();
