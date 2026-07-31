(() => {
type FillCredentialsMessage = {
  type: "termkey-fill-credentials";
  documentToken: string;
  autofillReceipt: string;
  username?: string;
  password: string;
};

type FillGeneratedPasswordMessage = {
  type: "termkey.fillGeneratedPassword";
  documentToken: string;
  password: string;
};

type ContentScriptProbeMessage = {
  type: "termkey.contentScriptProbe";
};

type CaptureSubmittedLoginMessage = {
  type: "termkey.captureSubmittedLogin";
  documentToken: string;
};

type SubmittedLoginCapture =
  | {
      ok: true;
      username: string | null;
      password: string;
      autofillReceipt?: string;
    }
  | {
      ok: false;
      error: string;
    };

type InspectPageContextMessage = {
  type: "termkey.inspectPageContext";
};

type PendingLoginPromptContentMessage =
  | {
      type: "termkey.pendingLoginPrompt.mount";
      candidateId: string;
      documentToken: string;
    }
  | {
      type: "termkey.pendingLoginPrompt.remove";
      candidateId: string;
    }
  | {
      type: "termkey.pendingLoginPrompt.complete";
      candidateId: string;
      outcome: "saved" | "updated";
    };

type FillAttemptResult = {
  filledFields: number;
  filledUsername: boolean;
  filledPassword: boolean;
};

type InlineAutofillMatch = {
  id: string;
  grantId: string;
  name: string;
  username: string | null;
  hasSecondaryPassword: boolean;
};

type LoginTargets = {
  passwordInput?: HTMLInputElement;
  usernameInput?: HTMLInputElement;
};

type GeneratedPasswordTargets = {
  ambiguous?: boolean;
  primaryPasswordInput?: HTMLInputElement;
  confirmationPasswordInput?: HTMLInputElement;
  usernameInput?: HTMLInputElement;
  primaryScore?: number;
};

const FILL_RETRY_DELAYS_MS = [0, 150, 350, 700] as const;
const SUBMITTED_LOGIN_DEDUPE_MS = 1_000;
const LOGIN_ACTION_SELECTOR =
  "button, input[type='submit'], input[type='button'], [role='button']";
const PROMPT_IFRAME_ID = "termkey-pending-login-prompt";
const INLINE_AUTOFILL_HOST_ID = "termkey-inline-autofill";
const DOCUMENT_TOKEN = Array.from(
  crypto.getRandomValues(new Uint8Array(32)),
  (byte) => byte.toString(16).padStart(2, "0")
).join("");
const runtimeChrome = typeof chrome === "undefined" ? undefined : chrome;
const contentGlobal = globalThis as typeof globalThis & {
  __termkeyContentScriptLoaded?: boolean;
};
let submittedLoginSnapshot: SubmittedLoginCapture | undefined;
let submittedLoginNotification: Promise<unknown> | undefined;
let recentlyNotifiedSubmittedLogin: SubmittedLoginCapture | undefined;
let submittedLoginDedupeTimer: number | undefined;
let rememberedLoginUsername: string | undefined;
let recentTermKeyFill:
  | {
      receipt: string;
      username: string | null;
      password: string;
    }
  | undefined;
let pageContextNotificationQueued = false;
let mountedPromptCandidateId: string | undefined;
let inlineAutofillTarget: HTMLInputElement | undefined;
let inlineAutofillHost: HTMLDivElement | undefined;
let inlineAutofillButton: HTMLButtonElement | undefined;
let inlineAutofillMenu: HTMLDivElement | undefined;
let inlineAutofillRequestVersion = 0;
let inlineAutofillCloseTimer: number | undefined;

if (runtimeChrome?.runtime && contentGlobal.__termkeyContentScriptLoaded) {
  return;
}
if (runtimeChrome?.runtime) {
  contentGlobal.__termkeyContentScriptLoaded = true;
}

function sleep(delayMs: number) {
  return new Promise<void>((resolve) => {
    window.setTimeout(resolve, delayMs);
  });
}

function isPendingLoginPromptCandidateId(candidateId: unknown) {
  return typeof candidateId === "string" && /^[a-f0-9]{64}$/.test(candidateId);
}

function removeMountedPrompt() {
  document.getElementById(PROMPT_IFRAME_ID)?.remove();
  mountedPromptCandidateId = undefined;
}

function mountPendingLoginPrompt(candidateId: string) {
  if (
    mountedPromptCandidateId === candidateId &&
    document.getElementById(PROMPT_IFRAME_ID)
  ) {
    return;
  }

  removeMountedPrompt();
  const iframe = document.createElement("iframe");
  iframe.id = PROMPT_IFRAME_ID;
  iframe.src =
    `${runtimeChrome!.runtime.getURL("prompt.html")}#candidate=${candidateId}`;
  iframe.title = "TermKey password save prompt";
  iframe.referrerPolicy = "no-referrer";
  iframe.setAttribute("title", "TermKey password save prompt");
  iframe.setAttribute("allow", "");
  iframe.setAttribute("referrerpolicy", "no-referrer");
  Object.assign(iframe.style, {
    position: "fixed",
    top: "16px",
    right: "16px",
    width: "min(380px, calc(100vw - 32px))",
    height: "280px",
    border: "0",
    background: "transparent",
    zIndex: "2147483647",
  });
  mountedPromptCandidateId = candidateId;
  document.documentElement.append(iframe);
}

function removePendingLoginPrompt(candidateId: string) {
  if (mountedPromptCandidateId === candidateId) {
    removeMountedPrompt();
  }
}

function completePendingLoginPrompt(
  candidateId: string,
  _outcome: "saved" | "updated"
) {
  if (mountedPromptCandidateId !== candidateId) {
    return;
  }

  window.setTimeout(() => removePendingLoginPrompt(candidateId), 900);
}

function getInputType(input: HTMLInputElement) {
  return (input.getAttribute("type") ?? "text").toLowerCase();
}

function isVisibleInput(input: HTMLInputElement) {
  const rect = input.getBoundingClientRect();
  const style = window.getComputedStyle(input);

  if (getInputType(input) === "hidden") {
    return false;
  }

  return (
    rect.width > 0 &&
    rect.height > 0 &&
    style.visibility !== "hidden" &&
    style.display !== "none" &&
    style.opacity !== "0" &&
    style.pointerEvents !== "none" &&
    !input.disabled &&
    !input.readOnly &&
    !input.closest("[aria-hidden='true']")
  );
}

function setInputValue(input: HTMLInputElement, value: string) {
  input.focus();

  const prototype = Object.getPrototypeOf(input) as HTMLInputElement;
  const descriptor = Object.getOwnPropertyDescriptor(prototype, "value");

  if (descriptor?.set) {
    descriptor.set.call(input, value);
  } else {
    input.value = value;
  }

  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
}

function getInputText(input: HTMLInputElement, attribute: string) {
  return (input.getAttribute(attribute) ?? "").toLowerCase();
}

function getAutocompleteTokens(input: HTMLInputElement) {
  return input.autocomplete
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);
}

function collectInputElements() {
  const seen = new Set<HTMLInputElement>();

  function visit(root: ParentNode) {
    if (!("querySelectorAll" in root)) {
      return;
    }

    root.querySelectorAll<HTMLInputElement>("input").forEach((input) => {
      seen.add(input);
    });

    root.querySelectorAll<HTMLElement>("*").forEach((element) => {
      if (element.shadowRoot) {
        visit(element.shadowRoot);
      }
    });
  }

  visit(document);
  return Array.from(seen);
}

function getInputLabelText(input: HTMLInputElement) {
  const labels = new Set<string>();

  input.labels?.forEach((label) => {
    labels.add(label.textContent ?? "");
  });

  const wrappingLabel = input.closest("label");
  if (wrappingLabel) {
    labels.add(wrappingLabel.textContent ?? "");
  }

  return Array.from(labels).join(" ").toLowerCase();
}

function getInputDescriptor(input: HTMLInputElement) {
  const form = input.form;

  return [
    input.name,
    input.id,
    input.placeholder,
    input.autocomplete,
    getInputText(input, "aria-label"),
    getInputText(input, "data-testid"),
    getInputText(input, "data-qa"),
    getInputText(input, "data-test"),
    getInputLabelText(input),
    form?.getAttribute("aria-label") ?? "",
    form?.getAttribute("name") ?? "",
    form?.getAttribute("id") ?? "",
  ]
    .join(" ")
    .toLowerCase();
}

function getCandidateRoot(element: HTMLElement | null | undefined) {
  return (
    element?.closest("form, [role='dialog'], dialog, [role='form'], main, section, article") ??
    null
  );
}

function getCandidateContext(element: HTMLElement) {
  const candidateRoot = getCandidateRoot(element);
  if (candidateRoot) {
    return candidateRoot;
  }

  const treeRoot = element.getRootNode();
  return treeRoot instanceof ShadowRoot ? treeRoot : document.body;
}

function getContextBoost(input: HTMLInputElement) {
  let score = 0;
  const activeElement = document.activeElement;

  if (activeElement === input) {
    score += 10;
  }

  if (!(activeElement instanceof HTMLElement)) {
    return score;
  }

  const activeRoot = getCandidateRoot(activeElement);
  if (activeRoot?.contains(input)) {
    score += 6;
  }

  if (input.form && activeElement instanceof HTMLElement && input.form.contains(activeElement)) {
    score += 6;
  }

  return score;
}

function isUsernameCompatibleInput(input: HTMLInputElement) {
  const type = getInputType(input);
  const autocompleteTokens = getAutocompleteTokens(input);

  return (
    type === "text" ||
    type === "email" ||
    type === "tel" ||
    type === "search" ||
    autocompleteTokens.includes("username") ||
    autocompleteTokens.includes("email")
  );
}

function getUsernameCandidateScore(
  input: HTMLInputElement,
  passwordInput: HTMLInputElement | undefined
) {
  if (!isVisibleInput(input) || !isUsernameCompatibleInput(input)) {
    return Number.NEGATIVE_INFINITY;
  }

  const type = getInputType(input);
  const autocompleteTokens = getAutocompleteTokens(input);
  const descriptor = getInputDescriptor(input);

  let score = 0;

  if (autocompleteTokens.includes("username")) {
    score += 14;
  }

  if (autocompleteTokens.includes("email")) {
    score += 10;
  }

  if (type === "email") {
    score += 8;
  }

  if (type === "tel") {
    score += 5;
  }

  if (
    /user|email|login|identifier|account|member|customer|phone|mobile/.test(
      descriptor
    )
  ) {
    score += 6;
  }

  if (/search|coupon|promo|filter|captcha/.test(descriptor)) {
    score -= 8;
  }

  if (/otp|code|2fa|pass|password|pin/.test(descriptor)) {
    score -= 12;
  }

  if (type === "search") {
    score -= 6;
  }

  if (passwordInput) {
    if (passwordInput.form && input.form === passwordInput.form) {
      score += 10;
    }

    const passwordRoot = getCandidateRoot(passwordInput);
    if (passwordRoot?.contains(input)) {
      score += 6;
    }

    if (input.compareDocumentPosition(passwordInput) & Node.DOCUMENT_POSITION_FOLLOWING) {
      score += 4;
    }
  }

  return score + getContextBoost(input);
}

function getPasswordCandidateScore(input: HTMLInputElement) {
  if (!isVisibleInput(input) || getInputType(input) !== "password") {
    return Number.NEGATIVE_INFINITY;
  }

  const autocompleteTokens = getAutocompleteTokens(input);
  const descriptor = getInputDescriptor(input);

  if (
    autocompleteTokens.includes("one-time-code") ||
    autocompleteTokens.includes("new-password") ||
    /confirm|confirmation|repeat|verify|re-enter/.test(descriptor) ||
    /otp|one.?time|2fa|verification.?code/.test(descriptor)
  ) {
    return Number.NEGATIVE_INFINITY;
  }

  let score = 0;

  if (autocompleteTokens.includes("current-password")) {
    score += 18;
  }

  if (autocompleteTokens.includes("password")) {
    score += 8;
  }

  if (autocompleteTokens.includes("new-password")) {
    score -= 14;
  }

  if (/pass|password|passcode|pwd|secret/.test(descriptor)) {
    score += 4;
  }

  if (/confirm|confirmation|repeat|verify|re-enter/.test(descriptor)) {
    score -= 14;
  }

  if (/otp|one.?time|2fa|code|search|coupon|promo/.test(descriptor)) {
    score -= 12;
  }

  return score + getContextBoost(input);
}

function findBestUsernameCandidate(
  inputs: HTMLInputElement[],
  passwordInput: HTMLInputElement | undefined,
  requireSharedContext: boolean
) {
  return inputs
    .map((input) => ({
      input,
      score: getUsernameCandidateScore(input, passwordInput),
    }))
    .filter(
      (
        candidate
      ): candidate is { input: HTMLInputElement; score: number } =>
        Number.isFinite(candidate.score) &&
        candidate.score > 0 &&
        (!requireSharedContext ||
          sharesCandidateContext(candidate.input, passwordInput))
    )
    .sort((left, right) => right.score - left.score)[0];
}

function findBestUsernameInput(
  inputs: HTMLInputElement[],
  passwordInput: HTMLInputElement | undefined,
  requireSharedContext = false
) {
  return findBestUsernameCandidate(
    inputs,
    passwordInput,
    requireSharedContext
  )?.input;
}

function sharesCandidateContext(
  left: HTMLInputElement,
  right: HTMLInputElement | undefined
) {
  if (!right) {
    return false;
  }

  if (left.form || right.form) {
    return Boolean(left.form && right.form && left.form === right.form);
  }

  return getCandidateContext(left) === getCandidateContext(right);
}

function compareLoginGroups(
  left: {
    usernameInput?: HTMLInputElement;
    groupScore: number;
  },
  right: {
    usernameInput?: HTMLInputElement;
    groupScore: number;
  }
) {
  const pairDifference =
    Number(Boolean(right.usernameInput)) -
    Number(Boolean(left.usernameInput));
  return pairDifference || right.groupScore - left.groupScore;
}

function findBestLoginTargets(inputs: HTMLInputElement[]): LoginTargets {
  const groups = inputs
    .map((passwordInput) => ({
      passwordInput,
      passwordScore: getPasswordCandidateScore(passwordInput),
    }))
    .filter(
      (
        candidate
      ): candidate is {
        passwordInput: HTMLInputElement;
        passwordScore: number;
      } =>
        Number.isFinite(candidate.passwordScore) &&
        candidate.passwordScore >= 0
    )
    .map((candidate) => {
      const usernameCandidate = findBestUsernameCandidate(
        inputs,
        candidate.passwordInput,
        true
      );
      return {
        ...candidate,
        usernameInput: usernameCandidate?.input,
        groupScore:
          candidate.passwordScore + (usernameCandidate?.score ?? 0),
      };
    })
    .sort(compareLoginGroups);

  if (
    groups.length > 1 &&
    compareLoginGroups(groups[0], groups[1]) === 0
  ) {
    return {};
  }

  return groups[0] ?? {};
}

function hasGeneratedPasswordSemanticSignal(text: string) {
  return /(?:new|create|choose|set|reset|change|update)[\s_-]{0,3}(?:your[\s_-]+)?(?:pass(?:word|code)?|secret|credential)|(?:pass(?:word|code)?|secret|credential)[\s_-]{0,3}(?:new|reset|change|update)|sign.?up|signup|register|registration|create[\s_-]+(?:your[\s_-]+)?account|add[\s_-]+(?:member|user|account)|invite/.test(
    text
  );
}

function getGeneratedFieldIntentText(input: HTMLInputElement) {
  const labels = new Set<string>();
  input.labels?.forEach((label) => {
    if (!label.querySelector("a, nav, [role='link'], [role='navigation']")) {
      labels.add(label.textContent ?? "");
    }
  });

  return [
    input.name,
    input.id,
    input.placeholder,
    input.autocomplete,
    getInputText(input, "aria-label"),
    getInputText(input, "data-testid"),
    getInputText(input, "data-qa"),
    getInputText(input, "data-test"),
    ...labels,
  ]
    .join(" ")
    .toLowerCase();
}

function getGeneratedStructuralIntentText(input: HTMLInputElement) {
  const root = input.form ?? getCandidateRoot(input);
  if (!root) {
    return "";
  }

  const intentText = [
    root.getAttribute("id") ?? "",
    root.getAttribute("name") ?? "",
    root.getAttribute("aria-label") ?? "",
  ];
  root
    .querySelectorAll<HTMLElement>(
      "h1, h2, h3, h4, h5, h6, legend, button:not([type]), button[type='submit'], input[type='submit']"
    )
    .forEach((element) => {
      if (
        element.closest("a, nav, [role='link'], [role='navigation']") ||
        element.querySelector("a, nav, [role='link'], [role='navigation']")
      ) {
        return;
      }

      intentText.push(
        element instanceof HTMLInputElement
          ? element.value
          : element.textContent ?? ""
      );
    });

  return intentText.join(" ").toLowerCase().replace(/\s+/g, " ").trim();
}

function hasLoginSemanticSignal(text: string) {
  return /sign.?in|log.?in|login/.test(text);
}

function getGeneratedPasswordCandidateScore(input: HTMLInputElement) {
  if (!isVisibleInput(input) || getInputType(input) !== "password") {
    return Number.NEGATIVE_INFINITY;
  }

  const autocompleteTokens = getAutocompleteTokens(input);
  const descriptor = getInputDescriptor(input);
  const fieldIntentText = getGeneratedFieldIntentText(input);
  const structuralIntentText = getGeneratedStructuralIntentText(input);

  if (
    autocompleteTokens.includes("one-time-code") ||
    autocompleteTokens.includes("current-password") ||
    /current|old|existing/.test(descriptor) ||
    /otp|one.?time|2fa|code|pin|verification/.test(descriptor)
  ) {
    return Number.NEGATIVE_INFINITY;
  }

  const hasStrongFieldSignal =
    autocompleteTokens.includes("new-password") ||
    hasGeneratedPasswordSemanticSignal(fieldIntentText);
  const hasStructuralGeneratedSignal =
    hasGeneratedPasswordSemanticSignal(structuralIntentText);
  const hasStructuralLoginSignal =
    hasLoginSemanticSignal(structuralIntentText);
  if (
    !hasStrongFieldSignal &&
    (!hasStructuralGeneratedSignal || hasStructuralLoginSignal)
  ) {
    return Number.NEGATIVE_INFINITY;
  }

  let semanticScore = 0;
  if (autocompleteTokens.includes("new-password")) {
    semanticScore += 20;
  }

  if (hasGeneratedPasswordSemanticSignal(fieldIntentText)) {
    semanticScore += 10;
  }

  if (
    hasStructuralGeneratedSignal &&
    (!hasStructuralLoginSignal || hasStrongFieldSignal)
  ) {
    semanticScore += 10;
  }

  if (semanticScore === 0) {
    return Number.NEGATIVE_INFINITY;
  }

  let score = semanticScore;

  if (/confirm|confirmation|repeat|verify|re-enter/.test(descriptor)) {
    score -= 8;
  }

  if (hasStructuralLoginSignal) {
    score -= 12;
  }

  return score + getContextBoost(input);
}

function getConfirmationPasswordScore(
  input: HTMLInputElement,
  primaryPasswordInput: HTMLInputElement | undefined
) {
  if (
    !isVisibleInput(input) ||
    getInputType(input) !== "password" ||
    input === primaryPasswordInput
  ) {
    return Number.NEGATIVE_INFINITY;
  }

  const autocompleteTokens = getAutocompleteTokens(input);
  const descriptor = getInputDescriptor(input);

  if (
    !sharesCandidateContext(input, primaryPasswordInput) ||
    autocompleteTokens.includes("one-time-code") ||
    autocompleteTokens.includes("current-password") ||
    /current|old|existing/.test(descriptor) ||
    /otp|one.?time|2fa|code|pin|verification/.test(descriptor)
  ) {
    return Number.NEGATIVE_INFINITY;
  }

  const hasConfirmationDescriptor =
    /confirm|confirmation|repeat|verify|re-enter|match/.test(descriptor);
  const followsPrimary = Boolean(
    primaryPasswordInput &&
      input.compareDocumentPosition(primaryPasswordInput) &
        Node.DOCUMENT_POSITION_PRECEDING
  );
  if (
    !hasConfirmationDescriptor &&
    !(autocompleteTokens.includes("new-password") && followsPrimary)
  ) {
    return Number.NEGATIVE_INFINITY;
  }

  let score = 0;

  if (hasConfirmationDescriptor) {
    score += 18;
  }

  if (autocompleteTokens.includes("new-password")) {
    score += 8;
  }

  score += 8;

  if (followsPrimary) {
    score += 6;
  }

  return score + getContextBoost(input);
}

function findGeneratedPasswordTargets(
  inputs: HTMLInputElement[]
): GeneratedPasswordTargets {
  const primaryCandidates = inputs
    .map((input) => ({
      input,
      score: getGeneratedPasswordCandidateScore(input),
    }))
    .filter(
      (
        candidate
      ): candidate is { input: HTMLInputElement; score: number } =>
        Number.isFinite(candidate.score) &&
        candidate.score > 0 &&
        !looksLikeConfirmationPassword(candidate.input)
    )
    .sort((left, right) => right.score - left.score);

  if (primaryCandidates.length === 0) {
    return {};
  }

  const groups = primaryCandidates
    .map((primaryCandidate) => {
      const confirmationCandidate = inputs
        .map((input) => ({
          input,
          score: getConfirmationPasswordScore(
            input,
            primaryCandidate.input
          ),
        }))
        .filter(
          (
            candidate
          ): candidate is { input: HTMLInputElement; score: number } =>
            Number.isFinite(candidate.score) && candidate.score > 0
        )
        .sort((left, right) => right.score - left.score)[0];
      const usernameCandidate = findBestUsernameCandidate(
        inputs,
        primaryCandidate.input,
        true
      );
      return {
        primaryPasswordInput: primaryCandidate.input,
        confirmationPasswordInput: confirmationCandidate?.input,
        usernameInput: usernameCandidate?.input,
        primaryScore: primaryCandidate.score,
        groupScore:
          primaryCandidate.score +
          (confirmationCandidate?.score ?? 0) +
          (usernameCandidate?.score ?? 0),
      };
    })
    .sort(compareGeneratedPasswordGroups);

  if (
    groups.length > 1 &&
    compareGeneratedPasswordGroups(groups[0], groups[1]) === 0
  ) {
    return { ambiguous: true };
  }

  return groups[0] ?? {};
}

function compareGeneratedPasswordGroups(
  left: {
    confirmationPasswordInput?: HTMLInputElement;
    groupScore: number;
  },
  right: {
    confirmationPasswordInput?: HTMLInputElement;
    groupScore: number;
  }
) {
  const pairDifference =
    Number(Boolean(right.confirmationPasswordInput)) -
    Number(Boolean(left.confirmationPasswordInput));
  return pairDifference || right.groupScore - left.groupScore;
}

function canGeneratePasswordForInputs(inputs: HTMLInputElement[]) {
  const targets = findGeneratedPasswordTargets(inputs);
  return Boolean(
    targets.primaryPasswordInput &&
      typeof targets.primaryScore === "number" &&
      Number.isFinite(targets.primaryScore) &&
      targets.primaryScore > 0
  );
}

function hasAutocompleteToken(
  input: HTMLInputElement,
  token: "current-password" | "new-password"
) {
  return getAutocompleteTokens(input).includes(token);
}

function looksLikeConfirmationPassword(input: HTMLInputElement) {
  return /confirm|confirmation|repeat|verify|re-enter|match/.test(
    getInputDescriptor(input)
  );
}

function inferPageIntent(inputs: HTMLInputElement[]) {
  const loginTargets = findBestLoginTargets(inputs);
  const generatedTargets = findGeneratedPasswordTargets(inputs);
  const hasPasswordField = Boolean(
    loginTargets.passwordInput || generatedTargets.primaryPasswordInput
  );
  const hasConfirmationPasswordField = Boolean(
    generatedTargets.confirmationPasswordInput
  );
  const hasCurrentPasswordInGeneratedContext = inputs.some(
    (input) =>
      isVisibleInput(input) &&
      getInputType(input) === "password" &&
      hasAutocompleteToken(input, "current-password") &&
      sharesCandidateContext(input, generatedTargets.primaryPasswordInput)
  );
  const hasUsernameField = Boolean(
    loginTargets.usernameInput ||
      generatedTargets.usernameInput ||
      findBestUsernameInput(inputs, undefined)
  );

  if (
    generatedTargets.primaryPasswordInput &&
    hasCurrentPasswordInGeneratedContext
  ) {
    return {
      intent: "password_change" as const,
      hasPasswordField,
      hasConfirmationPasswordField,
    };
  }

  if (generatedTargets.primaryPasswordInput) {
    return {
      intent: "signup" as const,
      hasPasswordField,
      hasConfirmationPasswordField,
    };
  }

  if (hasPasswordField || hasUsernameField) {
    return {
      intent: "login" as const,
      hasPasswordField,
      hasConfirmationPasswordField,
    };
  }

  return {
    intent: "unknown" as const,
    hasPasswordField: false,
    hasConfirmationPasswordField: false,
  };
}

function fillVisibleCredentials(
  message: FillCredentialsMessage
): FillAttemptResult {
  const inputs = collectInputElements();
  const targets = findBestLoginTargets(inputs);
  const passwordInput = targets.passwordInput;
  const usernameInput = message.username
    ? targets.usernameInput
    : undefined;

  let filledUsername = false;
  let filledPassword = false;

  if (message.username && usernameInput) {
    setInputValue(usernameInput, message.username);
    filledUsername = true;
  }

  if (passwordInput) {
    setInputValue(passwordInput, message.password);
    filledPassword = true;
  }

  return {
    filledFields: Number(filledUsername) + Number(filledPassword),
    filledUsername,
    filledPassword,
  };
}

async function fillCredentials(message: FillCredentialsMessage) {
  let filledUsername = false;
  let filledPassword = false;

  for (const delayMs of FILL_RETRY_DELAYS_MS) {
    if (delayMs > 0) {
      await sleep(delayMs);
    }

    const attempt = fillVisibleCredentials(message);
    filledUsername ||= attempt.filledUsername;
    filledPassword ||= attempt.filledPassword;

    if (filledPassword) {
      break;
    }
  }

  const filledFields = Number(filledUsername) + Number(filledPassword);
  if (filledFields === 0) {
    if (message.username) {
      return {
        ok: false,
        error: "No visible username or password field was found on this page.",
      };
    }

    return {
      ok: false,
      error: "No visible password field was found on this page.",
    };
  }

  if (
    filledPassword &&
    /^[a-f0-9]{64}$/.test(message.autofillReceipt)
  ) {
    recentTermKeyFill = {
      receipt: message.autofillReceipt,
      username: message.username?.trim() || null,
      password: message.password,
    };
  }

  return {
    ok: true,
    filledFields,
    filledUsername,
    filledPassword,
  };
}

function captureSubmittedLoginFromInputs(inputs: HTMLInputElement[]): SubmittedLoginCapture {
  const inferred = inferPageIntent(inputs);
  const { passwordInput, usernameInput } = findBestLoginTargets(inputs);
  const currentUsername = usernameInput?.value.trim();
  if (currentUsername) {
    rememberedLoginUsername = currentUsername;
  }

  if (
    inferred.intent !== "login" ||
    !passwordInput ||
    !passwordInput.value.trim()
  ) {
    return {
      ok: false,
      error: "No visible submitted login credentials were found on this page.",
    };
  }

  const capture = {
    ok: true,
    username: currentUsername || rememberedLoginUsername || null,
    password: passwordInput.value,
  } as const;
  if (
    recentTermKeyFill &&
    recentTermKeyFill.username === capture.username &&
    recentTermKeyFill.password === capture.password
  ) {
    return {
      ...capture,
      autofillReceipt: recentTermKeyFill.receipt,
    };
  }
  recentTermKeyFill = undefined;
  return capture;
}

function captureSubmittedLogin(submittedForm: HTMLFormElement): SubmittedLoginCapture {
  return captureSubmittedLoginFromInputs(
    collectInputElements().filter((input) => input.form === submittedForm)
  );
}

function captureLoginAction(element: HTMLElement): SubmittedLoginCapture {
  const root = getCandidateRoot(element);
  const inputs = root
    ? Array.from(root.querySelectorAll<HTMLInputElement>("input"))
    : collectInputElements();
  return captureSubmittedLoginFromInputs(inputs);
}

function rememberLoginUsername(input: HTMLInputElement) {
  const username = input.value.trim();
  const score = getUsernameCandidateScore(input, undefined);
  if (
    username &&
    Number.isFinite(score) &&
    score > 0
  ) {
    rememberedLoginUsername = username;
  }
}

function sameSubmittedLogin(
  left: SubmittedLoginCapture | undefined,
  right: SubmittedLoginCapture | undefined
) {
  return (
    left?.ok === true &&
    right?.ok === true &&
    left.username === right.username &&
    left.password === right.password &&
    left.autofillReceipt === right.autofillReceipt
  );
}

function stageSubmittedLogin(snapshot: SubmittedLoginCapture | undefined) {
  if (sameSubmittedLogin(recentlyNotifiedSubmittedLogin, snapshot)) {
    return;
  }
  recentlyNotifiedSubmittedLogin = snapshot;
  if (submittedLoginDedupeTimer !== undefined) {
    window.clearTimeout(submittedLoginDedupeTimer);
  }
  submittedLoginDedupeTimer = window.setTimeout(() => {
    recentlyNotifiedSubmittedLogin = undefined;
    submittedLoginDedupeTimer = undefined;
  }, SUBMITTED_LOGIN_DEDUPE_MS);
  submittedLoginSnapshot = snapshot;
  notifySubmittedLogin(snapshot);
}

function findLoginAction(event: Event) {
  return event.composedPath().find(
    (candidate): candidate is HTMLElement =>
      candidate instanceof HTMLElement &&
      candidate.matches(LOGIN_ACTION_SELECTOR)
  );
}

function captureLoginActionEvent(event: Event) {
  const action = findLoginAction(event);
  if (!action) {
    return;
  }
  const snapshot = captureLoginAction(action);
  if (snapshot.ok) {
    stageSubmittedLogin(snapshot);
  }
}

function notifySubmittedLogin(snapshot: SubmittedLoginCapture | undefined) {
  const message = snapshot?.ok
    ? {
        type: "termkey.content.loginSubmitted" as const,
        documentToken: DOCUMENT_TOKEN,
        username: snapshot.username,
        password: snapshot.password,
        ...(snapshot.autofillReceipt
          ? { autofillReceipt: snapshot.autofillReceipt }
          : {}),
      }
    : {
        type: "termkey.content.loginSubmitted" as const,
        documentToken: DOCUMENT_TOKEN,
      };
  const notification = runtimeChrome?.runtime?.sendMessage?.(message);
  if (
    notification &&
    typeof (notification as PromiseLike<unknown>).then === "function"
  ) {
    const settled = Promise.resolve(notification).catch(() => undefined);
    submittedLoginNotification = settled;
    void settled.finally(() => {
      if (submittedLoginNotification === settled) {
        submittedLoginNotification = undefined;
      }
      if (submittedLoginSnapshot === snapshot) {
        submittedLoginSnapshot = undefined;
      }
    });
  }
}

function fillGeneratedPassword(message: FillGeneratedPasswordMessage) {
  const inputs = collectInputElements();
  const {
    primaryPasswordInput,
    confirmationPasswordInput,
    usernameInput,
  } = findGeneratedPasswordTargets(inputs);

  if (!primaryPasswordInput) {
    return {
      ok: false,
      error:
        "No visible signup password field was found on this page. Open the account creation form first.",
    };
  }

  setInputValue(primaryPasswordInput, message.password);

  let filledPasswordFields = 1;
  if (
    confirmationPasswordInput &&
    confirmationPasswordInput !== primaryPasswordInput
  ) {
    setInputValue(confirmationPasswordInput, message.password);
    filledPasswordFields += 1;
  }

  return {
    ok: true,
    username: usernameInput?.value.trim() || null,
    filledPasswordFields,
  };
}

function inspectPageContext() {
  const inputs = collectInputElements();
  const inferred = inferPageIntent(inputs);
  const loginTargets = findBestLoginTargets(inputs);
  const generatedTargets = findGeneratedPasswordTargets(inputs);
  const usernameInput =
    inferred.intent === "signup" || inferred.intent === "password_change"
      ? generatedTargets.usernameInput
      : loginTargets.usernameInput ??
        findBestUsernameInput(inputs, undefined);
  const hasEmptyLoginField =
    inferred.intent === "login" &&
    (loginTargets.usernameInput?.value.trim() === "" ||
      Boolean(loginTargets.passwordInput && !loginTargets.passwordInput.value));

  return {
    ok: true,
    intent: inferred.intent,
    visibleUsername: usernameInput?.value.trim() || null,
    hasPasswordField: inferred.hasPasswordField,
    hasEmptyLoginField,
    hasConfirmationPasswordField: inferred.hasConfirmationPasswordField,
    canGeneratePassword: canGeneratePasswordForInputs(inputs),
    hasVisibleLoginFailure:
      inferred.intent === "login" && hasVisibleLoginFailure(),
  };
}

function sendInlineAutofillMessage(
  message: Record<string, unknown>,
  callback: (response: unknown) => void
) {
  if (!runtimeChrome?.runtime?.sendMessage) {
    callback({
      ok: false,
      error: "TermKey extension messaging is unavailable.",
    });
    return;
  }
  runtimeChrome.runtime.sendMessage(message, callback);
}

function inlineAutofillLogo() {
  const logo = document.createElement("img");
  logo.src = runtimeChrome?.runtime?.getURL(
    "public/icons/termkey-icon-32.png"
  ) ?? "";
  logo.alt = "";
  logo.draggable = false;
  return logo;
}

function ensureInlineAutofillUi() {
  if (
    inlineAutofillHost &&
    inlineAutofillButton &&
    inlineAutofillMenu
  ) {
    return;
  }

  const host = document.createElement("div");
  host.id = INLINE_AUTOFILL_HOST_ID;
  host.dataset.state = "idle";
  host.style.cssText =
    "all:initial;position:fixed;inset:0;z-index:2147483646;pointer-events:none;";
  const shadow = host.attachShadow({ mode: "closed" });
  const style = document.createElement("style");
  style.textContent = `
    :host { all: initial; }
    * { box-sizing: border-box; }
    .trigger {
      position: fixed;
      display: none;
      width: 26px;
      height: 26px;
      padding: 3px;
      border: 0;
      border-radius: 7px;
      color: #65d7ff;
      background: #102438;
      box-shadow: 0 0 0 1px rgba(101, 215, 255, .52), 0 5px 14px rgba(0, 0, 0, .28);
      cursor: pointer;
      pointer-events: auto;
    }
    .trigger:hover, .trigger:focus-visible, .trigger[data-open="true"] {
      color: #93e5ff;
      background: #17334d;
      box-shadow: 0 0 0 2px rgba(101, 215, 255, .72), 0 7px 18px rgba(0, 0, 0, .34);
      outline: none;
    }
    .trigger img { display: block; width: 20px; height: 20px; object-fit: contain; }
    .menu {
      position: fixed;
      display: none;
      overflow: hidden;
      min-width: 280px;
      max-width: min(380px, calc(100vw - 20px));
      border: 1px solid #33435a;
      border-radius: 12px;
      color: #eef4ff;
      background: #111a28;
      box-shadow: 0 18px 48px rgba(0, 0, 0, .48);
      font: 500 14px/1.35 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      pointer-events: auto;
    }
    .brand {
      padding: 10px 13px 8px;
      border-bottom: 1px solid #26354a;
      color: #65d7ff;
      font-size: 12px;
      font-weight: 750;
      letter-spacing: .08em;
      text-transform: uppercase;
    }
    .row {
      display: grid;
      grid-template-columns: 34px minmax(0, 1fr) auto;
      gap: 10px;
      align-items: center;
      width: 100%;
      padding: 12px 13px;
      border: 0;
      border-bottom: 1px solid #26354a;
      color: inherit;
      background: transparent;
      text-align: left;
      cursor: pointer;
    }
    .row:last-child { border-bottom: 0; }
    .row:hover, .row:focus-visible { background: #19263a; outline: none; }
    .mark {
      display: grid;
      place-items: center;
      width: 34px;
      height: 34px;
      border-radius: 9px;
      color: #65d7ff;
      background: #102b40;
    }
    .mark img { width: 24px; height: 24px; object-fit: contain; }
    .copy { min-width: 0; }
    .name, .detail { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .name { color: #f7f9ff; font-size: 15px; font-weight: 700; }
    .detail { margin-top: 2px; color: #aebbd0; font-size: 13px; }
    .action { color: #65d7ff; font-size: 13px; font-weight: 750; }
    .status { padding: 15px 14px; color: #b8c5d8; }
    @media (prefers-reduced-motion: no-preference) {
      .menu { animation: termkey-menu-in 120ms ease-out; transform-origin: top right; }
      @keyframes termkey-menu-in {
        from { opacity: 0; transform: translateY(-4px) scale(.985); }
        to { opacity: 1; transform: translateY(0) scale(1); }
      }
    }
  `;
  const trigger = document.createElement("button");
  trigger.type = "button";
  trigger.className = "trigger";
  trigger.title = "Fill with TermKey";
  trigger.setAttribute("aria-label", "Show TermKey saved logins");
  trigger.append(inlineAutofillLogo());
  const menu = document.createElement("div");
  menu.className = "menu";
  menu.setAttribute("role", "dialog");
  menu.setAttribute("aria-label", "TermKey saved logins");

  trigger.addEventListener("pointerdown", (event) => event.preventDefault());
  trigger.addEventListener("click", () => {
    if (menu.style.display === "block") {
      closeInlineAutofillMenu();
    } else {
      openInlineAutofillMenu();
    }
  });
  menu.addEventListener("pointerdown", (event) => event.preventDefault());

  shadow.append(style, trigger, menu);
  (document.documentElement ?? document.body).append(host);
  inlineAutofillHost = host;
  inlineAutofillButton = trigger;
  inlineAutofillMenu = menu;
}

function positionInlineAutofillUi() {
  if (
    !inlineAutofillTarget ||
    !inlineAutofillHost ||
    !inlineAutofillButton ||
    !inlineAutofillMenu
  ) {
    return;
  }
  const rect = inlineAutofillTarget.getBoundingClientRect();
  const visible =
    rect.width >= 70 &&
    rect.height >= 18 &&
    rect.bottom > 0 &&
    rect.top < window.innerHeight;
  if (!visible) {
    inlineAutofillButton.style.display = "none";
    inlineAutofillMenu.style.display = "none";
    inlineAutofillHost.dataset.state = "hidden";
    return;
  }

  const triggerSize = 26;
  inlineAutofillButton.style.display = "block";
  inlineAutofillButton.style.left = `${Math.max(
    4,
    Math.min(window.innerWidth - triggerSize - 4, rect.right - triggerSize - 5)
  )}px`;
  inlineAutofillButton.style.top = `${Math.max(
    4,
    rect.top + (rect.height - triggerSize) / 2
  )}px`;
  const menuWidth = Math.max(280, Math.min(380, rect.width));
  const availableMenuWidth = Math.max(180, window.innerWidth - 20);
  const renderedMenuWidth = Math.min(menuWidth, availableMenuWidth);
  inlineAutofillMenu.style.width = `${renderedMenuWidth}px`;
  inlineAutofillMenu.style.left = `${Math.max(
    10,
    Math.min(window.innerWidth - renderedMenuWidth - 10, rect.left)
  )}px`;
  const menuHeight = inlineAutofillMenu.getBoundingClientRect().height;
  const belowTop = rect.bottom + 7;
  const aboveTop = rect.top - menuHeight - 7;
  inlineAutofillMenu.style.top = `${
    menuHeight > 0 &&
    belowTop + menuHeight > window.innerHeight - 10 &&
    aboveTop >= 10
      ? aboveTop
      : Math.max(10, Math.min(window.innerHeight - 12, belowTop))
  }px`;
  inlineAutofillHost.dataset.state =
    inlineAutofillMenu.style.display === "block" ? "open" : "ready";
}

function closeInlineAutofillMenu() {
  inlineAutofillRequestVersion += 1;
  if (inlineAutofillCloseTimer !== undefined) {
    window.clearTimeout(inlineAutofillCloseTimer);
    inlineAutofillCloseTimer = undefined;
  }
  if (inlineAutofillMenu) {
    inlineAutofillMenu.style.display = "none";
  }
  inlineAutofillButton?.setAttribute("data-open", "false");
  if (inlineAutofillHost && inlineAutofillTarget) {
    inlineAutofillHost.dataset.state = "ready";
  }
}

function hideInlineAutofillUi() {
  inlineAutofillTarget = undefined;
  inlineAutofillRequestVersion += 1;
  if (inlineAutofillCloseTimer !== undefined) {
    window.clearTimeout(inlineAutofillCloseTimer);
    inlineAutofillCloseTimer = undefined;
  }
  inlineAutofillButton?.setAttribute("data-open", "false");
  if (inlineAutofillButton) inlineAutofillButton.style.display = "none";
  if (inlineAutofillMenu) inlineAutofillMenu.style.display = "none";
  if (inlineAutofillHost) inlineAutofillHost.dataset.state = "hidden";
}

function renderInlineAutofillStatus(message: string) {
  if (!inlineAutofillMenu) return;
  inlineAutofillMenu.replaceChildren();
  const brand = document.createElement("div");
  brand.className = "brand";
  brand.textContent = "TermKey";
  const status = document.createElement("div");
  status.className = "status";
  status.textContent = message;
  inlineAutofillMenu.append(brand, status);
}

function createInlineAutofillRow(
  name: string,
  detail: string,
  action: string,
  onSelect: () => void
) {
  const row = document.createElement("button");
  row.type = "button";
  row.className = "row";
  const mark = document.createElement("span");
  mark.className = "mark";
  mark.append(inlineAutofillLogo());
  const copy = document.createElement("span");
  copy.className = "copy";
  const title = document.createElement("span");
  title.className = "name";
  title.textContent = name;
  const subtitle = document.createElement("span");
  subtitle.className = "detail";
  subtitle.textContent = detail;
  copy.append(title, subtitle);
  const actionLabel = document.createElement("span");
  actionLabel.className = "action";
  actionLabel.textContent = action;
  row.append(mark, copy, actionLabel);
  row.addEventListener("click", onSelect);
  return row;
}

function openTermKeyPopup(match?: InlineAutofillMatch) {
  renderInlineAutofillStatus("Opening TermKey…");
  sendInlineAutofillMessage(
    {
      type: "termkey.content.inlineAutofill.openPopup",
      documentToken: DOCUMENT_TOKEN,
      ...(match
        ? {
            grantId: match.grantId,
            entryId: match.id,
            name: match.name,
            username: match.username,
            hasSecondaryPassword: match.hasSecondaryPassword,
          }
        : {}),
    },
    (response) => {
      if (
        typeof response === "object" &&
        response !== null &&
        "ok" in response &&
        response.ok === true
      ) {
        closeInlineAutofillMenu();
        return;
      }
      renderInlineAutofillStatus(
        typeof response === "object" &&
          response !== null &&
          "error" in response &&
          typeof response.error === "string"
          ? response.error
          : "Open the TermKey extension to continue."
      );
    }
  );
}

function fillInlineAutofillMatch(match: InlineAutofillMatch) {
  if (match.hasSecondaryPassword) {
    openTermKeyPopup(match);
    return;
  }
  renderInlineAutofillStatus(`Filling ${match.name}…`);
  sendInlineAutofillMessage(
    {
      type: "termkey.content.inlineAutofill.fill",
      documentToken: DOCUMENT_TOKEN,
      grantId: match.grantId,
      entryId: match.id,
    },
    (response) => {
      if (
        typeof response === "object" &&
        response !== null &&
        "ok" in response &&
        response.ok === true
      ) {
        hideInlineAutofillUi();
        return;
      }
      if (
        typeof response === "object" &&
        response !== null &&
        "refreshMatches" in response &&
        response.refreshMatches === true
      ) {
        openInlineAutofillMenu();
        return;
      }
      renderInlineAutofillStatus(
        typeof response === "object" &&
          response !== null &&
          "error" in response &&
          typeof response.error === "string"
          ? response.error
          : "TermKey could not fill this login."
      );
    }
  );
}

function renderInlineAutofillResponse(response: unknown) {
  if (
    !inlineAutofillMenu ||
    typeof response !== "object" ||
    response === null ||
    !("ok" in response) ||
    response.ok !== true ||
    !("response" in response) ||
    typeof response.response !== "object" ||
    response.response === null ||
    !("type" in response.response) ||
    response.response.type !== "inline_autofill"
  ) {
    const error =
      typeof response === "object" &&
      response !== null &&
      "error" in response &&
      typeof response.error === "string"
        ? response.error
        : "TermKey could not check this login.";
    renderInlineAutofillStatus(error);
    return;
  }

  const payload = response.response as {
    state?: string;
    siteHostname?: string;
    matches?: unknown[];
  };
  if (inlineAutofillHost) {
    inlineAutofillHost.dataset.mode =
      typeof payload.state === "string" ? payload.state : "error";
  }
  inlineAutofillMenu.replaceChildren();
  const brand = document.createElement("div");
  brand.className = "brand";
  brand.textContent = "TermKey";
  inlineAutofillMenu.append(brand);

  if (payload.state === "locked") {
    inlineAutofillMenu.append(
      createInlineAutofillRow(
        "Unlock TermKey",
        "Open the extension to unlock and fill",
        "Unlock",
        openTermKeyPopup
      )
    );
    return;
  }
  if (payload.state === "missing_vault") {
    const status = document.createElement("div");
    status.className = "status";
    status.textContent = "Create a TermKey vault before using autofill.";
    inlineAutofillMenu.append(status);
    return;
  }
  const matches = Array.isArray(payload.matches)
    ? payload.matches.filter((match): match is InlineAutofillMatch => {
        if (typeof match !== "object" || match === null) return false;
        const candidate = match as Partial<InlineAutofillMatch>;
        return (
          typeof candidate.id === "string" &&
          typeof candidate.grantId === "string" &&
          /^[a-f0-9]{64}$/.test(candidate.grantId) &&
          typeof candidate.name === "string" &&
          (typeof candidate.username === "string" ||
            candidate.username === null) &&
          typeof candidate.hasSecondaryPassword === "boolean"
        );
      })
    : [];
  if (matches.length === 0) {
    const status = document.createElement("div");
    status.className = "status";
    status.textContent = `No saved login for ${
      typeof payload.siteHostname === "string"
        ? payload.siteHostname
        : "this site"
    }.`;
    inlineAutofillMenu.append(status);
    return;
  }
  for (const match of matches) {
    inlineAutofillMenu.append(
      createInlineAutofillRow(
        match.name,
        match.username ?? "Password login",
        match.hasSecondaryPassword ? "Open" : "Fill",
        () => fillInlineAutofillMatch(match)
      )
    );
  }
}

function openInlineAutofillMenu() {
  if (
    !inlineAutofillTarget ||
    !inlineAutofillButton ||
    !inlineAutofillMenu
  ) {
    return;
  }
  const requestVersion = ++inlineAutofillRequestVersion;
  if (inlineAutofillCloseTimer !== undefined) {
    window.clearTimeout(inlineAutofillCloseTimer);
  }
  inlineAutofillCloseTimer = window.setTimeout(
    closeInlineAutofillMenu,
    25_000
  );
  inlineAutofillMenu.style.display = "block";
  inlineAutofillButton.setAttribute("data-open", "true");
  renderInlineAutofillStatus("Checking saved logins…");
  positionInlineAutofillUi();
  sendInlineAutofillMessage(
    {
      type: "termkey.content.inlineAutofill.request",
      documentToken: DOCUMENT_TOKEN,
    },
    (response) => {
      if (requestVersion !== inlineAutofillRequestVersion) return;
      renderInlineAutofillResponse(response);
      positionInlineAutofillUi();
    }
  );
}

function showInlineAutofillFor(target: HTMLInputElement) {
  const inputs = collectInputElements();
  const inferred = inferPageIntent(inputs);
  const loginTargets = findBestLoginTargets(inputs);
  if (
    inferred.intent !== "login" ||
    (target !== loginTargets.usernameInput &&
      target !== loginTargets.passwordInput) ||
    (loginTargets.usernameInput?.value.trim() !== "" &&
      Boolean(loginTargets.passwordInput?.value))
  ) {
    hideInlineAutofillUi();
    return;
  }
  inlineAutofillTarget = target;
  ensureInlineAutofillUi();
  positionInlineAutofillUi();
  openInlineAutofillMenu();
}

function hasVisibleLoginFailure() {
  return Array.from(
    document.querySelectorAll<HTMLElement>("[role='alert'], .error, .alert")
  ).some((element) => {
    const text = element.textContent?.trim() ?? "";
    return (
      isVisibleElement(element) &&
      /invalid|incorrect|failed|try again/i.test(text)
    );
  });
}

function isVisibleElement(element: HTMLElement) {
  for (
    let current: HTMLElement | null = element;
    current;
    current = current.parentElement
  ) {
    const style = window.getComputedStyle(current);
    if (
      current.hidden ||
      current.getAttribute("aria-hidden") === "true" ||
      style.display === "none" ||
      style.visibility === "hidden" ||
      style.opacity === "0"
    ) {
      return false;
    }
  }

  return true;
}

runtimeChrome?.runtime?.onMessage?.addListener(
  (
    message:
      | FillCredentialsMessage
      | FillGeneratedPasswordMessage
      | ContentScriptProbeMessage
      | CaptureSubmittedLoginMessage
      | InspectPageContextMessage
      | PendingLoginPromptContentMessage,
    sender: { id?: string },
    sendResponse: (response: unknown) => void
  ) => {
    if (sender.id !== runtimeChrome.runtime.id) {
      return false;
    }

    if (message?.type === "termkey.contentScriptProbe") {
      sendResponse({ ok: true, documentToken: DOCUMENT_TOKEN });
      return true;
    }

    if (message?.type === "termkey.pendingLoginPrompt.mount") {
      if (
        isPendingLoginPromptCandidateId(message.candidateId) &&
        message.documentToken === DOCUMENT_TOKEN
      ) {
        mountPendingLoginPrompt(message.candidateId);
      }
      return false;
    }

    if (message?.type === "termkey.pendingLoginPrompt.remove") {
      if (isPendingLoginPromptCandidateId(message.candidateId)) {
        removePendingLoginPrompt(message.candidateId);
      }
      return false;
    }

    if (message?.type === "termkey.pendingLoginPrompt.complete") {
      if (
        isPendingLoginPromptCandidateId(message.candidateId) &&
        (message.outcome === "saved" || message.outcome === "updated")
      ) {
        completePendingLoginPrompt(message.candidateId, message.outcome);
      }
      return false;
    }

    if (
      (message?.type === "termkey.captureSubmittedLogin" ||
        message?.type === "termkey.fillGeneratedPassword" ||
        message?.type === "termkey-fill-credentials") &&
      message.documentToken !== DOCUMENT_TOKEN
    ) {
      sendResponse({
        ok: false,
        error: "The page document changed before delivery.",
      });
      return true;
    }

    if (message?.type === "termkey.captureSubmittedLogin") {
      const snapshot = submittedLoginSnapshot ?? {
        ok: false as const,
        error: "No submitted login credentials are available.",
      };
      submittedLoginSnapshot = undefined;
      sendResponse(snapshot);
      return true;
    }

    if (message?.type === "termkey.inspectPageContext") {
      sendResponse({
        ...inspectPageContext(),
        documentToken: DOCUMENT_TOKEN,
      });
      return true;
    }

    if (message?.type === "termkey.fillGeneratedPassword") {
      sendResponse(fillGeneratedPassword(message));
      return true;
    }

    if (message?.type !== "termkey-fill-credentials") {
      return false;
    }

    void fillCredentials(message)
      .then(sendResponse)
      .catch((error) => {
        sendResponse({
          ok: false,
          error:
            error instanceof Error
              ? error.message
              : "Content script failed while filling the page.",
        });
      });

    return true;
  }
);

function sendPageContextChanged() {
  pageContextNotificationQueued = false;
  const notify = () =>
    runtimeChrome?.runtime?.sendMessage?.({
      type: "termkey.content.pageContextChanged",
      documentToken: DOCUMENT_TOKEN,
    });
  if (submittedLoginNotification) {
    void submittedLoginNotification.then(notify);
  } else {
    void notify();
  }
}

function schedulePageContextChanged() {
  if (pageContextNotificationQueued) {
    return;
  }
  pageContextNotificationQueued = true;
  queueMicrotask(sendPageContextChanged);
}

function wrapHistoryMethod(method: "pushState" | "replaceState") {
  const original = window.history[method].bind(window.history);
  window.history[method] = ((
    data: unknown,
    unused: string,
    url?: string | URL | null
  ) => {
    original(data, unused, url);
    schedulePageContextChanged();
  }) as History["pushState"];
}

window.addEventListener(
  "submit",
  (event) => {
    const snapshot =
      event.target instanceof HTMLFormElement
        ? captureSubmittedLogin(event.target)
        : undefined;
    stageSubmittedLogin(snapshot);
  },
  true
);

window.addEventListener("pointerdown", captureLoginActionEvent, true);
window.addEventListener("click", captureLoginActionEvent, true);

window.addEventListener(
  "keydown",
  (event) => {
    if (
      event.key !== "Enter" ||
      event.repeat ||
      event.isComposing ||
      event.altKey ||
      event.ctrlKey ||
      event.metaKey ||
      !(event.target instanceof HTMLInputElement) ||
      getInputType(event.target) !== "password"
    ) {
      return;
    }
    const snapshot = captureLoginAction(event.target);
    if (snapshot.ok) {
      stageSubmittedLogin(snapshot);
    }
  },
  true
);

document.addEventListener(
  "focusin",
  (event) => {
    if (event.target instanceof HTMLInputElement) {
      showInlineAutofillFor(event.target);
    }
  },
  true
);

window.addEventListener(
  "input",
  (event) => {
    if (event.target instanceof HTMLInputElement) {
      rememberLoginUsername(event.target);
    }
    if (!inlineAutofillTarget) return;
    const context = inspectPageContext();
    if (!context.hasEmptyLoginField) {
      hideInlineAutofillUi();
    }
  },
  true
);

document.addEventListener(
  "pointerdown",
  (event) => {
    if (
      inlineAutofillHost &&
      !event.composedPath().includes(inlineAutofillHost) &&
      event.target !== inlineAutofillTarget
    ) {
      closeInlineAutofillMenu();
    }
  },
  true
);

if (runtimeChrome?.runtime) {
  wrapHistoryMethod("pushState");
  wrapHistoryMethod("replaceState");
  window.addEventListener("popstate", schedulePageContextChanged);
  window.addEventListener("hashchange", schedulePageContextChanged);
  window.addEventListener("resize", positionInlineAutofillUi);
  window.addEventListener("scroll", positionInlineAutofillUi, true);
  window.addEventListener(
    "pagehide",
    () => {
      removeMountedPrompt();
      inlineAutofillHost?.remove();
    },
    { once: true }
  );
  new MutationObserver(() => {
    schedulePageContextChanged();
    positionInlineAutofillUi();
  }).observe(document, {
    childList: true,
    subtree: true,
  });
  console.log("TermKey content script running");
}
})();
