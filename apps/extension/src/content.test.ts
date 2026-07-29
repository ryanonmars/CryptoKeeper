// @vitest-environment jsdom

import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { MockChromeEvent } from "./test/chrome-mock";

type ContentListener = (
  message: unknown,
  sender: { id?: string },
  sendResponse: (response: unknown) => void
) => boolean;

let listener: ContentListener;
let documentToken: string;
let sendMessage: ReturnType<typeof vi.fn>;

beforeEach(async () => {
  vi.resetModules();
  delete (
    globalThis as typeof globalThis & {
      __termkeyContentScriptLoaded?: boolean;
    }
  ).__termkeyContentScriptLoaded;
  document.body.innerHTML = "";
  Object.defineProperty(HTMLInputElement.prototype, "getBoundingClientRect", {
    configurable: true,
    value: () => ({
      width: 160,
      height: 24,
      top: 0,
      right: 160,
      bottom: 24,
      left: 0,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    }),
  });
  const onMessage = new MockChromeEvent<
    [unknown, { id?: string }, (response: unknown) => void]
  >();
  onMessage.addListener.mockImplementation((registered) => {
    listener = registered as ContentListener;
  });
  sendMessage = vi.fn();
  vi.stubGlobal("chrome", {
    runtime: {
      id: "extension-id",
      onMessage,
      sendMessage,
    },
  });
  await import("./content");
  const probe = dispatch({ type: "termkey.contentScriptProbe" });
  documentToken = (probe.response as { documentToken: string }).documentToken;
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
});

function dispatch(message: unknown, sender = { id: "extension-id" }) {
  if (
    typeof message === "object" &&
    message !== null &&
    "type" in message &&
    ((message as { type?: string }).type === "termkey-fill-credentials" ||
      (message as { type?: string }).type === "termkey.fillGeneratedPassword" ||
      (message as { type?: string }).type === "termkey.captureVisibleCredentials" ||
      (message as { type?: string }).type === "termkey.captureSubmittedLogin") &&
    !("documentToken" in message)
  ) {
    message = { ...message, documentToken };
  }
  let response: unknown;
  const handled = listener(message, sender, (value) => {
    response = value;
  });
  return { handled, response };
}

function input(selector: string, root: ParentNode = document) {
  const element = root.querySelector<HTMLInputElement>(selector);
  if (!element) {
    throw new Error(`Missing fixture input: ${selector}`);
  }
  return element;
}

function fillCredentials(username: string | null, password: string) {
  return new Promise<unknown>((resolve) => {
    listener(
      {
        type: "termkey-fill-credentials",
        documentToken,
        username: username ?? undefined,
        password,
      },
      { id: "extension-id" },
      resolve
    );
  });
}

it("selects the current password in a login form", async () => {
  document.body.innerHTML = `
    <form id="login">
      <input id="login-username" autocomplete="username">
      <input id="login-current" type="password" autocomplete="current-password">
      <input id="login-otp" type="password" autocomplete="one-time-code">
    </form>
  `;

  await expect(
    fillCredentials("person@example.test", "login-secret")
  ).resolves.toMatchObject({
    ok: true,
    filledFields: 2,
    filledUsername: true,
    filledPassword: true,
  });
  expect(input("#login-username").value).toBe("person@example.test");
  expect(input("#login-current").value).toBe("login-secret");
  expect(input("#login-otp").value).toBe("");
});

it("selects both new-password fields in a signup form", () => {
  document.body.innerHTML = `
    <form id="signup" aria-label="Create account">
      <input id="signup-username" autocomplete="username" value="new@example.test">
      <input id="signup-primary" type="password" autocomplete="new-password" aria-label="Create password">
      <input id="signup-confirmation" type="password" autocomplete="new-password" aria-label="Confirm password">
    </form>
    <form id="login">
      <input id="existing-password" type="password" autocomplete="current-password">
    </form>
  `;

  const result = dispatch({
    type: "termkey.fillGeneratedPassword",
    password: "generated-secret",
  });

  expect(result.response).toEqual({
    ok: true,
    username: "new@example.test",
    filledPasswordFields: 2,
  });
  expect(input("#signup-primary").value).toBe("generated-secret");
  expect(input("#signup-confirmation").value).toBe("generated-secret");
  expect(input("#existing-password").value).toBe("");
});

