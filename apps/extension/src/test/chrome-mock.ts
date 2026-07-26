import { vi } from "vitest";

type Listener<TArgs extends unknown[]> = (...args: TArgs) => void;

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
        if (request.type === "ping" && request.protocolVersion === 2) {
          this.onMessage.emit({
            type: "pong",
            app: "termkey",
            version: "1.0.0",
            protocolVersion: 2,
            capabilities: [
              "opaque-match-handles",
              "document-token-binding",
              "origin-only-save",
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
  let nativeResponder:
    | ((request: unknown, port: MockNativePort) => unknown | undefined)
    | undefined;
  const ports: MockNativePort[] = [];

  const runtimeOnMessage =
    new MockChromeEvent<
      [
        unknown,
        { id?: string; url?: string },
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
            typed.type === "ping" && typed.protocolVersion === 2
              ? {
                  type: "pong",
                  app: "termkey",
                  version: "1.0.0",
                  protocolVersion: 2,
                  capabilities: [
                    "opaque-match-handles",
                    "document-token-binding",
                    "origin-only-save",
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
        ) => tabMessageHandler?.(tabId, message, options)
      ),
      onRemoved: new MockChromeEvent<[number]>(),
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
    setActiveTab(tab: MockTab) {
      activeTab = { ...tab };
      if (typeof tab.id === "number") {
        tabsById.set(tab.id, { ...tab });
      }
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
