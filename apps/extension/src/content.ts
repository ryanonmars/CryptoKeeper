(() => {
type FillCredentialsMessage = {
  type: "termkey-fill-credentials";
  documentToken: string;
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

type CaptureVisibleCredentialsMessage = {
  type: "termkey.captureVisibleCredentials";
  documentToken: string;
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
    }
  | {
      ok: false;
      error: string;
    };

type InspectPageContextMessage = {
  type: "termkey.inspectPageContext";
};

type FillAttemptResult = {
  filledFields: number;
  filledUsername: boolean;
  filledPassword: boolean;
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
let pageContextNotificationQueued = false;

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

  return {
    ok: true,
    filledFields,
    filledUsername,
    filledPassword,
  };
}

function captureVisibleCredentials() {
  const inputs = collectInputElements();
  const inferred = inferPageIntent(inputs);
  const loginTargets = findBestLoginTargets(inputs);
  const generatedTargets = findGeneratedPasswordTargets(inputs);
  if (generatedTargets.ambiguous) {
    return {
      ok: false,
      error: "No visible password field was found on this page.",
    };
  }
  const useGeneratedTargets =
    inferred.intent === "signup" || inferred.intent === "password_change";
  const passwordInput = useGeneratedTargets
    ? generatedTargets.primaryPasswordInput
    : loginTargets.passwordInput;
  const usernameInput =
    (useGeneratedTargets
      ? generatedTargets.usernameInput
      : loginTargets.usernameInput) ??
    (passwordInput ? undefined : findBestUsernameInput(inputs, undefined));
  const username = usernameInput?.value.trim() || null;

  if (!passwordInput) {
    if (username) {
      return {
        ok: true,
        captureState: "username_only" as const,
        username,
      };
    }

    return {
      ok: false,
      error: "No visible password field was found on this page.",
    };
  }

  if (!passwordInput.value) {
    if (username) {
      return {
        ok: true,
        captureState: "username_only" as const,
        username,
      };
    }

    return {
      ok: false,
      error: "Type your password into the page before saving this login.",
    };
  }

  return {
    ok: true,
    captureState: username ? ("complete" as const) : ("password_only" as const),
    username,
    password: passwordInput.value,
  };
}

function captureSubmittedLoginFromInputs(inputs: HTMLInputElement[]): SubmittedLoginCapture {
  const inferred = inferPageIntent(inputs);
  const { passwordInput, usernameInput } = findBestLoginTargets(inputs);

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

  return {
    ok: true,
    username: usernameInput?.value.trim() || null,
    password: passwordInput.value,
  };
}

function captureSubmittedLogin(submittedForm: HTMLFormElement): SubmittedLoginCapture {
  return captureSubmittedLoginFromInputs(
    collectInputElements().filter((input) => input.form === submittedForm)
  );
}

function captureClickedLogin(button: HTMLElement): SubmittedLoginCapture {
  const root = getCandidateRoot(button);
  const inputs = root
    ? Array.from(root.querySelectorAll<HTMLInputElement>("input"))
    : collectInputElements();
  return captureSubmittedLoginFromInputs(inputs);
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
      | CaptureVisibleCredentialsMessage
      | CaptureSubmittedLoginMessage
      | InspectPageContextMessage,
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

    if (
      (message?.type === "termkey.captureVisibleCredentials" ||
        message?.type === "termkey.captureSubmittedLogin" ||
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

    if (message?.type === "termkey.captureVisibleCredentials") {
      sendResponse(captureVisibleCredentials());
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

document.addEventListener(
  "submit",
  (event) => {
    const snapshot =
      event.target instanceof HTMLFormElement
        ? captureSubmittedLogin(event.target)
        : undefined;
    submittedLoginSnapshot = snapshot;
    const notification = runtimeChrome?.runtime?.sendMessage?.({
      type: "termkey.content.loginSubmitted",
      documentToken: DOCUMENT_TOKEN,
    });
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
  },
  true
);

document.addEventListener(
  "click",
  (event) => {
    const button = event.target instanceof HTMLElement
      ? event.target.closest("button, input[type='submit'], input[type='button']")
      : null;
    if (
      !(button instanceof HTMLElement) ||
      (button instanceof HTMLButtonElement && button.type === "submit")
    ) {
      return;
    }
    const snapshot = captureClickedLogin(button);
    if (!snapshot.ok) {
      return;
    }
    submittedLoginSnapshot = snapshot;
    const notification = runtimeChrome?.runtime?.sendMessage?.({
      type: "termkey.content.loginSubmitted",
      documentToken: DOCUMENT_TOKEN,
    });
    if (notification && typeof (notification as PromiseLike<unknown>).then === "function") {
      const settled = Promise.resolve(notification).catch(() => undefined);
      submittedLoginNotification = settled;
      void settled.finally(() => {
        if (submittedLoginNotification === settled) submittedLoginNotification = undefined;
        if (submittedLoginSnapshot === snapshot) submittedLoginSnapshot = undefined;
      });
    }
  },
  true
);

if (runtimeChrome?.runtime) {
  wrapHistoryMethod("pushState");
  wrapHistoryMethod("replaceState");
  window.addEventListener("popstate", schedulePageContextChanged);
  window.addEventListener("hashchange", schedulePageContextChanged);
  new MutationObserver(schedulePageContextChanged).observe(document, {
    childList: true,
    subtree: true,
  });
  console.log("TermKey content script running");
}
})();