it("does not fill an ambiguous second password as confirmation", () => {
  document.body.innerHTML = `
    <form id="signup" aria-label="Create account">
      <input id="signup-primary" type="password" autocomplete="new-password">
      <input id="ambiguous-password" type="password" value="leave-unchanged">
    </form>
  `;

  const result = dispatch({
    type: "termkey.fillGeneratedPassword",
    password: "generated-secret",
  });

  expect(result.response).toMatchObject({
    ok: true,
    filledPasswordFields: 1,
  });
  expect(input("#signup-primary").value).toBe("generated-secret");
  expect(input("#ambiguous-password").value).toBe("leave-unchanged");
});

it("does not authorize generated fill from focus alone", () => {
  document.body.innerHTML = `
    <form id="generic-form">
      <h2>User account</h2>
      <input id="generic-password" type="password" value="login-secret">
    </form>
  `;
  input("#generic-password").focus();

  const result = dispatch({
    type: "termkey.fillGeneratedPassword",
    password: "generated-secret",
  });

  expect(result.response).toEqual({
    ok: false,
    error:
      "No visible signup password field was found on this page. Open the account creation form first.",
  });
  expect(input("#generic-password").value).toBe("login-secret");
});

it("does not authorize generated fill from a reset link in a sign-in form", () => {
  document.body.innerHTML = `
    <form id="sign-in-form">
      <h2>Sign in</h2>
      <input id="sign-in-password" type="password" value="login-secret">
      <a href="/reset-password">Forgot password? Reset password</a>
    </form>
  `;
  input("#sign-in-password").focus();

  const result = dispatch({
    type: "termkey.fillGeneratedPassword",
    password: "generated-secret",
  });

  expect(result.response).toEqual({
    ok: false,
    error:
      "No visible signup password field was found on this page. Open the account creation form first.",
  });
  expect(input("#sign-in-password").value).toBe("login-secret");
});

it.each([
  ["change-password link", '<a href="/change-password">Change password</a>'],
  [
    "update-password navigation",
    '<nav><a href="/update-password">Update password</a></nav>',
  ],
  ["create-account help copy", "<p>Need help? Create account</p>"],
])("does not trust %s as generated-password intent", (_caseName, copy) => {
  document.body.innerHTML = `
    <form id="generic-login">
      <h2>Sign in</h2>
      <input id="generic-login-password" type="password" value="login-secret">
      ${copy}
    </form>
  `;
  input("#generic-login-password").focus();

  const result = dispatch({
    type: "termkey.fillGeneratedPassword",
    password: "generated-secret",
  });

  expect(result.response).toMatchObject({ ok: false });
  expect(input("#generic-login-password").value).toBe("login-secret");
});

it("allows an unambiguous reset form to receive a generated password", () => {
  document.body.innerHTML = `
    <form id="reset-password-form" aria-label="Reset password">
      <h2>Reset password</h2>
      <input id="replacement-password" type="password">
      <button type="submit">Update password</button>
    </form>
  `;

  const result = dispatch({
    type: "termkey.fillGeneratedPassword",
    password: "generated-secret",
  });

  expect(result.response).toMatchObject({
    ok: true,
    filledPasswordFields: 1,
  });
  expect(input("#replacement-password").value).toBe("generated-secret");
});

it("rejects mixed sign-in and reset structural intent for a generic field", () => {
  document.body.innerHTML = `
    <form id="mixed-intent-form" aria-label="Sign in">
      <h2>Sign in</h2>
      <input id="mixed-generic-password" type="password" value="login-secret">
      <button type="submit">Reset password</button>
    </form>
  `;
  input("#mixed-generic-password").focus();

  expect(
    dispatch({
      type: "termkey.fillGeneratedPassword",
      password: "generated-secret",
    }).response
  ).toMatchObject({ ok: false });
  expect(input("#mixed-generic-password").value).toBe("login-secret");
});

