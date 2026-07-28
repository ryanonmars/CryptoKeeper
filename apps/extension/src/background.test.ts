import { beforeEach, describe, expect, it, vi } from "vitest";

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
  protocolVersion: 2,
  capabilities: [
    "opaque-match-handles",
    "document-token-binding",
    "origin-only-save",
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

  it("stores only an HTTPS origin without path query or fragment", async () => {
    const mock = createChromeMock({
      id: 7,
      url: "https://EXAMPLE.test:443/account/login?next=%2Fvault#password",
    });
    mock.setTabMessageHandler((_tabId, message) => {
      const type = (message as { type?: string }).type;
      if (type === "termkey.contentScriptProbe") {
        return { ok: true, documentToken: "a".repeat(64) };
      }
      if (type === "termkey.captureVisibleCredentials") {
        return {
          ok: true,
          captureState: "complete",
          username: "person@example.test",
          password: "captured-secret",
        };
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
        { type: "termkey.content.captureVisibleCredentials" },
        mock.extensionSender
      )
    ).resolves.toMatchObject({
      ok: true,
      response: {
        type: "captured_login",
        candidate: {
          username: "person@example.test",
          password: "captured-secret",
          url: "https://example.test",
        },
      },
    });
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
    const queued = client.request({ type: "ping", protocolVersion: 2 });
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
      protocolVersion: 2,
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
      protocolVersion: 2,
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
        protocolVersion: 2,
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
    const response = client.request({ type: "ping", protocolVersion: 2 });
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
          queued = client.request({ type: "ping", protocolVersion: 2 });
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

    const later = client.request({ type: "ping", protocolVersion: 2 });
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
          queued = client.request({ type: "ping", protocolVersion: 2 });
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

    const later = client.request({ type: "ping", protocolVersion: 2 });
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
    const queued = client.request({ type: "ping", protocolVersion: 2 });
    expect(ports[0].postMessage).toHaveBeenCalledTimes(2);

    ports[0].onDisconnect.emit();
    await expect(current).resolves.toMatchObject({ ok: false });
    await expect(queued).resolves.toMatchObject({ ok: false });

    const later = client.request({ type: "ping", protocolVersion: 2 });
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
    const queued = client.request({ type: "ping", protocolVersion: 2 });
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

    const later = client.request({ type: "ping", protocolVersion: 2 });
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
