import { vi } from "vitest";

type Listener<TArgs extends unknown[]> = (...args: TArgs) => unknown;

export class MockChromeEvent<TArgs extends unknown[]> {
  private readonly listeners: Listener<TArgs>[] = [];

  addListener = vi.fn((listener: Listener<TArgs>) => {
    this.listeners.push(listener);
  });

  emit(...args: TArgs) {
    for (const listener of [...this.listeners]) {
      listener(...args);
    }
  }

  async emitAsync(...args: TArgs) {
    await Promise.all(this.listeners.map((listener) => listener(...args)));
  }

  hasListeners() {
    return this.listeners.length > 0;
  }
}

export class MockNativePort {
  readonly onMessage = new MockChromeEvent<[unknown]>();
  readonly onDisconnect = new MockChromeEvent<[]>();
  readonly postMessage = vi.fn<(message: unknown) => void>();
  readonly disconnect = vi.fn();

  constructor(autoNegotiate = true) {
    if (autoNegotiate) {
      this.postMessage.mockImplementation((message) => {
        const request = message as {
          type?: string;
          protocolVersion?: number;
          requestId?: string;
        };
        if (request.type === "ping" && request.protocolVersion === 3) {
          this.onMessage.emit({
            type: "pong",
            app: "termkey",
            version: "1.0.0",
            protocolVersion: 3,
            capabilities: [
              "opaque-match-handles",
              "document-token-binding",
              "origin-only-save",
              "password-entry-update",
              "bounded-native-output",
            ],
            requestId: request.requestId,
          });
        }
      });
    }
  }
}

export type MockTab = {
  id?: number;
  url?: string;
};