it("allows a strong new-password field despite mixed structural intent", () => {
  document.body.innerHTML = `
    <form id="mixed-intent-form" aria-label="Sign in">
      <h2>Sign in</h2>
      <input id="mixed-new-password" type="password" autocomplete="new-password">
      <button type="submit">Reset password</button>
    </form>
  `;

  expect(
    dispatch({
      type: "termkey.fillGeneratedPassword",
      password: "generated-secret",
    }).response
  ).toMatchObject({ ok: true, filledPasswordFields: 1 });
  expect(input("#mixed-new-password").value).toBe("generated-secret");
});

it("does not capture the old password during password change", () => {
  document.body.innerHTML = `
    <form id="password-change" aria-label="Change password">
      <input id="old-password" type="password" autocomplete="current-password" value="old-secret">
      <input id="new-password" type="password" autocomplete="new-password" value="new-secret">
      <input id="confirm-password" type="password" autocomplete="new-password" aria-label="Confirm password" value="new-secret">
    </form>
  `;

  const result = dispatch({ type: "termkey.captureVisibleCredentials" });

  expect(result.response).toEqual({
    ok: true,
    captureState: "password_only",
    username: null,
    password: "new-secret",
  });
});

it("captures an unambiguous submitted login with its document token", () => {
  document.body.innerHTML = `
    <form>
      <input autocomplete="username" value="sam">
      <input type="password" autocomplete="current-password" value="secret">
    </form>
  `;
  const form = document.querySelector("form");
  if (!form) {
    throw new Error("Missing submitted login fixture.");
  }
  form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));

  expect(
    dispatch({ type: "termkey.captureSubmittedLogin", documentToken }).response
  ).toEqual({ ok: true, username: "sam", password: "secret" });
});

it.each([
  [
    "a signup form",
    `<form><input type="password" autocomplete="new-password" value="secret"></form>`,
  ],
  [
    "equally ranked passwords in the submitted form",
    `<form>
       <input type="password" autocomplete="current-password" value="one">
       <input type="password" autocomplete="current-password" value="two">
     </form>`,
  ],
  ["a username-only form", `<form><input autocomplete="username" value="sam"></form>`],
  [
    "a blank password",
    `<form><input autocomplete="username" value="sam"><input type="password" autocomplete="current-password" value=""></form>`,
  ],
])("rejects submitted capture from %s", (_caseName, fixture) => {
  document.body.innerHTML = fixture;
  const form = document.querySelector("form");
  if (form) {
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  }

  expect(
    dispatch({ type: "termkey.captureSubmittedLogin", documentToken }).response
  ).toMatchObject({ ok: false });
});

it("reports a visible invalid-login alert only on a login page", () => {
  document.body.innerHTML = `
    <form>
      <input autocomplete="username" value="sam">
      <input type="password" autocomplete="current-password" value="secret">
      <p role="alert">Invalid password</p>
    </form>
  `;

  expect(dispatch({ type: "termkey.inspectPageContext" }).response).toMatchObject({
    ok: true,
    intent: "login",
    hasVisibleLoginFailure: true,
  });
});

it("does not report an invalid-login alert hidden by an ancestor", () => {
  document.body.innerHTML = `
    <form>
      <input autocomplete="username" value="sam">
      <input type="password" autocomplete="current-password" value="secret">
      <div hidden><p role="alert">Invalid password</p></div>
    </form>
  `;

  expect(dispatch({ type: "termkey.inspectPageContext" }).response).toMatchObject({
    ok: true,
    intent: "login",
    hasVisibleLoginFailure: false,
  });
});

it("does not report a visible failure alert on a signup page", () => {
  document.body.innerHTML = `
    <form aria-label="Create account">
      <input type="password" autocomplete="new-password" value="secret">
      <p class="error">Try again</p>
    </form>
  `;

  expect(dispatch({ type: "termkey.inspectPageContext" }).response).toMatchObject({
    ok: true,
    intent: "signup",
    hasVisibleLoginFailure: false,
  });
});

it("reports an empty eligible password field as fillable", () => {
  document.body.innerHTML = `
    <form><input autocomplete="username" value="sam"><input type="password" autocomplete="current-password"></form>
  `;

  expect(dispatch({ type: "termkey.inspectPageContext" }).response).toMatchObject({
    ok: true,
    intent: "login",
    hasEmptyLoginField: true,
  });
});

