import { beforeEach, describe, expect, expectTypeOf, it, vi } from "vitest";
import type {
  PendingLoginPromptMetadata,
  PopupPendingLoginResponse,
} from "@termkey/types";

import {
  NATIVE_REQUEST_TIMEOUT_MS,
  NativeHostClient,
  createBackgroundService,
} from "./background";
import {
  MockNativePort,
  createChromeMock,
} from "./test/chrome-mock";

const protocolInfo = {
  protocolVersion: 3,
  capabilities: [
    "opaque-match-handles",
    "document-token-binding",
    "origin-only-save",
    "password-entry-update",
    "bounded-native-output",
  ],
} as const;

const siteMatchesResponse = {
  type: "site_matches",
  siteUrl: "https://example.test",
  siteOrigin: "https://example.test",
  siteHostname: "example.test",
  matches: [
    {
      id: "entry-1",
      name: "Example",
      username: "person@example.test",
      url: "https://example.test",
      matchType: "exact_origin",
      hasSecondaryPassword: false,
    },
  ],
} as const;

const statusResponse = {
  type: "status",
  app: "termkey",
  version: "1.0.0",
  vaultPath: "/vault",
  vaultExists: true,
  firstRunComplete: true,
  recoveryConfigured: true,
  locked: false,
  ...protocolInfo,
} as const;

function postedRequestId(port: MockNativePort, callIndex = -1) {
  const calls = port.postMessage.mock.calls;
  const index = callIndex < 0 ? calls.length + callIndex : callIndex;
  return (calls[index][0] as { requestId: string }).requestId;
}

function correlatedResponse(
  port: MockNativePort,
  response: Record<string, unknown>,
  callIndex = -1
) {
  return {
    ...response,
    requestId: postedRequestId(port, callIndex),
  };
}

function installHappyPathResponders(mock: ReturnType<typeof createChromeMock>) {
  let documentToken = "a".repeat(64);
  mock.setTabMessageHandler((_tabId, message, options) => {
    expect(options).toEqual({ frameId: 0 });
    const type = (message as { type?: string }).type;
    if (type === "termkey.contentScriptProbe") {
      return { ok: true, documentToken };
    }
    if (type === "termkey-fill-credentials") {
      return {
        ok: true,
        filledFields: 2,
        filledUsername: true,
        filledPassword: true,
      };
    }
    return { ok: true, documentToken };
  });
  mock.setNativeResponder((request) => {
    const type = (request as { type?: string }).type;
    if (type === "find_site_matches") {
      return siteMatchesResponse;
    }
    if (type === "get_autofill_entry") {
      return {
        type: "autofill_entry",
        entry: {
          id: "entry-1",
          name: "Example",
          username: "person@example.test",
          password: "secret",
          url: "https://example.test",
        },
      };
    }
    return { type: "error", message: "Unexpected request." };
  });

  return {
    setDocumentToken(token: string) {
      documentToken = token;
    },
  };
}

function grantIdFromDiscovery(result: unknown) {
  return (
    result as {
      ok: true;
      response: { matches: Array<{ grantId: string }> };
    }
  ).response.matches[0].grantId;
}