export function createChromeMock(initialTab: MockTab = {
  id: 7,
  url: "https://example.test/login",
}) {
  let activeTab = { ...initialTab };
  const tabsById = new Map<number, MockTab>();
  if (typeof initialTab.id === "number") {
    tabsById.set(initialTab.id, { ...initialTab });
  }
  let tabMessageHandler:
    | ((tabId: number, message: unknown, options: { frameId: number }) => unknown)
    | undefined;
  let documentToken = "a".repeat(64);
  let submittedLogin:
    | { ok: true; username: string | null; password: string }
    | { ok: false; error: string }
    | undefined;
  let pageContext:
    | {
        intent: "login" | "signup" | "password_change" | "unknown";
        hasPasswordField: boolean;
        hasVisibleLoginFailure: boolean;
      }
    | undefined;
  let nativeResponder:
    | ((request: unknown, port: MockNativePort) => unknown | undefined)
    | undefined;
  const ports: MockNativePort[] = [];

  const runtimeOnMessage =
    new MockChromeEvent<
      [
        unknown,
        {
          id?: string;
          url?: string;
          tab?: { id?: number; url?: string };
          frameId?: number;
        },
        (response: unknown) => void,
      ]
    >();

  const chrome = {
    runtime: {
      id: "extension-id",
      lastError: undefined as { message?: string } | undefined,
      getURL: (path = "") =>
        `chrome-extension://extension-id/${path.replace(/^\/+/, "")}`,
      connectNative: vi.fn(() => {
        const port = new MockNativePort();
        ports.push(port);
        port.postMessage.mockImplementation((request) => {
          const typed = request as {
            type?: string;
            protocolVersion?: number;
          };
          const response =
            typed.type === "ping" && typed.protocolVersion === 3
              ? {
                  type: "pong",
                  app: "termkey",
                  version: "1.0.0",
                  protocolVersion: 3,
                  capabilities: [
                    "opaque-match-handles",
                    "document-token-binding",
                    "origin-only-save",
                    "password-entry-update",
                    "bounded-native-output",
                  ],
                }
              : nativeResponder?.(request, port);
          if (response !== undefined) {
            const correlatedResponse =
              typeof response === "object" &&
              response !== null &&
              !("requestId" in response)
                ? {
                    ...response,
                    requestId: (request as { requestId?: string }).requestId,
                  }
                : response;
            queueMicrotask(() => port.onMessage.emit(correlatedResponse));
          }
        });
        return port;
      }),
      onMessage: runtimeOnMessage,
      onInstalled: new MockChromeEvent<[]>(),
      onStartup: new MockChromeEvent<[]>(),
    },
    tabs: {
      query: vi.fn(async () => [{ ...activeTab }]),
      get: vi.fn(async (tabId: number) => {
        const tab = tabsById.get(tabId);
        if (!tab) {
          throw new Error("Tab not found.");
        }
        return { ...tab };
      }),
      sendMessage: vi.fn(
        async (
          tabId: number,
          message: unknown,
          options: { frameId: number }
        ) => {
          if (tabMessageHandler) {
            return tabMessageHandler(tabId, message, options);
          }
          const type = (message as { type?: string }).type;
          if (type === "termkey.contentScriptProbe") {
            return { ok: true, documentToken };
          }
          if (type === "termkey.captureSubmittedLogin" && submittedLogin) {
            return submittedLogin;
          }
          if (type === "termkey.inspectPageContext" && pageContext) {
            return {
              ok: true,
              documentToken,
              ...pageContext,
            };
          }
          return undefined;
        }
      ),
      onRemoved: new MockChromeEvent<[number]>(),
      onUpdated: new MockChromeEvent<
        [
          number,
          { status?: string; url?: string },
          { id?: number; url?: string },
        ]
      >(),
    },
    action: {
      openPopup: vi.fn(async () => undefined),
    },
    scripting: {
      executeScript: vi.fn(async () => undefined),
    },
    storage: {
      session: {
        get: vi.fn(
          (
            _keys: string[],
            callback: (result: Record<string, unknown>) => void
          ) => callback({})
        ),
        set: vi.fn(
          (_value: Record<string, unknown>, callback: () => void) => callback()
        ),
        remove: vi.fn((_key: string, callback: () => void) => callback()),
      },
    },
  };

  return {
    chrome,
    ports,
    runtimeOnMessage,
    extensionSender: {
      id: "extension-id",
      url: "chrome-extension://extension-id/popup.html",
    },
    promptSender(tabId = 7) {
      const tab = tabsById.get(tabId);
      return {
        id: "extension-id",
        url: "chrome-extension://extension-id/prompt.html",
        tab: tab ? { ...tab } : { id: tabId },
        frameId: 1,
      };
    },
    setActiveTab(tab: MockTab) {
      activeTab = { ...tab };
      if (typeof tab.id === "number") {
        tabsById.set(tab.id, { ...tab });
      }
    },
    setTab(tab: MockTab) {
      if (typeof tab.id === "number") {
        tabsById.set(tab.id, { ...tab });
        if (activeTab.id === tab.id) {
          activeTab = { ...tab };
        }
      }
    },
    setDocumentToken(token: string) {
      documentToken = token;
    },
    setSubmittedLogin(
      response:
        | { ok: true; username: string | null; password: string }
        | { ok: false; error: string }
    ) {
      submittedLogin = response;
    },
    setPageContext(context: {
      intent: "login" | "signup" | "password_change" | "unknown";
      hasPasswordField: boolean;
      hasVisibleLoginFailure: boolean;
    }) {
      pageContext = context;
    },
    dispatchContentMessage(message: unknown, tabId: number) {
      const tab = tabsById.get(tabId);
      if (!runtimeOnMessage.hasListeners()) {
        return Promise.resolve(undefined);
      }
      return new Promise<unknown>((resolve) => {
        runtimeOnMessage.emit(
          message,
          {
            id: "extension-id",
            url: tab?.url,
            tab: tab ? { ...tab } : { id: tabId },
            frameId: 0,
          },
          resolve
        );
      });
    },
    async dispatchTabUpdated(
      tabId: number,
      changeInfo: { status?: string; url?: string }
    ) {
      const current = tabsById.get(tabId) ?? { id: tabId };
      const updated = {
        ...current,
        ...(changeInfo.url === undefined ? {} : { url: changeInfo.url }),
      };
      tabsById.set(tabId, updated);
      if (activeTab.id === tabId) {
        activeTab = { ...updated };
      }
      await chrome.tabs.onUpdated.emitAsync(tabId, changeInfo, {
        ...updated,
      });
    },
    async dispatchTabRemoved(tabId: number) {
      tabsById.delete(tabId);
      await chrome.tabs.onRemoved.emitAsync(tabId);
    },
    setTabMessageHandler(
      handler: (
        tabId: number,
        message: unknown,
        options: { frameId: number }
      ) => unknown
    ) {
      tabMessageHandler = handler;
    },
    setNativeResponder(
      responder: (request: unknown, port: MockNativePort) => unknown | undefined
    ) {
      nativeResponder = responder;
    },
  };
}