it("reports an empty eligible username field as fillable", () => {
  document.body.innerHTML = `
    <form><input autocomplete="username"><input type="password" autocomplete="current-password" value="secret"></form>
  `;

  expect(dispatch({ type: "termkey.inspectPageContext" }).response).toMatchObject({
    ok: true,
    intent: "login",
    hasEmptyLoginField: true,
  });
});

it("reports a filled eligible login form as not fillable", () => {
  document.body.innerHTML = `
    <form><input autocomplete="username" value="sam"><input type="password" autocomplete="current-password" value="secret"></form>
  `;

  expect(dispatch({ type: "termkey.inspectPageContext" }).response).toMatchObject({
    ok: true,
    intent: "login",
    hasEmptyLoginField: false,
  });
});

it("notifies the background of a submitted login without credentials", () => {
  document.body.innerHTML = `
    <form>
      <input autocomplete="username" value="sam">
      <input type="password" autocomplete="current-password" value="secret">
    </form>
  `;
  const form = document.querySelector("form");
  if (!form) {
    throw new Error("Missing submitted login fixture.");
  }

  form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));

  expect(sendMessage).toHaveBeenCalledWith({
    type: "termkey.content.loginSubmitted",
    documentToken,
  });
  expect(sendMessage.mock.calls[0][0]).not.toHaveProperty("password");
});

it("snapshots submitted credentials before a site handler clears the form", () => {
  document.body.innerHTML = `
    <form>
      <input autocomplete="username" value="sam">
      <input type="password" autocomplete="current-password" value="secret">
    </form>
  `;
  const form = document.querySelector("form");
  if (!form) {
    throw new Error("Missing submitted login fixture.");
  }
  form.addEventListener("submit", () => {
    form.remove();
  });

  form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));

  expect(document.querySelector("form")).toBeNull();
  expect(
    dispatch({ type: "termkey.captureSubmittedLogin", documentToken }).response
  ).toEqual({ ok: true, username: "sam", password: "secret" });
  expect(sendMessage).toHaveBeenCalledWith({
    type: "termkey.content.loginSubmitted",
    documentToken,
  });
  expect(sendMessage.mock.calls[0][0]).not.toHaveProperty("password");
});

it("captures a login when a non-submit button handles authentication", () => {
  document.body.innerHTML = `
    <section id="login-panel">
      <input autocomplete="username" value="sam">
      <input type="password" autocomplete="current-password" value="secret">
      <button type="button">Login</button>
    </section>
  `;
  const button = document.querySelector("button");
  if (!button) throw new Error("Missing login button fixture.");

  button.dispatchEvent(new MouseEvent("click", { bubbles: true }));

  expect(dispatch({ type: "termkey.captureSubmittedLogin", documentToken }).response)
    .toEqual({ ok: true, username: "sam", password: "secret" });
  expect(sendMessage).toHaveBeenCalledWith({
    type: "termkey.content.loginSubmitted",
    documentToken,
  });
});

it("emits a token-bound page-context event after a history API transition", async () => {
  history.pushState({}, "", "/signed-in");

  await vi.waitFor(() => {
    expect(sendMessage).toHaveBeenCalledWith({
      type: "termkey.content.pageContextChanged",
      documentToken,
    });
  });
});

it("does not treat OTP input as a password", async () => {
  document.body.innerHTML = `
    <form id="challenge">
      <input id="challenge-username" autocomplete="username" value="person@example.test">
      <input id="challenge-code" type="password" aria-label="Authentication code" value="123456">
    </form>
  `;

  await expect(fillCredentials(null, "login-secret")).resolves.toEqual({
    ok: false,
    error: "No visible password field was found on this page.",
  });
  expect(input("#challenge-code").value).toBe("123456");
  expect(dispatch({ type: "termkey.captureVisibleCredentials" }).response).toEqual({
    ok: true,
    captureState: "username_only",
    username: "person@example.test",
  });
});