describe("background security boundaries", () => {
  beforeEach(() => {
    vi.useRealTimers();
  });

  it("forwards empty eligible login-field metadata from the content script", async () => {
    const mock = createChromeMock();
    mock.setTabMessageHandler((_tabId, message) => {
      if ((message as { type?: string }).type === "termkey.contentScriptProbe") {
        return { ok: true, documentToken: "a".repeat(64) };
      }
      if ((message as { type?: string }).type === "termkey.inspectPageContext") {
        return {
          ok: true,
          documentToken: "a".repeat(64),
          intent: "login",
          visibleUsername: null,
          hasPasswordField: true,
          hasEmptyLoginField: true,
          hasConfirmationPasswordField: false,
          canGeneratePassword: false,
        };
      }
      throw new Error("Unexpected content-script message.");
    });
    const service = createBackgroundService(mock.chrome);

    const response = await service.handleMessage(
      { type: "termkey.content.inspectPageContext" },
      mock.extensionSender
    );

    expect(response).toMatchObject({
      ok: true,
      response: {
        type: "page_context",
        context: { hasEmptyLoginField: true },
      },
    });
  });

  it("returns no pending login when the extension has not captured one", async () => {
    expectTypeOf<PopupPendingLoginResponse["candidate"]>().toEqualTypeOf<
      | {
          username: string | null;
          url: string;
          mode: "save" | "update" | "unlock";
          requiresSecondaryPassword?: boolean;
          existingEntryName?: string;
        }
      | null
    >();

    const mock = createChromeMock();
    const service = createBackgroundService(mock.chrome);

    expect(
      await service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).toEqual({
      ok: true,
      response: { type: "pending_login", candidate: null },
    });
  });

  it("keeps in-page prompt metadata free of submitted passwords", () => {
    expectTypeOf<PendingLoginPromptMetadata>().toMatchTypeOf<{
      candidateId: string;
      origin: string;
      hostname: string;
      username: string | null;
      defaultName: string;
      mode: "save" | "update" | "unlock" | "protected-update" | "resolve";
      isHttp: boolean;
    }>();
    expectTypeOf<PendingLoginPromptMetadata>().not.toHaveProperty("password");
    expectTypeOf<PendingLoginPromptMetadata>().not.toHaveProperty(
      "masterPassword"
    );
    expectTypeOf<PendingLoginPromptMetadata>().not.toHaveProperty(
      "secondaryPassword"
    );
  });

  it("mounts one opaque prompt after a successful page transition", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "sam@example.test",
      password: "website-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    createBackgroundService(mock.chrome);

    await mock.dispatchContentMessage(
      { type: "termkey.content.loginSubmitted", documentToken: "a".repeat(64) },
      7
    );
    await mock.dispatchContentMessage(
      { type: "termkey.content.pageContextChanged", documentToken: "a".repeat(64) },
      7
    );

    const mountCalls = mock.chrome.tabs.sendMessage.mock.calls.filter(
      ([, message]) =>
        (message as { type?: string }).type ===
        "termkey.pendingLoginPrompt.mount"
    );
    expect(mountCalls).toHaveLength(1);
    expect(mock.chrome.tabs.sendMessage).toHaveBeenCalledWith(
      7,
      {
        type: "termkey.pendingLoginPrompt.mount",
        candidateId: expect.stringMatching(/^[a-f0-9]{64}$/),
        documentToken: "a".repeat(64),
      },
      { frameId: 0 }
    );
    expect(JSON.stringify(mountCalls)).not.toContain("website-secret");
    expect(JSON.stringify(mountCalls)).not.toContain("sam@example.test");
  });

  it("uses the destination document token when an immediate same-origin reload succeeds", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "sam@example.test",
      password: "website-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    let resolveTabGet!: (tab: { id: number; url: string }) => void;
    mock.chrome.tabs.get.mockImplementationOnce(
      () => new Promise((resolve) => { resolveTabGet = resolve; })
    );
    createBackgroundService(mock.chrome);

    const submitted = mock.dispatchContentMessage(
      { type: "termkey.content.loginSubmitted", documentToken: "a".repeat(64) },
      7
    );
    mock.setTab({ id: 7, url: "https://example.test/account" });
    mock.setDocumentToken("b".repeat(64));
    resolveTabGet({ id: 7, url: "https://example.test/account" });
    await submitted;

    expect(mock.chrome.tabs.sendMessage).toHaveBeenCalledWith(
      7,
      {
        type: "termkey.pendingLoginPrompt.mount",
        candidateId: expect.stringMatching(/^[a-f0-9]{64}$/),
        documentToken: "b".repeat(64),
      },
      { frameId: 0 }
    );
  });

  it("removes a mounted prompt when a later inspection finds an invalid login", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({ ok: true, username: "sam", password: "website-secret" });
    mock.setPageContext({ intent: "unknown", hasPasswordField: false, hasVisibleLoginFailure: false });
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      { type: "termkey.content.loginSubmitted", documentToken: "a".repeat(64) }, 7
    );
    await mock.dispatchContentMessage(
      { type: "termkey.content.pageContextChanged", documentToken: "a".repeat(64) }, 7
    );
    const mount = mock.chrome.tabs.sendMessage.mock.calls.find(
      ([, message]) => (message as { type?: string }).type === "termkey.pendingLoginPrompt.mount"
    );
    const candidateId = (mount?.[1] as { candidateId: string }).candidateId;

    mock.setPageContext({ intent: "login", hasPasswordField: true, hasVisibleLoginFailure: true });
    await service.handleTabUpdated(7, { status: "complete" });

    expect(mock.chrome.tabs.sendMessage).toHaveBeenCalledWith(
      7,
      { type: "termkey.pendingLoginPrompt.remove", candidateId },
      { frameId: 0 }
    );
  });

  it("does not mount a prompt while the submitted login form is unchanged", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({ ok: true, username: "sam", password: "website-secret" });
    mock.setPageContext({ intent: "login", hasPasswordField: true, hasVisibleLoginFailure: false });
    createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      { type: "termkey.content.loginSubmitted", documentToken: "a".repeat(64) }, 7
    );
    await mock.dispatchTabUpdated(7, { status: "complete" });

    expect(mock.chrome.tabs.sendMessage).not.toHaveBeenCalledWith(
      7,
      expect.objectContaining({ type: "termkey.pendingLoginPrompt.mount" }),
      { frameId: 0 }
    );
  });

  it.each([
    ["replacement", async (mock: ReturnType<typeof createChromeMock>) => {
      mock.setSubmittedLogin({ ok: true, username: "replacement", password: "replacement-secret" });
      await mock.dispatchContentMessage(
        { type: "termkey.content.loginSubmitted", documentToken: "a".repeat(64) }, 7
      );
    }],
    ["expiry", async (_mock: ReturnType<typeof createChromeMock>) => {
      await vi.advanceTimersByTimeAsync(120_000);
    }],
    ["tab close", async (mock: ReturnType<typeof createChromeMock>) => {
      await mock.dispatchTabRemoved(7);
    }],
    ["cross-origin navigation", async (mock: ReturnType<typeof createChromeMock>) => {
      await mock.dispatchTabUpdated(7, { url: "https://elsewhere.test/account" });
    }],
  ])("removes the matching mounted prompt on %s", async (_caseName, removeCandidate) => {
    vi.useFakeTimers();
    const mock = createChromeMock();
    mock.setSubmittedLogin({ ok: true, username: "sam", password: "website-secret" });
    mock.setPageContext({ intent: "unknown", hasPasswordField: false, hasVisibleLoginFailure: false });
    createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      { type: "termkey.content.loginSubmitted", documentToken: "a".repeat(64) }, 7
    );
    await mock.dispatchContentMessage(
      { type: "termkey.content.pageContextChanged", documentToken: "a".repeat(64) }, 7
    );
    const mount = mock.chrome.tabs.sendMessage.mock.calls.find(
      ([, message]) => (message as { type?: string }).type === "termkey.pendingLoginPrompt.mount"
    );
    const candidateId = (mount?.[1] as { candidateId: string }).candidateId;

    await removeCandidate(mock);

    expect(mock.chrome.tabs.sendMessage).toHaveBeenCalledWith(
      7,
      { type: "termkey.pendingLoginPrompt.remove", candidateId },
      { frameId: 0 }
    );
  });

  it("keeps directly submitted credentials when the login page reloads immediately", async () => {
    const mock = createChromeMock();
    mock.setNativeResponder((request) =>
      (request as { type?: string }).type === "status"
        ? statusResponse
        : (request as { type?: string }).type === "find_site_matches"
        ? { ...siteMatchesResponse, matches: [] }
        : { type: "error", message: "Unexpected request." }
    );
    const service = createBackgroundService(mock.chrome);

    await expect(
      mock.dispatchContentMessage(
        {
          type: "termkey.content.loginSubmitted",
          documentToken: "a".repeat(64),
          username: "sam",
          password: "submitted-secret",
        },
        7
      )
    ).resolves.toEqual({ ok: true });
    expect(mock.chrome.tabs.sendMessage).not.toHaveBeenCalledWith(
      7,
      expect.objectContaining({ type: "termkey.captureSubmittedLogin" }),
      { frameId: 0 }
    );
    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toEqual({
      ok: true,
      response: { type: "pending_login", candidate: null },
    });

    mock.setTab({ id: 7, url: "https://example.test/account" });
    mock.setDocumentToken("b".repeat(64));
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    await mock.dispatchTabUpdated(7, { status: "complete" });

    const result = await service.handleMessage(
      { type: "termkey.pendingLogin.get" },
      mock.extensionSender
    );
    expect(result).toEqual({
      ok: true,
      response: {
        type: "pending_login",
        candidate: {
          username: "sam",
          url: "https://example.test",
          mode: "save",
        },
      },
    });
    expect(JSON.stringify(result)).not.toContain("submitted-secret");
    expect(mock.chrome.storage.session.set).not.toHaveBeenCalled();
  });

  it("checks a direct submission after navigation already completed", async () => {
    const mock = createChromeMock();
    mock.setNativeResponder((request) =>
      (request as { type?: string }).type === "status"
        ? statusResponse
        : (request as { type?: string }).type === "find_site_matches"
        ? { ...siteMatchesResponse, matches: [] }
        : { type: "error", message: "Unexpected request." }
    );
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    let resolveTabGet!: (tab: { id: number; url: string }) => void;
    mock.chrome.tabs.get.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveTabGet = resolve;
        })
    );
    const service = createBackgroundService(mock.chrome);

    const submitted = mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
        username: "sam",
        password: "submitted-secret",
      },
      7
    );
    mock.setTab({ id: 7, url: "https://example.test/account" });
    mock.setDocumentToken("b".repeat(64));
    resolveTabGet({ id: 7, url: "https://example.test/account" });
    await expect(submitted).resolves.toEqual({ ok: true });

    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toEqual({
      ok: true,
      response: {
        type: "pending_login",
        candidate: {
          username: "sam",
          url: "https://example.test",
          mode: "save",
        },
      },
    });
  });

  it("offers a submitted login after a successful same-origin SPA transition", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "sam",
      password: "submitted-secret",
    });
    mock.setNativeResponder((request) =>
      (request as { type?: string }).type === "status"
        ? statusResponse
        : (request as { type?: string }).type === "find_site_matches"
        ? { ...siteMatchesResponse, matches: [] }
        : { type: "error", message: "Unexpected request." }
    );
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    const service = createBackgroundService(mock.chrome);

    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.pageContextChanged",
        documentToken: "a".repeat(64),
      },
      7
    );
    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toMatchObject({
      ok: true,
      response: {
        candidate: { username: "sam", mode: "save" },
      },
    });
  });

  it("does not let a new same-origin document ready an older document's candidate", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "sam",
      password: "submitted-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    mock.setNativeResponder((request) =>
      (request as { type?: string }).type === "status"
        ? statusResponse
        : (request as { type?: string }).type === "find_site_matches"
        ? { ...siteMatchesResponse, matches: [] }
        : { type: "error", message: "Unexpected request." }
    );
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );

    mock.setDocumentToken("b".repeat(64));
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.pageContextChanged",
        documentToken: "b".repeat(64),
      },
      7
    );

    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toEqual({
      ok: true,
      response: { type: "pending_login", candidate: null },
    });
    expect(mock.chrome.runtime.connectNative).not.toHaveBeenCalled();
  });

  it("discards a submitted login when the destination shows a visible failure", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "sam",
      password: "submitted-secret",
    });
    mock.setPageContext({
      intent: "login",
      hasPasswordField: true,
      hasVisibleLoginFailure: true,
    });
    const service = createBackgroundService(mock.chrome);

    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );
    await mock.dispatchTabUpdated(7, { status: "complete" });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.pageContextChanged",
        documentToken: "a".repeat(64),
      },
      7
    );

    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toEqual({
      ok: true,
      response: { type: "pending_login", candidate: null },
    });
    expect(mock.chrome.runtime.connectNative).not.toHaveBeenCalled();
  });

  it("retains a submitted login while the login page remains unchanged", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "sam",
      password: "submitted-secret",
    });
    mock.setPageContext({
      intent: "login",
      hasPasswordField: true,
      hasVisibleLoginFailure: false,
    });
    mock.setNativeResponder((request) =>
      (request as { type?: string }).type === "status"
        ? statusResponse
        : (request as { type?: string }).type === "find_site_matches"
        ? { ...siteMatchesResponse, matches: [] }
        : { type: "error", message: "Unexpected request." }
    );
    const service = createBackgroundService(mock.chrome);

    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );
    await mock.dispatchTabUpdated(7, { status: "complete" });
    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toMatchObject({ response: { candidate: null } });

    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.pageContextChanged",
        documentToken: "a".repeat(64),
      },
      7
    );

    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toMatchObject({
      ok: true,
      response: { candidate: { username: "sam" } },
    });
  });

  it("expires a pending login after two minutes", async () => {
    let now = 1_000;
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "sam",
      password: "submitted-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    const service = createBackgroundService(mock.chrome, { now: () => now });

    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.pageContextChanged",
        documentToken: "a".repeat(64),
      },
      7
    );
    now += 120_000;

    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toEqual({
      ok: true,
      response: { type: "pending_login", candidate: null },
    });
    expect(mock.chrome.runtime.connectNative).not.toHaveBeenCalled();
  });

  it("proactively expires candidates and keeps replacement deadlines identity-safe", async () => {
    vi.useFakeTimers();
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "first-user",
      password: "first-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    mock.setNativeResponder((request) =>
      (request as { type?: string }).type === "status"
        ? statusResponse
        : (request as { type?: string }).type === "find_site_matches"
        ? { ...siteMatchesResponse, matches: [] }
        : { type: "error", message: "Unexpected request." }
    );
    const service = createBackgroundService(mock.chrome, { now: () => 1_000 });
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );

    await vi.advanceTimersByTimeAsync(60_000);
    mock.setSubmittedLogin({
      ok: true,
      username: "replacement-user",
      password: "replacement-secret",
    });
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.pageContextChanged",
        documentToken: "a".repeat(64),
      },
      7
    );

    await vi.advanceTimersByTimeAsync(60_000);
    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toMatchObject({
      ok: true,
      response: { candidate: { username: "replacement-user" } },
    });

    await vi.advanceTimersByTimeAsync(60_000);
    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toEqual({
      ok: true,
      response: { type: "pending_login", candidate: null },
    });
  });

  it.each([
    ["tab close", async (mock: ReturnType<typeof createChromeMock>) => {
      await mock.dispatchTabRemoved(7);
    }],
    ["cross-origin navigation", async (mock: ReturnType<typeof createChromeMock>) => {
      await mock.dispatchTabUpdated(7, {
        status: "complete",
        url: "https://elsewhere.test/account",
      });
    }],
  ])("clears a pending login on %s", async (_caseName, clearCandidate) => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "sam",
      password: "submitted-secret",
    });
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );

    await clearCandidate(mock);

    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toEqual({
      ok: true,
      response: { type: "pending_login", candidate: null },
    });
  });

  it("clears a pending login from a transient cross-origin URL update", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "sam",
      password: "submitted-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    mock.setNativeResponder((request) =>
      (request as { type?: string }).type === "find_site_matches"
        ? { ...siteMatchesResponse, matches: [] }
        : { type: "error", message: "Unexpected request." }
    );
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );

    await mock.chrome.tabs.onUpdated.emitAsync(
      7,
      { url: "https://elsewhere.test/redirect" },
      { id: 7, url: "https://elsewhere.test/redirect" }
    );
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.pageContextChanged",
        documentToken: "a".repeat(64),
      },
      7
    );

    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toEqual({
      ok: true,
      response: { type: "pending_login", candidate: null },
    });
    expect(mock.chrome.runtime.connectNative).not.toHaveBeenCalled();
  });

  it("classifies a pending login as update only for an identical non-null username", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "person@example.test",
      password: "submitted-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    mock.setNativeResponder((request) =>
      (request as { type?: string }).type === "status"
        ? statusResponse
        : (request as { type?: string }).type === "find_site_matches"
        ? siteMatchesResponse
        : { type: "error", message: "Unexpected request." }
    );
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.pageContextChanged",
        documentToken: "a".repeat(64),
      },
      7
    );

    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toMatchObject({
      ok: true,
      response: { candidate: { mode: "update" } },
    });
  });

  it("does not expose stale metadata when the candidate changes during match lookup", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "first-user",
      password: "first-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    mock.setNativeResponder((request) =>
      (request as { type?: string }).type === "status" ? statusResponse : undefined
    );
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.pageContextChanged",
        documentToken: "a".repeat(64),
      },
      7
    );

    const pendingGet = service.handleMessage(
      { type: "termkey.pendingLogin.get" },
      mock.extensionSender
    );
    await vi.waitFor(() => {
      expect(mock.ports[0]?.postMessage).toHaveBeenCalledTimes(3);
    });
    mock.setSubmittedLogin({
      ok: true,
      username: "replacement-user",
      password: "replacement-secret",
    });
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );
    mock.ports[0].onMessage.emit(
      correlatedResponse(mock.ports[0], siteMatchesResponse)
    );

    await expect(pendingGet).resolves.toEqual({
      ok: true,
      response: { type: "pending_login", candidate: null },
    });
  });

  it("does not classify two missing usernames as an update", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: null,
      password: "submitted-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    mock.setNativeResponder((request) =>
      (request as { type?: string }).type === "status"
        ? statusResponse
        : (request as { type?: string }).type === "find_site_matches"
        ? {
            ...siteMatchesResponse,
            matches: [{ ...siteMatchesResponse.matches[0], username: null }],
          }
        : { type: "error", message: "Unexpected request." }
    );
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.pageContextChanged",
        documentToken: "a".repeat(64),
      },
      7
    );

    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toMatchObject({
      ok: true,
      response: {
        candidate: {
          username: null,
          mode: "save",
        },
      },
    });
  });

  it("dismisses a ready pending login", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "sam",
      password: "submitted-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.pageContextChanged",
        documentToken: "a".repeat(64),
      },
      7
    );

    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.dismiss" },
        mock.extensionSender
      )
    ).resolves.toEqual({
      ok: true,
      response: { type: "pending_login", candidate: null },
    });
    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toMatchObject({ response: { candidate: null } });
  });

  it("saves with the background-only password and clears after native success", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "captured-user",
      password: "background-only-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    let nativeSave: unknown;
    mock.setNativeResponder((request) => {
      if ((request as { type?: string }).type === "save_password_entry") {
        nativeSave = request;
        return { type: "save_entry", entryName: "Example account" };
      }
      return { type: "error", message: "Unexpected request." };
    });
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.pageContextChanged",
        documentToken: "a".repeat(64),
      },
      7
    );
    const mountedCandidateId = (
      mock.chrome.tabs.sendMessage.mock.calls.find(
        ([, message]) =>
          (message as { type?: string }).type ===
          "termkey.pendingLoginPrompt.mount"
      )?.[1] as { candidateId: string }
    ).candidateId;

    await expect(
      service.handleMessage(
        {
          type: "termkey.pendingLogin.save",
          name: "Example account",
          username: "edited-user",
          secondaryPassword: "vault-secret",
        },
        mock.extensionSender
      )
    ).resolves.toEqual({
      ok: true,
      response: {
        type: "save_entry_result",
        entryName: "Example account",
      },
    });
    expect(nativeSave).toMatchObject({
      type: "save_password_entry",
      name: "Example account",
      username: "edited-user",
      password: "background-only-secret",
      url: "https://example.test",
      secondaryPassword: "vault-secret",
    });
    expect(mock.chrome.tabs.sendMessage).toHaveBeenCalledWith(
      7,
      {
        type: "termkey.pendingLoginPrompt.complete",
        candidateId: mountedCandidateId,
        outcome: "saved",
      },
      { frameId: 0 }
    );
    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toMatchObject({ response: { candidate: null } });
  });

  it("offers a ready pending login in unlock mode while the vault is locked", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "captured-user",
      password: "background-only-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    mock.setNativeResponder((request) => {
      const typed = request as { type?: string; protocolVersion?: number };
      if (typed.type === "status" && typed.protocolVersion === 3) {
        return { ...statusResponse, locked: true };
      }
      if (typed.type === "status") {
        return {
          type: "error",
          message:
            "TermKey browser integration is out of date. Run `termkey browser repair`.",
        };
      }
      return { type: "error", message: "Unexpected request." };
    });
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      { type: "termkey.content.loginSubmitted", documentToken: "a".repeat(64) },
      7
    );
    await mock.dispatchContentMessage(
      { type: "termkey.content.pageContextChanged", documentToken: "a".repeat(64) },
      7
    );

    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toMatchObject({
      ok: true,
      response: { type: "pending_login", candidate: { mode: "unlock" } },
    });
  });

  it("does not return locked metadata for a candidate replaced during the status request", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "first-user",
      password: "first-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    mock.setNativeResponder(() => undefined);
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      { type: "termkey.content.loginSubmitted", documentToken: "a".repeat(64) },
      7
    );
    await mock.dispatchContentMessage(
      { type: "termkey.content.pageContextChanged", documentToken: "a".repeat(64) },
      7
    );

    const pendingGet = service.handleMessage(
      { type: "termkey.pendingLogin.get" },
      mock.extensionSender
    );
    await vi.waitFor(() => {
      expect(
        mock.ports.flatMap((port) =>
          port.postMessage.mock.calls.filter(
            ([request]) => (request as { type?: string }).type === "status"
          )
        )
      ).toHaveLength(1);
    });

    mock.setSubmittedLogin({
      ok: true,
      username: "replacement-user",
      password: "replacement-secret",
    });
    await mock.dispatchContentMessage(
      { type: "termkey.content.loginSubmitted", documentToken: "b".repeat(64) },
      7
    );
    await mock.dispatchContentMessage(
      { type: "termkey.content.pageContextChanged", documentToken: "b".repeat(64) },
      7
    );

    const statusPort = mock.ports.find((port) =>
      port.postMessage.mock.calls.some(
        ([request]) => (request as { type?: string }).type === "status"
      )
    );
    if (!statusPort) {
      throw new Error("Status request was not sent.");
    }
    const statusCallIndex = statusPort.postMessage.mock.calls.findIndex(
      ([request]) => (request as { type?: string }).type === "status"
    );
    statusPort.onMessage.emit(
      correlatedResponse(
        statusPort,
        { ...statusResponse, locked: true },
        statusCallIndex
      )
    );

    await expect(pendingGet).resolves.toEqual({
      ok: true,
      response: { type: "pending_login", candidate: null },
    });
  });

  it("unlocks, resolves, and saves using the background-owned website password", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({ ok: true, username: "captured-user", password: "website-secret" });
    mock.setPageContext({ intent: "unknown", hasPasswordField: false, hasVisibleLoginFailure: false });
    const nativeRequests: Array<Record<string, unknown>> = [];
    const recoveryNotice = "Configure a new recovery phrase.";
    mock.setNativeResponder((request) => {
      nativeRequests.push(request as Record<string, unknown>);
      switch ((request as { type?: string }).type) {
        case "unlock": return { type: "unlock", unlocked: true, recoveryNotice };
        case "find_site_matches": return { ...siteMatchesResponse, matches: [] };
        case "save_password_entry": return { type: "save_entry", entryName: "Example account" };
        default: return { type: "error", message: "Unexpected request." };
      }
    });
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage({ type: "termkey.content.loginSubmitted", documentToken: "a".repeat(64) }, 7);
    await mock.dispatchContentMessage({ type: "termkey.content.pageContextChanged", documentToken: "a".repeat(64) }, 7);

    await expect(service.handleMessage({
      type: "termkey.pendingLogin.unlockAndSave",
      name: "Example account",
      username: "edited-user",
      masterPassword: "master-secret",
      secondaryPassword: "vault-secret",
    }, mock.extensionSender)).resolves.toEqual({
      ok: true,
      response: {
        type: "unlock_and_save_result",
        unlocked: true,
        saved: true,
        mode: "save",
        entryName: "Example account",
        recoveryNotice,
      },
    });
    expect(nativeRequests.map((request) => request.type)).toEqual([
      "unlock",
      "find_site_matches",
      "save_password_entry",
    ]);
    expect(nativeRequests.filter((request) => request.password === "website-secret")).toEqual([
      expect.objectContaining({ type: "save_password_entry", password: "website-secret" }),
    ]);
    expect(nativeRequests[0]).toMatchObject({ type: "unlock", password: "master-secret" });
  });

  it("keeps match-resolution failures unresolved after a successful unlock", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "captured-user",
      password: "website-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    const nativeRequests: Array<Record<string, unknown>> = [];
    const recoveryNotice = "Configure a new recovery phrase.";
    mock.setNativeResponder((request) => {
      nativeRequests.push(request as Record<string, unknown>);
      switch ((request as { type?: string }).type) {
        case "unlock":
          return { type: "unlock", unlocked: true, recoveryNotice };
        case "find_site_matches":
          return { type: "error", message: "Could not inspect saved logins." };
        default:
          return { type: "error", message: "Unexpected request." };
      }
    });
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      { type: "termkey.content.loginSubmitted", documentToken: "a".repeat(64) },
      7
    );
    await mock.dispatchContentMessage(
      { type: "termkey.content.pageContextChanged", documentToken: "a".repeat(64) },
      7
    );

    const result = await service.handleMessage(
      {
        type: "termkey.pendingLogin.unlockAndSave",
        name: "Example account",
        masterPassword: "master-secret",
      },
      mock.extensionSender
    );

    expect(result).toEqual({
      ok: true,
      response: {
        type: "unlock_and_save_result",
        unlocked: true,
        saved: false,
        error: "Could not inspect saved logins.",
        recoveryNotice,
      },
    });
    expect(nativeRequests.map((request) => request.type)).toEqual([
      "unlock",
      "find_site_matches",
    ]);
    expect(
      nativeRequests.filter(
        (request) =>
          request.type === "save_password_entry" ||
          request.type === "update_password_entry"
      )
    ).toHaveLength(0);
  });

  it("updates the matched login after unlocking", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({ ok: true, username: "person@example.test", password: "website-secret" });
    mock.setPageContext({ intent: "unknown", hasPasswordField: false, hasVisibleLoginFailure: false });
    const nativeRequests: Array<Record<string, unknown>> = [];
    mock.setNativeResponder((request) => {
      nativeRequests.push(request as Record<string, unknown>);
      switch ((request as { type?: string }).type) {
        case "unlock": return { type: "unlock", unlocked: true };
        case "find_site_matches": return siteMatchesResponse;
        case "update_password_entry": return { type: "save_entry", entryName: "Existing account" };
        default: return { type: "error", message: "Unexpected request." };
      }
    });
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage({ type: "termkey.content.loginSubmitted", documentToken: "a".repeat(64) }, 7);
    await mock.dispatchContentMessage({ type: "termkey.content.pageContextChanged", documentToken: "a".repeat(64) }, 7);

    await expect(service.handleMessage({
      type: "termkey.pendingLogin.unlockAndSave",
      name: "Existing account",
      masterPassword: "master-secret",
    }, mock.extensionSender)).resolves.toMatchObject({
      ok: true,
      response: { unlocked: true, saved: true, mode: "update" },
    });
    expect(nativeRequests.map((request) => request.type)).toEqual([
      "unlock",
      "find_site_matches",
      "update_password_entry",
    ]);
  });

  it("retains the pending login when unlocking fails without resolving or saving", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({ ok: true, username: "captured-user", password: "website-secret" });
    mock.setPageContext({ intent: "unknown", hasPasswordField: false, hasVisibleLoginFailure: false });
    const nativeRequests: Array<Record<string, unknown>> = [];
    mock.setNativeResponder((request) => {
      nativeRequests.push(request as Record<string, unknown>);
      if ((request as { type?: string }).type === "unlock") {
        return { type: "error", message: "Incorrect master password." };
      }
      if ((request as { type?: string }).type === "status") return { ...statusResponse, locked: true };
      return { type: "error", message: "Unexpected request." };
    });
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage({ type: "termkey.content.loginSubmitted", documentToken: "a".repeat(64) }, 7);
    await mock.dispatchContentMessage({ type: "termkey.content.pageContextChanged", documentToken: "a".repeat(64) }, 7);

    await expect(service.handleMessage({
      type: "termkey.pendingLogin.unlockAndSave",
      name: "Example account",
      masterPassword: "wrong-master-secret",
    }, mock.extensionSender)).resolves.toEqual({
      ok: true,
      response: { type: "unlock_and_save_result", unlocked: false, saved: false, error: "Incorrect master password." },
    });
    expect(nativeRequests.map((request) => request.type)).toEqual(["unlock"]);
    await expect(service.handleMessage({ type: "termkey.pendingLogin.get" }, mock.extensionSender)).resolves.toMatchObject({
      ok: true,
      response: { candidate: { mode: "unlock" } },
    });
  });

  it("rejects a concurrent unlock-and-save transaction for the same pending login", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({ ok: true, username: "captured-user", password: "website-secret" });
    mock.setPageContext({ intent: "unknown", hasPasswordField: false, hasVisibleLoginFailure: false });
    const nativeRequests: Array<Record<string, unknown>> = [];
    mock.setNativeResponder((request) => {
      nativeRequests.push(request as Record<string, unknown>);
      switch ((request as { type?: string }).type) {
        case "unlock": return { type: "unlock", unlocked: true };
        case "find_site_matches": return { ...siteMatchesResponse, matches: [] };
        case "save_password_entry": return { type: "save_entry", entryName: "Example account" };
        default: return { type: "error", message: "Unexpected request." };
      }
    });
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage({ type: "termkey.content.loginSubmitted", documentToken: "a".repeat(64) }, 7);
    await mock.dispatchContentMessage({ type: "termkey.content.pageContextChanged", documentToken: "a".repeat(64) }, 7);
    const message = {
      type: "termkey.pendingLogin.unlockAndSave" as const,
      name: "Example account",
      masterPassword: "master-secret",
    };

    const first = service.handleMessage(message, mock.extensionSender);
    const second = service.handleMessage(message, mock.extensionSender);

    await expect(first).resolves.toMatchObject({
      ok: true,
      response: { type: "unlock_and_save_result", unlocked: true, saved: true },
    });
    await expect(second).resolves.toEqual({
      ok: false,
      error: "A pending login save is already in progress.",
    });
    expect(nativeRequests.filter((request) => request.type === "save_password_entry")).toHaveLength(1);
  });

  it("updates the matched login instead of appending a duplicate", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "person@example.test",
      password: "background-only-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    const nativeRequests: unknown[] = [];
    mock.setNativeResponder((request) => {
      nativeRequests.push(request);
      const type = (request as { type?: string }).type;
      if (type === "status") {
        return statusResponse;
      }
      if (type === "find_site_matches") {
        return siteMatchesResponse;
      }
      if (type === "update_password_entry") {
        return { type: "save_entry", entryName: "Renamed account" };
      }
      return { type: "error", message: "Unexpected request." };
    });
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.pageContextChanged",
        documentToken: "a".repeat(64),
      },
      7
    );
    const mountedCandidateId = (
      mock.chrome.tabs.sendMessage.mock.calls.find(
        ([, message]) =>
          (message as { type?: string }).type ===
          "termkey.pendingLoginPrompt.mount"
      )?.[1] as { candidateId: string }
    ).candidateId;
    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toMatchObject({
      response: { candidate: { mode: "update" } },
    });

    await expect(
      service.handleMessage(
        {
          type: "termkey.pendingLogin.save",
          name: "Renamed account",
          username: "edited@example.test",
          secondaryPassword: "vault-secret",
        },
        mock.extensionSender
      )
    ).resolves.toEqual({
      ok: true,
      response: {
        type: "save_entry_result",
        entryName: "Renamed account",
      },
    });
    expect(nativeRequests).toContainEqual(
      expect.objectContaining({
        type: "update_password_entry",
        id: "entry-1",
        origin: "https://example.test",
        name: "Renamed account",
        username: "edited@example.test",
        password: "background-only-secret",
        url: "https://example.test",
        secondaryPassword: "vault-secret",
      })
    );
    expect(
      nativeRequests.some(
        (request) =>
          (request as { type?: string }).type === "save_password_entry"
      )
    ).toBe(false);
    expect(mock.chrome.tabs.sendMessage).toHaveBeenCalledWith(
      7,
      {
        type: "termkey.pendingLoginPrompt.complete",
        candidateId: mountedCandidateId,
        outcome: "updated",
      },
      { frameId: 0 }
    );
  });

  it("retains a pending login when native save fails", async () => {
    const mock = createChromeMock();
    mock.setSubmittedLogin({
      ok: true,
      username: "sam",
      password: "background-only-secret",
    });
    mock.setPageContext({
      intent: "unknown",
      hasPasswordField: false,
      hasVisibleLoginFailure: false,
    });
    mock.setNativeResponder((request) => {
      const type = (request as { type?: string }).type;
      if (type === "status") {
        return statusResponse;
      }
      if (type === "save_password_entry") {
        return { type: "error", message: "Vault remains locked." };
      }
      if (type === "find_site_matches") {
        return { ...siteMatchesResponse, matches: [] };
      }
      return { type: "error", message: "Unexpected request." };
    });
    const service = createBackgroundService(mock.chrome);
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.loginSubmitted",
        documentToken: "a".repeat(64),
      },
      7
    );
    await mock.dispatchContentMessage(
      {
        type: "termkey.content.pageContextChanged",
        documentToken: "a".repeat(64),
      },
      7
    );

    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.save", name: "Example" },
        mock.extensionSender
      )
    ).resolves.toEqual({ ok: false, error: "Vault remains locked." });
    await expect(
      service.handleMessage(
        { type: "termkey.pendingLogin.get" },
        mock.extensionSender
      )
    ).resolves.toMatchObject({
      ok: true,
      response: { candidate: { username: "sam" } },
    });
  });

  it("discovers HTTP tabs using their canonical origin", async () => {
    const mock = createChromeMock({ id: 7, url: "http://example.test/login" });
    installHappyPathResponders(mock);
    mock.setNativeResponder((request) => {
      if ((request as { type?: string }).type === "find_site_matches") {
        return {
          ...siteMatchesResponse,
          siteUrl: "http://example.test",
          siteOrigin: "http://example.test",
        };
      }
      return { type: "error", message: "Unexpected request." };
    });
    const service = createBackgroundService(mock.chrome);

    const result = await service.handleMessage(
      { type: "termkey.nativeHost.findSiteMatches" },
      mock.extensionSender
    );

    expect(result).toMatchObject({
      ok: true,
      response: { type: "site_matches" },
    });
    expect(mock.chrome.runtime.connectNative).toHaveBeenCalledTimes(1);
  });

  it("rejects HTTPS tabs containing user information", async () => {
    const mock = createChromeMock({
      id: 7,
      url: "https://user:password@example.test/login",
    });
    const service = createBackgroundService(mock.chrome);

    const result = await service.handleMessage(
      { type: "termkey.nativeHost.findSiteMatches" },
      mock.extensionSender
    );

    expect(result).toEqual({
      ok: false,
      error: "Current tab URL must not contain user information.",
    });
    expect(mock.chrome.scripting.executeScript).not.toHaveBeenCalled();
    expect(mock.chrome.runtime.connectNative).not.toHaveBeenCalled();
  });

  it("persists an HTTP origin at the privileged save boundary", async () => {
    const mock = createChromeMock();
    let nativeRequest: unknown;
    mock.setNativeResponder((request) => {
      nativeRequest = request;
      return { type: "save_entry", entryName: "Example" };
    });
    const service = createBackgroundService(mock.chrome);

    const result = await service.handleMessage(
      {
        type: "termkey.nativeHost.savePasswordEntry",
        name: "Example",
        password: "secret",
        url: "http://example.test/login",
      },
      mock.extensionSender
    );

    expect(result).toMatchObject({
      ok: true,
      response: { type: "save_entry_result", entryName: "Example" },
    });
    expect(nativeRequest).toMatchObject({
      type: "save_password_entry",
      url: "http://example.test",
    });
  });

  it("rejects URL user information at the privileged save boundary", async () => {
    const mock = createChromeMock();
    const service = createBackgroundService(mock.chrome);

    const result = await service.handleMessage(
      {
        type: "termkey.nativeHost.savePasswordEntry",
        name: "Example",
        password: "secret",
        url: "https://user:password@example.test/login",
      },
      mock.extensionSender
    );

    expect(result).toEqual({
      ok: false,
      error: "Current tab URL must not contain user information.",
    });
    expect(mock.chrome.runtime.connectNative).not.toHaveBeenCalled();
  });

  it("persists only the canonical HTTPS origin at the privileged save boundary", async () => {
    const mock = createChromeMock();
    let nativeRequest: unknown;
    mock.setNativeResponder((request) => {
      nativeRequest = request;
      return { type: "save_entry", entryName: "Example" };
    });
    const service = createBackgroundService(mock.chrome);

    await expect(
      service.handleMessage(
        {
          type: "termkey.nativeHost.savePasswordEntry",
          name: "Example",
          username: "person@example.test",
          password: "secret",
          url: "https://EXAMPLE.test:443/path?query=yes#fragment",
        },
        mock.extensionSender
      )
    ).resolves.toMatchObject({ ok: true });
    expect(nativeRequest).toEqual({
      type: "save_password_entry",
      name: "Example",
      username: "person@example.test",
      password: "secret",
      url: "https://example.test",
      secondaryPassword: undefined,
      requestId: expect.stringMatching(/^[a-f0-9]{64}$/),
    });
  });

  it("uses only an HTTPS origin without path query or fragment for generated passwords", async () => {
    const mock = createChromeMock({
      id: 7,
      url: "https://EXAMPLE.test:443/account/login?next=%2Fvault#password",
    });
    mock.setTabMessageHandler((_tabId, message) => {
      const type = (message as { type?: string }).type;
      if (type === "termkey.contentScriptProbe") {
        return { ok: true, documentToken: "a".repeat(64) };
      }
      if (type === "termkey.fillGeneratedPassword") {
        return {
          ok: true,
          username: "person@example.test",
          filledPasswordFields: 2,
        };
      }
      throw new Error(`Unexpected tab message: ${type}`);
    });
    mock.setNativeResponder((request) => {
      if ((request as { type?: string }).type === "generate_password") {
        return { type: "generated_password", password: "generated-secret" };
      }
      return { type: "error", message: "Unexpected request." };
    });
    const service = createBackgroundService(mock.chrome);

    await expect(
      service.handleMessage(
        { type: "termkey.passwords.generateForPage" },
        mock.extensionSender
      )
    ).resolves.toMatchObject({
      ok: true,
      response: {
        type: "generated_password",
        candidate: {
          username: "person@example.test",
          password: "generated-secret",
          url: "https://example.test",
        },
      },
    });
  });

  it("returns only the authenticated canonical origin for popup presentation", async () => {
    const mock = createChromeMock({
      id: 7,
      url: "https://EXAMPLE.test:443/account/login?next=%2Fvault#password",
    });
    mock.setTabMessageHandler(() => ({
      ok: true,
      documentToken: "a".repeat(64),
    }));
    mock.setNativeResponder((request) => {
      if ((request as { type?: string }).type === "find_site_matches") {
        return {
          ...siteMatchesResponse,
          siteUrl: "https://example.test/native/path?query=yes#fragment",
          siteOrigin: "https://example.test/native/path?query=yes#fragment",
          siteHostname: "untrusted-display.example",
        };
      }
      return { type: "error", message: "Unexpected request." };
    });
    const service = createBackgroundService(mock.chrome, {
      generateGrantId: () => "canonical-origin-grant",
    });

    await expect(
      service.handleMessage(
        { type: "termkey.nativeHost.findSiteMatches" },
        mock.extensionSender
      )
    ).resolves.toMatchObject({
      ok: true,
      response: {
        siteUrl: "https://example.test",
        siteOrigin: "https://example.test",
        siteHostname: "example.test",
      },
    });
  });

  it("keeps same-entry discovery grants bound to their own tab and document", async () => {
    const mock = createChromeMock({
      id: 1,
      url: "https://example.test/login",
    });
    const tokens = new Map([
      [1, "a".repeat(64)],
      [2, "b".repeat(64)],
    ]);
    mock.setTabMessageHandler((tabId, message) => {
      if (
        (message as { type?: string }).type === "termkey.contentScriptProbe"
      ) {
        return { ok: true, documentToken: tokens.get(tabId) };
      }
      if ((message as { type?: string }).type === "termkey-fill-credentials") {
        return {
          ok: true,
          filledFields: 2,
          filledUsername: true,
          filledPassword: true,
        };
      }
      return { ok: true, documentToken: tokens.get(tabId) };
    });
    mock.setNativeResponder((request) => {
      if ((request as { type?: string }).type === "find_site_matches") {
        return siteMatchesResponse;
      }
      return {
        type: "autofill_entry",
        entry: {
          id: "entry-1",
          name: "Example",
          username: "person@example.test",
          password: "secret",
          url: "https://example.test",
        },
      };
    });
    let nextGrant = 0;
    const service = createBackgroundService(mock.chrome, {
      generateGrantId: () => `grant-${++nextGrant}`,
    });

    const firstDiscovery = await service.handleMessage(
      { type: "termkey.nativeHost.findSiteMatches" },
      mock.extensionSender
    );
    mock.setActiveTab({ id: 2, url: "https://example.test/account" });
    const secondDiscovery = await service.handleMessage(
      { type: "termkey.nativeHost.findSiteMatches" },
      mock.extensionSender
    );
    if (!firstDiscovery.ok || !secondDiscovery.ok) {
      throw new Error("Expected both discoveries to succeed.");
    }
    const firstGrant = (
      firstDiscovery.response as {
        matches: Array<{ grantId: string }>;
      }
    ).matches[0].grantId;
    const secondGrant = (
      secondDiscovery.response as {
        matches: Array<{ grantId: string }>;
      }
    ).matches[0].grantId;

    await expect(
      service.handleMessage(
        {
          type: "termkey.autofill.fillSelectedMatch",
          grantId: firstGrant,
          entryId: "entry-1",
        },
        mock.extensionSender
      )
    ).resolves.toMatchObject({ ok: true });
    expect(mock.chrome.tabs.sendMessage).toHaveBeenCalledWith(
      1,
      expect.objectContaining({ type: "termkey-fill-credentials" }),
      { frameId: 0 }
    );
    expect(mock.chrome.tabs.sendMessage).not.toHaveBeenCalledWith(
      2,
      expect.objectContaining({ type: "termkey-fill-credentials" }),
      { frameId: 0 }
    );

    await expect(
      service.handleMessage(
        {
          type: "termkey.autofill.fillSelectedMatch",
          grantId: secondGrant,
          entryId: "entry-1",
        },
        mock.extensionSender
      )
    ).resolves.toMatchObject({ ok: true });
    expect(mock.chrome.tabs.sendMessage).toHaveBeenCalledWith(
      2,
      expect.objectContaining({ type: "termkey-fill-credentials" }),
      { frameId: 0 }
    );
  });

  it("releases a reserved grant after a native secondary-password error so the popup can retry", async () => {
    const mock = createChromeMock();
    installHappyPathResponders(mock);
    mock.setNativeResponder((request) => {
      const typed = request as {
        type?: string;
        secondaryPassword?: string;
      };
      if (typed.type === "find_site_matches") {
        return siteMatchesResponse;
      }
      if (
        typed.type === "get_autofill_entry" &&
        typed.secondaryPassword === "wrong"
      ) {
        return { type: "error", message: "Invalid secondary password" };
      }
      if (
        typed.type === "get_autofill_entry" &&
        typed.secondaryPassword === "correct"
      ) {
        return {
          type: "autofill_entry",
          entry: {
            id: "entry-1",
            name: "Example",
            username: "person@example.test",
            password: "secret",
            url: "https://example.test",
          },
        };
      }
      return { type: "error", message: "Unexpected request." };
    });
    const service = createBackgroundService(mock.chrome, {
      generateGrantId: () => "retry-grant",
    });
    const discovery = await service.handleMessage(
      { type: "termkey.nativeHost.findSiteMatches" },
      mock.extensionSender
    );
    const grantId = grantIdFromDiscovery(discovery);

    await expect(
      service.handleMessage(
        {
          type: "termkey.autofill.fillSelectedMatch",
          grantId,
          entryId: "entry-1",
          secondaryPassword: "wrong",
        },
        mock.extensionSender
      )
    ).resolves.toEqual({
      ok: false,
      error: "Invalid secondary password",
    });
    await expect(
      service.handleMessage(
        {
          type: "termkey.autofill.fillSelectedMatch",
          grantId,
          entryId: "entry-1",
          secondaryPassword: "correct",
        },
        mock.extensionSender
      )
    ).resolves.toMatchObject({ ok: true });
    expect(
      mock.chrome.tabs.sendMessage.mock.calls.filter(
        ([, message]) =>
          (message as { type?: string }).type === "termkey-fill-credentials"
      )
    ).toHaveLength(1);
    expect(mock.chrome.tabs.sendMessage).toHaveBeenLastCalledWith(
      7,
      expect.objectContaining({ type: "termkey-fill-credentials" }),
      { frameId: 0 }
    );
    expect(mock.ports).toHaveLength(1);
  });

  it("atomically reserves a grant so concurrent duplicate fills cannot retrieve twice", async () => {
    const mock = createChromeMock();
    installHappyPathResponders(mock);
    const service = createBackgroundService(mock.chrome, {
      generateGrantId: () => "exclusive-grant",
    });
    const discovery = await service.handleMessage(
      { type: "termkey.nativeHost.findSiteMatches" },
      mock.extensionSender
    );
    const grantId = grantIdFromDiscovery(discovery);
    mock.setNativeResponder(() => undefined);

    const first = service.handleMessage(
      {
        type: "termkey.autofill.fillSelectedMatch",
        grantId,
        entryId: "entry-1",
      },
      mock.extensionSender
    );
    await vi.waitFor(() => {
      expect(
        mock.ports.flatMap((port) =>
          port.postMessage.mock.calls.filter(
            ([request]) =>
              (request as { type?: string }).type === "get_autofill_entry"
          )
        )
      ).toHaveLength(1);
    });
    await expect(
      service.handleMessage(
        {
          type: "termkey.autofill.fillSelectedMatch",
          grantId,
          entryId: "entry-1",
        },
        mock.extensionSender
      )
    ).resolves.toMatchObject({ ok: false });

    const retrievalPort = mock.ports.find((port) =>
      port.postMessage.mock.calls.some(
        ([request]) =>
          (request as { type?: string }).type === "get_autofill_entry"
      )
    );
    if (!retrievalPort) {
      throw new Error("Expected an in-flight autofill retrieval.");
    }
    retrievalPort.onMessage.emit(correlatedResponse(retrievalPort, {
      type: "autofill_entry",
      entry: {
        id: "entry-1",
        name: "Example",
        username: "person@example.test",
        password: "secret",
        url: "https://example.test",
      },
    }));
    await expect(first).resolves.toMatchObject({ ok: true });
    expect(
      mock.ports.flatMap((port) =>
        port.postMessage.mock.calls.filter(
          ([request]) =>
            (request as { type?: string }).type === "get_autofill_entry"
        )
      )
    ).toHaveLength(1);
  });

  it("aborts autofill when the tab origin changes", async () => {
    const mock = createChromeMock();
    installHappyPathResponders(mock);
    const service = createBackgroundService(mock.chrome);
    const discovery = await service.handleMessage(
      { type: "termkey.nativeHost.findSiteMatches" },
      mock.extensionSender
    );
    mock.setActiveTab({ id: 7, url: "https://attacker.test/login" });

    const result = await service.handleMessage(
        {
          type: "termkey.autofill.fillSelectedMatch",
          grantId: grantIdFromDiscovery(discovery),
          entryId: "entry-1",
      },
      mock.extensionSender
    );

    expect(result.ok).toBe(false);
    expect(mock.ports[0].postMessage).toHaveBeenCalledTimes(2);
    expect(mock.chrome.tabs.sendMessage).not.toHaveBeenCalledWith(
      7,
      expect.objectContaining({ type: "termkey-fill-credentials" }),
      { frameId: 0 }
    );
  });

  it("aborts autofill when the per-document token changes", async () => {
    const mock = createChromeMock();
    const page = installHappyPathResponders(mock);
    const service = createBackgroundService(mock.chrome);
    const discovery = await service.handleMessage(
      { type: "termkey.nativeHost.findSiteMatches" },
      mock.extensionSender
    );
    page.setDocumentToken("b".repeat(64));

    const result = await service.handleMessage(
        {
          type: "termkey.autofill.fillSelectedMatch",
          grantId: grantIdFromDiscovery(discovery),
          entryId: "entry-1",
      },
      mock.extensionSender
    );

    expect(result.ok).toBe(false);
    expect(mock.ports[0].postMessage).toHaveBeenCalledTimes(2);
  });

  it("injects the content script only after a user action", async () => {
    const mock = createChromeMock();
    let injected = false;
    mock.setTabMessageHandler((_tabId, message) => {
      if (
        (message as { type?: string }).type === "termkey.contentScriptProbe" &&
        !injected
      ) {
        throw new Error("Could not establish connection. Receiving end does not exist.");
      }
      return { ok: true, documentToken: "a".repeat(64) };
    });
    mock.chrome.scripting.executeScript.mockImplementation(async (details) => {
      injected = true;
      expect(details).toEqual({
        target: { tabId: 7, frameIds: [0] },
        files: ["dist/content.js"],
      });
    });
    mock.setNativeResponder(() => siteMatchesResponse);
    const service = createBackgroundService(mock.chrome);

    expect(mock.chrome.scripting.executeScript).not.toHaveBeenCalled();
    const discovery = await service.handleMessage(
      { type: "termkey.nativeHost.findSiteMatches" },
      mock.extensionSender
    );

    expect(mock.chrome.scripting.executeScript).toHaveBeenCalledTimes(1);
    expect(mock.chrome.tabs.query).toHaveBeenCalledTimes(1);
  });

  it("aborts autofill when the origin changes after native retrieval", async () => {
    const mock = createChromeMock();
    installHappyPathResponders(mock);
    const service = createBackgroundService(mock.chrome);
    const discovery = await service.handleMessage(
      { type: "termkey.nativeHost.findSiteMatches" },
      mock.extensionSender
    );
    mock.setNativeResponder((request) => {
      if ((request as { type?: string }).type === "get_autofill_entry") {
        mock.setActiveTab({ id: 7, url: "https://attacker.test/" });
        return {
          type: "autofill_entry",
          entry: {
            id: "entry-1",
            name: "Example",
            username: "person@example.test",
            password: "secret",
            url: "https://example.test",
          },
        };
      }
      return { type: "error", message: "Unexpected request." };
    });

    const result = await service.handleMessage(
      {
        type: "termkey.autofill.fillSelectedMatch",
        grantId: grantIdFromDiscovery(discovery),
        entryId: "entry-1",
      },
      mock.extensionSender
    );

    expect(result.ok).toBe(false);
    expect(mock.chrome.tabs.sendMessage).not.toHaveBeenCalledWith(
      7,
      expect.objectContaining({ type: "termkey-fill-credentials" }),
      { frameId: 0 }
    );
  });

  it("aborts autofill when the document token changes after native retrieval", async () => {
    const mock = createChromeMock();
    const page = installHappyPathResponders(mock);
    const service = createBackgroundService(mock.chrome);
    const discovery = await service.handleMessage(
      { type: "termkey.nativeHost.findSiteMatches" },
      mock.extensionSender
    );
    mock.setNativeResponder((request) => {
      if ((request as { type?: string }).type === "get_autofill_entry") {
        page.setDocumentToken("b".repeat(64));
        return {
          type: "autofill_entry",
          entry: {
            id: "entry-1",
            name: "Example",
            username: "person@example.test",
            password: "secret",
            url: "https://example.test",
          },
        };
      }
      return { type: "error", message: "Unexpected request." };
    });

    const result = await service.handleMessage(
      {
        type: "termkey.autofill.fillSelectedMatch",
        grantId: grantIdFromDiscovery(discovery),
        entryId: "entry-1",
      },
      mock.extensionSender
    );

    expect(result.ok).toBe(false);
    expect(mock.chrome.tabs.sendMessage).not.toHaveBeenCalledWith(
      7,
      expect.objectContaining({ type: "termkey-fill-credentials" }),
      { frameId: 0 }
    );
  });

  it("binds the granted document token inside the final secret message", async () => {
    const mock = createChromeMock();
    installHappyPathResponders(mock);
    const service = createBackgroundService(mock.chrome);
    const discovery = await service.handleMessage(
      { type: "termkey.nativeHost.findSiteMatches" },
      mock.extensionSender
    );
    mock.setTabMessageHandler((_tabId, message) => {
      const typed = message as { type?: string; documentToken?: string };
      if (typed.type === "termkey.contentScriptProbe") {
        return { ok: true, documentToken: "a".repeat(64) };
      }
      if (typed.type === "termkey-fill-credentials") {
        expect(typed.documentToken).toBe("a".repeat(64));
        return {
          ok: false,
          error: "The page document changed before delivery.",
        };
      }
      return { ok: true };
    });

    const result = await service.handleMessage(
      {
        type: "termkey.autofill.fillSelectedMatch",
        grantId: grantIdFromDiscovery(discovery),
        entryId: "entry-1",
      },
      mock.extensionSender
    );

    expect(result).toEqual({
      ok: false,
      error: "The page document changed before delivery.",
    });
  });

  it("rejects an expired match grant before native retrieval", async () => {
    const mock = createChromeMock();
    installHappyPathResponders(mock);
    let now = 1_000;
    const service = createBackgroundService(mock.chrome, {
      now: () => now,
      grantTtlMs: 30_000,
    });
    const discovery = await service.handleMessage(
      { type: "termkey.nativeHost.findSiteMatches" },
      mock.extensionSender
    );
    now += 30_000;

    const result = await service.handleMessage(
      {
        type: "termkey.autofill.fillSelectedMatch",
        grantId: grantIdFromDiscovery(discovery),
        entryId: "entry-1",
      },
      mock.extensionSender
    );

    expect(result.ok).toBe(false);
    expect(mock.ports[0].postMessage).toHaveBeenCalledTimes(2);
    expect(mock.chrome.tabs.sendMessage).not.toHaveBeenCalledWith(
      7,
      expect.objectContaining({ type: "termkey-fill-credentials" }),
      { frameId: 0 }
    );
  });

  it("times out and resets a silent native port after ten seconds", async () => {
    vi.useFakeTimers();
    const ports: MockNativePort[] = [];
    const connectNative = vi.fn(() => {
      const port = new MockNativePort(ports.length > 0);
      ports.push(port);
      return port;
    });
    const client = new NativeHostClient(connectNative);
    const first = client.request({ type: "status" });
    const queued = client.request({ type: "ping", protocolVersion: 3 });
    expect(ports[0].postMessage).toHaveBeenCalledTimes(1);
    let firstSettled = false;
    void first.then(() => {
      firstSettled = true;
    });

    await vi.advanceTimersByTimeAsync(NATIVE_REQUEST_TIMEOUT_MS - 1);
    expect(firstSettled).toBe(false);
    expect(ports[0].disconnect).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1);
    await expect(first).resolves.toMatchObject({ ok: false });
    await expect(queued).resolves.toMatchObject({ ok: false });
    expect(ports[0].disconnect).toHaveBeenCalledTimes(1);

    const later = client.request({ type: "status" });
    expect(connectNative).toHaveBeenCalledTimes(2);
    ports[1].onMessage.emit(correlatedResponse(ports[1], {
      type: "status",
      app: "termkey",
      version: "1.0.0",
      vaultPath: "/vault",
      vaultExists: true,
      firstRunComplete: true,
      recoveryConfigured: true,
      locked: false,
      ...protocolInfo,
    }));
    await expect(later).resolves.toMatchObject({ ok: true });
  });

  it("recovers on a fresh port after connectNative throws", async () => {
    const port = new MockNativePort();
    let attempts = 0;
    const client = new NativeHostClient(() => {
      attempts += 1;
      if (attempts === 1) {
        throw new Error("connect failed");
      }
      return port;
    });

    await expect(client.request({ type: "status" })).resolves.toEqual({
      ok: false,
      error: "connect failed",
    });
    const later = client.request({ type: "status" });
    port.onMessage.emit(correlatedResponse(port, statusResponse));
    await expect(later).resolves.toMatchObject({ ok: true });
    expect(attempts).toBe(2);
  });

  it("keeps connection-local unlocked state for subsequent native requests", async () => {
    const ports: MockNativePort[] = [];
    let nextRequestId = 0;
    const client = new NativeHostClient(
      () => {
        const port = new MockNativePort(false);
        let unlocked = false;
        ports.push(port);
        port.postMessage.mockImplementation((wireRequest) => {
          const request = wireRequest as {
            type: string;
            requestId?: string;
          };
          if (request.type === "ping") {
            port.onMessage.emit({
              type: "pong",
              app: "termkey",
              version: "1.0.0",
              requestId: request.requestId,
              ...protocolInfo,
            });
            return;
          }
          if (request.type === "unlock") {
            unlocked = true;
            port.onMessage.emit({
              type: "unlock",
              requestId: request.requestId,
              unlocked: true,
            });
            return;
          }
          if (request.type === "status") {
            port.onMessage.emit({
              ...statusResponse,
              requestId: request.requestId,
              locked: !unlocked,
            });
          }
        });
        return port;
      },
      undefined,
      () => String(nextRequestId++).padStart(64, "a")
    );

    await expect(
      client.request({ type: "unlock", password: "master-password" })
    ).resolves.toMatchObject({ ok: true });
    await expect(client.request({ type: "status" })).resolves.toMatchObject({
      ok: true,
      response: { locked: false },
    });
    expect(ports).toHaveLength(1);
    expect(ports[0].disconnect).not.toHaveBeenCalled();
  });

  it("negotiates once before privileged work and renegotiates on a fresh port", async () => {
    const ports: MockNativePort[] = [];
    let nextRequestId = 0;
    const client = new NativeHostClient(
      () => {
        const port = new MockNativePort(false);
        ports.push(port);
        return port;
      },
      undefined,
      () => String(nextRequestId++).padStart(64, "a")
    );

    const unlock = client.request({
      type: "unlock",
      password: "master-password",
    });
    expect(ports[0].postMessage).toHaveBeenCalledTimes(1);
    expect(ports[0].postMessage.mock.calls[0][0]).toMatchObject({
      type: "ping",
      protocolVersion: 3,
    });
    ports[0].onMessage.emit(
      correlatedResponse(ports[0], {
        type: "pong",
        app: "termkey",
        version: "1.0.0",
        ...protocolInfo,
      })
    );
    expect(ports[0].postMessage).toHaveBeenCalledTimes(2);
    expect(ports[0].postMessage.mock.calls[1][0]).toMatchObject({
      type: "unlock",
      password: "master-password",
    });
    ports[0].onMessage.emit(
      correlatedResponse(
        ports[0],
        { type: "unlock", unlocked: true },
        1
      )
    );
    await expect(unlock).resolves.toMatchObject({ ok: true });

    ports[0].onDisconnect.emit();
    const generate = client.request({ type: "generate_password" });
    expect(ports).toHaveLength(2);
    expect(ports[1].postMessage.mock.calls[0][0]).toMatchObject({
      type: "ping",
      protocolVersion: 3,
    });
    ports[1].onMessage.emit(
      correlatedResponse(ports[1], {
        type: "pong",
        app: "termkey",
        version: "1.0.0",
        ...protocolInfo,
      })
    );
    expect(ports[1].postMessage.mock.calls[1][0]).toMatchObject({
      type: "generate_password",
    });
    ports[1].onMessage.emit(
      correlatedResponse(
        ports[1],
        { type: "generated_password", password: "generated" },
        1
      )
    );
    await expect(generate).resolves.toMatchObject({ ok: true });
  });

  it("rejects a native response without a syntactically valid requestId", async () => {
    const port = new MockNativePort();
    const client = new NativeHostClient(
      () => port,
      undefined,
      () => "e".repeat(64)
    );
    const response = client.request({ type: "status" });
    port.onMessage.emit(statusResponse);

    await expect(response).resolves.toEqual({
      ok: false,
      error: "Native host returned an invalid response.",
    });
    expect(port.disconnect).toHaveBeenCalledTimes(1);
  });

  it.each([
    [
      "missing protocol metadata",
      { type: "pong", app: "termkey", version: "0.1.0" },
    ],
    [
      "a different protocol version",
      {
        type: "pong",
        app: "termkey",
        version: "1.0.0",
        ...protocolInfo,
        protocolVersion: 1,
      },
    ],
    [
      "a missing required capability",
      {
        type: "pong",
        app: "termkey",
        version: "1.0.0",
        protocolVersion: 3,
        capabilities: ["opaque-match-handles"],
      },
    ],
  ])("turns %s into an actionable repair error", async (_caseName, pong) => {
    const port = new MockNativePort(false);
    const client = new NativeHostClient(
      () => port,
      undefined,
      () => "f".repeat(64)
    );
    const response = client.request({ type: "ping", protocolVersion: 3 });
    port.onMessage.emit(correlatedResponse(port, pong));

    await expect(response).resolves.toEqual({
      ok: false,
      error:
        "TermKey browser integration is out of date. Run `termkey browser repair`.",
    });
    expect(port.disconnect).toHaveBeenCalledTimes(1);
  });

  it("disconnects and fails queued work when postMessage throws", async () => {
    const ports: MockNativePort[] = [];
    let queued: ReturnType<NativeHostClient["request"]> | undefined;
    let client: NativeHostClient;
    client = new NativeHostClient(() => {
      const port = new MockNativePort();
      ports.push(port);
      if (ports.length === 1) {
        port.postMessage.mockImplementationOnce(() => {
          queued = client.request({ type: "ping", protocolVersion: 3 });
          throw new Error("post failed");
        });
      }
      return port;
    });

    const first = client.request({ type: "status" });
    await expect(first).resolves.toEqual({
      ok: false,
      error: "post failed",
    });
    await expect(queued).resolves.toEqual({
      ok: false,
      error: "post failed",
    });
    expect(ports[0].disconnect).toHaveBeenCalledTimes(1);

    const later = client.request({ type: "ping", protocolVersion: 3 });
    ports[1].onMessage.emit(correlatedResponse(ports[1], {
      type: "pong",
      app: "termkey",
      version: "1.0.0",
      ...protocolInfo,
    }));
    await expect(later).resolves.toMatchObject({ ok: true });
  });

  it("disconnects and fails queued work when listener registration throws", async () => {
    const ports: MockNativePort[] = [];
    let queued: ReturnType<NativeHostClient["request"]> | undefined;
    let client: NativeHostClient;
    client = new NativeHostClient(() => {
      const port = new MockNativePort();
      ports.push(port);
      if (ports.length === 1) {
        port.onMessage.addListener.mockImplementationOnce(() => {
          queued = client.request({ type: "ping", protocolVersion: 3 });
          throw new Error("listener failed");
        });
      }
      return port;
    });

    const first = client.request({ type: "status" });
    await expect(first).resolves.toEqual({
      ok: false,
      error: "listener failed",
    });
    await expect(queued).resolves.toEqual({
      ok: false,
      error: "listener failed",
    });
    expect(ports[0].disconnect).toHaveBeenCalledTimes(1);

    const later = client.request({ type: "ping", protocolVersion: 3 });
    ports[1].onMessage.emit(correlatedResponse(ports[1], {
      type: "pong",
      app: "termkey",
      version: "1.0.0",
      ...protocolInfo,
    }));
    await expect(later).resolves.toMatchObject({ ok: true });
  });

  it("handles a synchronous valid native response", async () => {
    const port = new MockNativePort();
    port.disconnect.mockImplementation(() => {
      port.onDisconnect.emit();
    });
    port.postMessage.mockImplementation((request) => {
      if ((request as { type?: string }).type === "ping") {
        port.onMessage.emit({
          type: "pong",
          app: "termkey",
          version: "1.0.0",
          requestId: (request as { requestId: string }).requestId,
          ...protocolInfo,
        });
        return;
      }
      port.onMessage.emit({
        ...statusResponse,
        requestId: (request as { requestId: string }).requestId,
      });
    });
    const client = new NativeHostClient(() => port);

    await expect(client.request({ type: "status" })).resolves.toMatchObject({
      ok: true,
    });
    expect(port.disconnect).not.toHaveBeenCalled();
  });

  it("rejects a duplicate response while the next request is active", async () => {
    const ports: MockNativePort[] = [];
    const requestIds = [
      "a".repeat(64),
      "b".repeat(64),
      "c".repeat(64),
      "d".repeat(64),
      "e".repeat(64),
      "f".repeat(64),
    ];
    const client = new NativeHostClient(() => {
      const port = new MockNativePort();
      ports.push(port);
      return port;
    }, undefined, () => requestIds.shift()!);
    const first = client.request({ type: "status" });
    const second = client.request({ type: "status" });

    ports[0].onMessage.emit(correlatedResponse(ports[0], statusResponse, 1));
    await expect(first).resolves.toMatchObject({ ok: true });
    await Promise.resolve();
    expect(ports).toHaveLength(1);
    expect(ports[0].postMessage).toHaveBeenCalledTimes(3);

    ports[0].onMessage.emit(correlatedResponse(ports[0], statusResponse, 1));
    await expect(second).resolves.toEqual({
      ok: false,
      error: "Native host returned a mismatched request ID.",
    });
    expect(ports[0].disconnect).toHaveBeenCalledTimes(1);

    const later = client.request({ type: "status" });
    expect(ports).toHaveLength(2);
    ports[1].onMessage.emit(correlatedResponse(ports[1], statusResponse));
    await expect(later).resolves.toMatchObject({ ok: true });
  });

  it("resets on an unsolicited response and recovers with a new request", async () => {
    const ports: MockNativePort[] = [];
    const client = new NativeHostClient(() => {
      const port = new MockNativePort();
      ports.push(port);
      return port;
    });
    const first = client.request({ type: "status" });
    ports[0].onMessage.emit(correlatedResponse(ports[0], statusResponse));
    await expect(first).resolves.toMatchObject({ ok: true });
    await Promise.resolve();

    ports[0].onMessage.emit(correlatedResponse(ports[0], statusResponse));
    expect(ports[0].disconnect).toHaveBeenCalledTimes(1);
    const later = client.request({ type: "status" });
    expect(ports).toHaveLength(2);
    ports[1].onMessage.emit(correlatedResponse(ports[1], statusResponse));
    await expect(later).resolves.toMatchObject({ ok: true });
  });

  it("fails current and queued work on disconnect and reconnects later", async () => {
    const ports: MockNativePort[] = [];
    const client = new NativeHostClient(() => {
      const port = new MockNativePort();
      ports.push(port);
      return port;
    });
    const current = client.request({ type: "status" });
    const queued = client.request({ type: "ping", protocolVersion: 3 });
    expect(ports[0].postMessage).toHaveBeenCalledTimes(2);

    ports[0].onDisconnect.emit();
    await expect(current).resolves.toMatchObject({ ok: false });
    await expect(queued).resolves.toMatchObject({ ok: false });

    const later = client.request({ type: "ping", protocolVersion: 3 });
    expect(ports).toHaveLength(2);
    ports[1].onMessage.emit(correlatedResponse(ports[1], {
      type: "pong",
      app: "termkey",
      version: "1.0.0",
      ...protocolInfo,
    }));
    await expect(later).resolves.toMatchObject({ ok: true });
  });

  it("rejects malformed native response objects", async () => {
    const ports: MockNativePort[] = [];
    const client = new NativeHostClient(() => {
      const port = new MockNativePort();
      ports.push(port);
      return port;
    });
    const response = client.request({
      type: "find_site_matches",
      url: "https://example.test",
    });
    const queued = client.request({ type: "status" });
    ports[0].onMessage.emit(correlatedResponse(ports[0], {
      ...siteMatchesResponse,
      matches: [{ ...siteMatchesResponse.matches[0], hasSecondaryPassword: "no" }],
    }));

    await expect(response).resolves.toEqual({
      ok: false,
      error: "Native host returned an invalid response.",
    });
    await expect(queued).resolves.toEqual({
      ok: false,
      error: "Native host returned an invalid response.",
    });
    expect(ports[0].disconnect).toHaveBeenCalledTimes(1);

    const later = client.request({ type: "status" });
    expect(ports).toHaveLength(2);
    ports[1].onMessage.emit(correlatedResponse(ports[1], {
      type: "status",
      app: "termkey",
      version: "1.0.0",
      vaultPath: "/vault",
      vaultExists: true,
      firstRunComplete: true,
      recoveryConfigured: true,
      locked: false,
      ...protocolInfo,
    }));
    await expect(later).resolves.toMatchObject({ ok: true });
  });

  it("rejects a native response whose type does not match its request", async () => {
    const ports: MockNativePort[] = [];
    const client = new NativeHostClient(() => {
      const port = new MockNativePort();
      ports.push(port);
      return port;
    });
    const response = client.request({ type: "status" });
    const queued = client.request({ type: "ping", protocolVersion: 3 });
    ports[0].onMessage.emit(correlatedResponse(ports[0], {
      type: "pong",
      app: "termkey",
      version: "1.0.0",
      ...protocolInfo,
    }));

    await expect(response).resolves.toEqual({
      ok: false,
      error: "Native host returned the wrong response type.",
    });
    await expect(queued).resolves.toEqual({
      ok: false,
      error: "Native host returned the wrong response type.",
    });
    expect(ports[0].disconnect).toHaveBeenCalledTimes(1);

    const later = client.request({ type: "ping", protocolVersion: 3 });
    expect(ports).toHaveLength(2);
    ports[1].onMessage.emit(correlatedResponse(ports[1], {
      type: "pong",
      app: "termkey",
      version: "1.0.0",
      ...protocolInfo,
    }));
    await expect(later).resolves.toMatchObject({ ok: true });
  });

  it("rejects privileged popup messages from untrusted senders", async () => {
    const mock = createChromeMock();
    const service = createBackgroundService(mock.chrome);

    await expect(
      service.handleMessage(
        { type: "termkey.nativeHost.status" },
        { id: "extension-id", url: "https://example.test/" }
      )
    ).resolves.toEqual({
      ok: false,
      error: "Unauthorized extension message sender.",
    });
    await expect(
      service.handleMessage(
        { type: "termkey.nativeHost.status" },
        {
          id: "different-extension",
          url: "chrome-extension://extension-id/popup.html",
        }
      )
    ).resolves.toMatchObject({ ok: false });
    await expect(
      service.handleMessage(
        { type: "termkey.nativeHost.status" },
        {
          id: "extension-id",
          url: "chrome-extension://another-extension/popup.html",
        }
      )
    ).resolves.toMatchObject({ ok: false });
    expect(mock.chrome.runtime.connectNative).not.toHaveBeenCalled();
  });
});