it("keeps username and password within the same form", async () => {
  document.body.innerHTML = `
    <form id="login">
      <input id="login-username" name="login">
      <input id="login-password" type="password">
    </form>
    <form id="newsletter">
      <input id="decoy-username" type="email" autocomplete="username" aria-label="Email account">
    </form>
  `;

  await expect(
    fillCredentials("person@example.test", "login-secret")
  ).resolves.toMatchObject({ ok: true, filledFields: 2 });
  expect(input("#login-username").value).toBe("person@example.test");
  expect(input("#login-password").value).toBe("login-secret");
  expect(input("#decoy-username").value).toBe("");
});

it("does not pair explicit different form owners during fill", async () => {
  document.body.innerHTML = `
    <section id="shared-login-context">
      <form id="username-form"></form>
      <form id="password-form"></form>
      <input id="owned-username" form="username-form" autocomplete="username">
      <input id="owned-password" form="password-form" type="password" autocomplete="current-password">
    </section>
  `;

  expect(input("#owned-username").form?.id).toBe("username-form");
  expect(input("#owned-password").form?.id).toBe("password-form");
  await expect(
    fillCredentials("person@example.test", "login-secret")
  ).resolves.toEqual({
    ok: true,
    filledFields: 1,
    filledUsername: false,
    filledPassword: true,
  });
  expect(input("#owned-username").value).toBe("");
  expect(input("#owned-password").value).toBe("login-secret");
});

it("does not pair explicit different form owners during capture", () => {
  document.body.innerHTML = `
    <section id="shared-login-context">
      <form id="username-form"></form>
      <form id="password-form"></form>
      <input id="owned-username" form="username-form" autocomplete="username" value="person@example.test">
      <input id="owned-password" form="password-form" type="password" autocomplete="current-password" value="login-secret">
    </section>
  `;

  expect(dispatch({ type: "termkey.captureVisibleCredentials" }).response).toEqual({
    ok: true,
    captureState: "password_only",
    username: null,
    password: "login-secret",
  });
  expect(input("#owned-username").value).toBe("person@example.test");
  expect(input("#owned-password").value).toBe("login-secret");
});

it("ignores a higher-scoring hostile decoy in another form", async () => {
  document.body.innerHTML = `
    <form id="coherent-login" aria-label="Login">
      <input id="coherent-username" autocomplete="username">
      <input id="coherent-password" type="password" autocomplete="current-password">
    </form>
    <form id="hostile-decoy">
      <input id="hostile-password" type="password" autocomplete="current-password" aria-label="Password secret">
    </form>
  `;
  input("#hostile-password").focus();

  await expect(
    fillCredentials("person@example.test", "login-secret")
  ).resolves.toMatchObject({ ok: true, filledFields: 2 });
  expect(input("#coherent-username").value).toBe("person@example.test");
  expect(input("#coherent-password").value).toBe("login-secret");
  expect(input("#hostile-password").value).toBe("");
});

it("rejects equally ranked login groups instead of using DOM order", async () => {
  document.body.innerHTML = `
    <form id="login-one">
      <input id="username-one" autocomplete="username">
      <input id="password-one" type="password" autocomplete="current-password">
    </form>
    <form id="login-two">
      <input id="username-two" autocomplete="username">
      <input id="password-two" type="password" autocomplete="current-password">
    </form>
  `;

  await expect(
    fillCredentials("person@example.test", "login-secret")
  ).resolves.toEqual({
    ok: false,
    error: "No visible username or password field was found on this page.",
  });
  expect(input("#username-one").value).toBe("");
  expect(input("#password-one").value).toBe("");
  expect(input("#username-two").value).toBe("");
  expect(input("#password-two").value).toBe("");
});

it("rejects equally ranked generated groups instead of using DOM order", () => {
  document.body.innerHTML = `
    <form id="signup-one">
      <input id="new-one" type="password" autocomplete="new-password">
      <input id="confirm-one" type="password" autocomplete="new-password" aria-label="Confirm password">
    </form>
    <form id="signup-two">
      <input id="new-two" type="password" autocomplete="new-password">
      <input id="confirm-two" type="password" autocomplete="new-password" aria-label="Confirm password">
    </form>
  `;

  const result = dispatch({
    type: "termkey.fillGeneratedPassword",
    password: "generated-secret",
  });

  expect(result.response).toEqual({
    ok: false,
    error:
      "No visible signup password field was found on this page. Open the account creation form first.",
  });
  expect(input("#new-one").value).toBe("");
  expect(input("#confirm-one").value).toBe("");
  expect(input("#new-two").value).toBe("");
  expect(input("#confirm-two").value).toBe("");
});

it("fails closed when password-change capture has tied new groups", () => {
  document.body.innerHTML = `
    <form id="password-change">
      <input id="old-password" type="password" autocomplete="current-password" value="old-secret">
      <input id="change-new-one" type="password" aria-label="New password" value="new-one-secret">
      <input id="change-new-two" type="password" aria-label="New password" value="new-two-secret">
    </form>
  `;

  expect(dispatch({ type: "termkey.captureVisibleCredentials" }).response).toEqual({
    ok: false,
    error: "No visible password field was found on this page.",
  });
  expect(input("#old-password").value).toBe("old-secret");
  expect(input("#change-new-one").value).toBe("new-one-secret");
  expect(input("#change-new-two").value).toBe("new-two-secret");
});

it("finds fields inside an open shadow root", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const shadowRoot = host.attachShadow({ mode: "open" });
  shadowRoot.innerHTML = `
    <form id="shadow-login">
      <input id="shadow-username" autocomplete="username">
      <input id="shadow-password" type="password" autocomplete="current-password">
    </form>
  `;

  await expect(
    fillCredentials("shadow@example.test", "shadow-secret")
  ).resolves.toMatchObject({ ok: true, filledFields: 2 });
  expect(input("#shadow-username", shadowRoot).value).toBe(
    "shadow@example.test"
  );
  expect(input("#shadow-password", shadowRoot).value).toBe("shadow-secret");
});

it("dispatches bubbling input and change events on selected fields", async () => {
  document.body.innerHTML = `
    <form id="event-login">
      <input id="event-username" autocomplete="username">
      <input id="event-password" type="password" autocomplete="current-password">
    </form>
  `;
  const events: string[] = [];
  const eventForm = document.querySelector("#event-login");
  if (!eventForm) {
    throw new Error("Missing event fixture form.");
  }
  eventForm.addEventListener("input", (event) => {
    events.push(`${(event.target as HTMLInputElement).id}:input`);
  });
  eventForm.addEventListener("change", (event) => {
    events.push(`${(event.target as HTMLInputElement).id}:change`);
  });

  await expect(
    fillCredentials("person@example.test", "login-secret")
  ).resolves.toMatchObject({ ok: true, filledFields: 2 });
  expect(events).toEqual([
    "event-username:input",
    "event-username:change",
    "event-password:input",
    "event-password:change",
  ]);
});

it("uses only the bounded retry schedule for a delayed password field", async () => {
  vi.useFakeTimers();
  const setTimeoutSpy = vi.spyOn(window, "setTimeout");
  const resultPromise = fillCredentials(null, "delayed-secret");

  await vi.advanceTimersByTimeAsync(149);
  expect(document.querySelector("#delayed-password")).toBeNull();
  await vi.advanceTimersByTimeAsync(1);
  await vi.advanceTimersByTimeAsync(349);
  expect(document.querySelector("#delayed-password")).toBeNull();
  await vi.advanceTimersByTimeAsync(1);

  document.body.innerHTML = `
    <form id="delayed-login">
      <input id="delayed-password" type="password" autocomplete="current-password">
    </form>
  `;
  const events: string[] = [];
  input("#delayed-password").addEventListener("input", () => {
    events.push("input");
  });
  input("#delayed-password").addEventListener("change", () => {
    events.push("change");
  });

  await vi.advanceTimersByTimeAsync(699);
  expect(input("#delayed-password").value).toBe("");
  await vi.advanceTimersByTimeAsync(1);
  await expect(resultPromise).resolves.toEqual({
    ok: true,
    filledFields: 1,
    filledUsername: false,
    filledPassword: true,
  });
  expect(input("#delayed-password").value).toBe("delayed-secret");
  expect(events).toEqual(["input", "change"]);
  expect(
    setTimeoutSpy.mock.calls
      .map((call) => call[1])
      .filter((delay) => typeof delay === "number" && delay > 0)
  ).toEqual([150, 350, 700]);
  const timerCallsAfterFill = setTimeoutSpy.mock.calls.length;

  await vi.advanceTimersByTimeAsync(10_000);
  expect(events).toEqual(["input", "change"]);
  expect(setTimeoutSpy).toHaveBeenCalledTimes(timerCallsAfterFill);
});

it("does not choose OTP or confirmation fields as login passwords", async () => {
  document.body.innerHTML = `
    <form>
      <input id="username" autocomplete="username" value="person@example.test">
      <input id="login-password" type="password" autocomplete="current-password">
      <input id="otp" type="password" autocomplete="one-time-code" aria-label="OTP code">
      <input id="new-password" type="password" autocomplete="new-password">
      <input id="confirm-password" type="password" autocomplete="new-password" aria-label="Confirm password">
    </form>
  `;

  const responsePromise = fillCredentials("person@example.test", "secret");
  await expect(responsePromise).resolves.toMatchObject({
    ok: true,
    filledPassword: true,
  });
  expect(
    (document.querySelector("#login-password") as HTMLInputElement).value
  ).toBe("secret");
  expect((document.querySelector("#otp") as HTMLInputElement).value).toBe("");
  expect(
    (document.querySelector("#confirm-password") as HTMLInputElement).value
  ).toBe("");

  document.body.innerHTML = `
    <form>
      <input type="password" autocomplete="one-time-code" aria-label="OTP code" value="123456">
      <input type="password" autocomplete="new-password" aria-label="Confirm password" value="not-a-login">
    </form>
  `;
  const capture = dispatch({
    type: "termkey.captureVisibleCredentials",
  });
  expect(capture.response).toMatchObject({
    ok: false,
    error: "No visible password field was found on this page.",
  });
});

it("returns a stable cryptographic document token from probe and context", () => {
  const probe = dispatch({ type: "termkey.contentScriptProbe" });
  const context = dispatch({ type: "termkey.inspectPageContext" });

  expect(probe.response).toMatchObject({
    ok: true,
    documentToken: expect.stringMatching(/^[a-f0-9]{64}$/),
  });
  expect(context.response).toMatchObject({
    ok: true,
    documentToken: (probe.response as { documentToken: string }).documentToken,
  });
});

it("rejects a mismatched final-delivery token before touching login fields", async () => {
  document.body.innerHTML = `
    <form>
      <input id="username" autocomplete="username">
      <input id="password" type="password" autocomplete="current-password">
    </form>
  `;

  const response = await new Promise<unknown>((resolve) => {
    listener(
      {
        type: "termkey-fill-credentials",
        documentToken: "0".repeat(64),
        username: "attacker-controlled@example.test",
        password: "must-not-be-filled",
      },
      { id: "extension-id" },
      resolve
    );
  });

  expect(response).toEqual({
    ok: false,
    error: "The page document changed before delivery.",
  });
  expect(input("#username").value).toBe("");
  expect(input("#password").value).toBe("");
});

it.each([
  [
    {
      type: "termkey.fillGeneratedPassword",
      documentToken: "0".repeat(64),
      password: "must-not-be-filled",
    },
  ],
  [
    {
      type: "termkey.captureVisibleCredentials",
      documentToken: "0".repeat(64),
    },
  ],
  [
    {
      type: "termkey.captureSubmittedLogin",
      documentToken: "0".repeat(64),
    },
  ],
])("rejects mismatched tokens for every other secret-bearing message", (message) => {
  document.body.innerHTML = `
    <form>
      <input id="username" autocomplete="username" value="person@example.test">
      <input id="password" type="password" autocomplete="new-password" value="page-secret">
    </form>
  `;
  let response: unknown;
  listener(message, { id: "extension-id" }, (value) => {
    response = value;
  });

  expect(response).toEqual({
    ok: false,
    error: "The page document changed before delivery.",
  });
  expect(input("#password").value).toBe("page-secret");
});

it("rejects content messages from other extension senders", () => {
  const result = dispatch(
    { type: "termkey.contentScriptProbe" },
    { id: "different-extension" }
  );

  expect(result.handled).toBe(false);
  expect(result.response).toBeUndefined();
});
