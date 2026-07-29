import { coreReady } from "@termkey/core";
import type {
  NativeHostRequest,
  NativeHostResponse,
  NativeHostWireRequest,
  PopupPageIntent,
  PopupToBackgroundMessage,
  PopupToBackgroundResponse,
} from "@termkey/types";

const NATIVE_HOST_NAME = "com.ryanonmars.termkey";
const MATCH_GRANT_TTL_MS = 30_000;
const PENDING_LOGIN_TTL_MS = 120_000;
const MAX_MATCH_GRANTS = 100;
export const NATIVE_PROTOCOL_VERSION = 3;
export const REQUIRED_NATIVE_CAPABILITIES = [
  "opaque-match-handles",
  "document-token-binding",
  "origin-only-save",
  "password-entry-update",
  "bounded-native-output",
] as const;
const PROTOCOL_REPAIR_ERROR =
  "TermKey browser integration is out of date. Run `termkey browser repair`.";

export const NATIVE_REQUEST_TIMEOUT_MS = 10_000;

type NativePort = {
  onMessage: {
    addListener(listener: (message: unknown) => void): void;
  };
  onDisconnect: {
    addListener(listener: () => void): void;
  };
  postMessage(message: NativeHostWireRequest): void;
  disconnect(): void;
};

type NativeQueueEntry = {
  request: NativeHostRequest;
  requestId: string;
  resolve: (response: NativeClientResponse) => void;
};

type NativeClientResponse =
  | { ok: true; response: NativeHostResponse }
  | { ok: false; error: string };

type NativeResponseForRequest<TRequest extends NativeHostRequest> =
  TRequest["type"] extends "ping"
    ? Extract<NativeHostResponse, { type: "pong" }>
    : TRequest["type"] extends "status"
      ? Extract<NativeHostResponse, { type: "status" }>
      : TRequest["type"] extends "get_autofill_entry"
        ? Extract<NativeHostResponse, { type: "autofill_entry" }>
        : TRequest["type"] extends "find_site_matches"
          ? Extract<NativeHostResponse, { type: "site_matches" }>
          : TRequest["type"] extends "generate_password"
            ? Extract<NativeHostResponse, { type: "generated_password" }>
            : TRequest["type"] extends
                  | "save_password_entry"
                  | "update_password_entry"
              ? Extract<NativeHostResponse, { type: "save_entry" }>
              : TRequest["type"] extends "list_entries"
                ? Extract<NativeHostResponse, { type: "list_entries" }>
                : Extract<NativeHostResponse, { type: "unlock" }>;

type NativeClientResponseFor<TRequest extends NativeHostRequest> =
  | { ok: true; response: NativeResponseForRequest<TRequest> }
  | { ok: false; error: string };

type PendingLogin = {
  tabId: number;
  sourceDocumentToken: string;
  origin: string;
  username: string | null;
  password: string;
  updateEntryId?: string;
  expiresAt: number;
  ready: boolean;
};

export type MatchGrant = {
  tabId: number;
  documentToken: string;
  origin: string;
  entryId: string;
  expiresAt: number;
};

type MessageSender = {
  id?: string;
  url?: string;
  tab?: { id?: number; url?: string };
  frameId?: number;
};

type BackgroundChrome = {
  runtime: {
    id: string;
    lastError?: { message?: string };
    getURL(path?: string): string;
    connectNative(name: string): NativePort;
    onMessage?: {
      addListener(
        listener: (
          message: unknown,
          sender: MessageSender,
          sendResponse: (response: unknown) => void
        ) => boolean
      ): void;
    };
  };
  tabs: {
    query(query: { active: true; currentWindow: true }): Promise<
      Array<{ id?: number; url?: string }>
    >;
    get(tabId: number): Promise<{ id?: number; url?: string }>;
    sendMessage(
      tabId: number,
      message: unknown,
      options: { frameId: 0 }
    ): Promise<unknown>;
    onRemoved?: {
      addListener(listener: (tabId: number) => void): void;
    };
    onUpdated?: {
      addListener(
        listener: (
          tabId: number,
          changeInfo: { status?: string; url?: string },
          tab: { id?: number; url?: string }
        ) => void
      ): void;
    };
  };
  scripting: {
    executeScript(details: {
      target: { tabId: number; frameIds: [0] };
      files: ["dist/content.js"];
    }): Promise<unknown>;
  };
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isStringOrNull(value: unknown): value is string | null {
  return typeof value === "string" || value === null;
}

function hasString(value: Record<string, unknown>, key: string) {
  return typeof value[key] === "string";
}

function hasCompatibleProtocolInfo(value: Record<string, unknown>) {
  const capabilities = value.capabilities;
  return (
    value.protocolVersion === NATIVE_PROTOCOL_VERSION &&
    Array.isArray(capabilities) &&
    capabilities.every((capability) => typeof capability === "string") &&
    REQUIRED_NATIVE_CAPABILITIES.every((required) =>
      capabilities.includes(required)
    )
  );
}

function isEntrySummary(value: unknown) {
  if (!isRecord(value)) {
    return false;
  }

  return (
    hasString(value, "id") &&
    hasString(value, "name") &&
    hasString(value, "secretType") &&
    hasString(value, "network") &&
    typeof value.hasSecondaryPassword === "boolean" &&
    isStringOrNull(value.publicAddress) &&
    isStringOrNull(value.username) &&
    isStringOrNull(value.url)
  );
}

function isSiteMatch(value: unknown) {
  if (!isRecord(value)) {
    return false;
  }

  return (
    hasString(value, "id") &&
    hasString(value, "name") &&
    isStringOrNull(value.username) &&
    (value.matchType === "exact_origin" ||
      value.matchType === "exact_host" ||
      value.matchType === "subdomain" ||
      value.matchType === "registrable_domain") &&
    typeof value.hasSecondaryPassword === "boolean"
  );
}

function isAutofillEntry(value: unknown) {
  if (!isRecord(value)) {
    return false;
  }

  return (
    hasString(value, "id") &&
    hasString(value, "name") &&
    isStringOrNull(value.username) &&
    hasString(value, "password")
  );
}

export function isNativeHostResponse(value: unknown): value is NativeHostResponse {
  if (
    !isRecord(value) ||
    typeof value.type !== "string" ||
    typeof value.requestId !== "string" ||
    !/^[a-f0-9]{64}$/.test(value.requestId)
  ) {
    return false;
  }

  switch (value.type) {
    case "pong":
      return (
        value.app === "termkey" &&
        hasString(value, "version") &&
        hasCompatibleProtocolInfo(value)
      );
    case "status":
      return (
        value.app === "termkey" &&
        hasString(value, "version") &&
        hasString(value, "vaultPath") &&
        typeof value.vaultExists === "boolean" &&
        typeof value.firstRunComplete === "boolean" &&
        typeof value.recoveryConfigured === "boolean" &&
        typeof value.locked === "boolean" &&
        hasCompatibleProtocolInfo(value)
      );
    case "autofill_entry":
      return isAutofillEntry(value.entry);
    case "generated_password":
      return hasString(value, "password");
    case "save_entry":
      return hasString(value, "entryName");
    case "site_matches":
      return (
        hasString(value, "siteUrl") &&
        hasString(value, "siteOrigin") &&
        hasString(value, "siteHostname") &&
        Array.isArray(value.matches) &&
        value.matches.every(isSiteMatch)
      );
    case "list_entries":
      return (
        Array.isArray(value.entries) && value.entries.every(isEntrySummary)
      );
    case "unlock":
      return (
        value.unlocked === true &&
        (value.recoveryNotice === undefined ||
          typeof value.recoveryNotice === "string")
      );
    case "error":
      return hasString(value, "message");
    default:
      return false;
  }
}

function expectedResponseType(
  request: NativeHostRequest
): Exclude<NativeHostResponse["type"], "error"> {
  switch (request.type) {
    case "ping":
      return "pong";
    case "status":
      return "status";
    case "get_autofill_entry":
      return "autofill_entry";
    case "find_site_matches":
      return "site_matches";
    case "generate_password":
      return "generated_password";
    case "save_password_entry":
    case "update_password_entry":
      return "save_entry";
    case "list_entries":
      return "list_entries";
    case "unlock":
      return "unlock";
  }
}

function generateNativeRequestId() {
  return Array.from(
    crypto.getRandomValues(new Uint8Array(32)),
    (byte) => byte.toString(16).padStart(2, "0")
  ).join("");
}

export class NativeHostClient {
  private port: NativePort | undefined;
  private current: NativeQueueEntry | undefined;
  private readonly queue: NativeQueueEntry[] = [];
  private timeoutId: ReturnType<typeof setTimeout> | undefined;
  private pumpScheduled = false;
  private protocolNegotiated = false;
  private handshakeRequestId: string | undefined;

  constructor(
    private readonly connectNative: () => NativePort,
    private readonly disconnectError: () => string = () =>
      "Native host disconnected. Please try again.",
    private readonly generateRequestId: () => string = generateNativeRequestId
  ) {}

  request<TRequest extends NativeHostRequest>(
    request: TRequest
  ): Promise<NativeClientResponseFor<TRequest>> {
    const requestId = this.generateRequestId();
    if (!/^[a-f0-9]{64}$/.test(requestId)) {
      return Promise.resolve({
        ok: false,
        error: "Could not generate a valid native request ID.",
      });
    }
    return new Promise((resolve) => {
      this.queue.push({
        request,
        requestId,
        resolve: resolve as (response: NativeClientResponse) => void,
      });
      this.pump();
    });
  }

  private getPort() {
    if (this.port) {
      return this.port;
    }

    const port = this.connectNative();
    this.port = port;
    port.onMessage.addListener((message) => {
      if (this.port === port) {
        this.handleMessage(message);
      }
    });
    port.onDisconnect.addListener(() => {
      if (this.port === port) {
        this.failAll(this.disconnectError(), false);
      }
    });
    return port;
  }

  private pump() {
    if (this.current || this.queue.length === 0) {
      return;
    }

    const entry = this.queue.shift();
    if (!entry) {
      return;
    }

    this.current = entry;
    try {
      const port = this.getPort();
      this.startTimeout();
      if (this.protocolNegotiated) {
        this.postCurrent(port);
      } else {
        const handshakeRequestId = this.generateRequestId();
        if (!/^[a-f0-9]{64}$/.test(handshakeRequestId)) {
          this.failAll(
            "Could not generate a valid native handshake request ID.",
            true
          );
          return;
        }
        this.handshakeRequestId = handshakeRequestId;
        port.postMessage({
          type: "ping",
          protocolVersion: NATIVE_PROTOCOL_VERSION,
          requestId: handshakeRequestId,
        });
      }
    } catch (error) {
      this.failAll(
        error instanceof Error
          ? error.message
          : "Could not connect to the native host.",
        true
      );
    }
  }

  private handleMessage(message: unknown) {
    if (!this.current) {
      this.failAll("Native host returned an unsolicited response.", true);
      return;
    }
    const entry = this.current;

    if (this.handshakeRequestId) {
      if (!isNativeHostResponse(message)) {
        this.failAll(
          isRecord(message) && message.type === "pong"
            ? PROTOCOL_REPAIR_ERROR
            : "Native host returned an invalid handshake response.",
          true
        );
        return;
      }
      if (message.requestId !== this.handshakeRequestId) {
        this.failAll("Native host returned a mismatched handshake request ID.", true);
        return;
      }
      if (message.type === "error") {
        this.failAll(
          message.message.includes("termkey browser repair")
            ? message.message
            : PROTOCOL_REPAIR_ERROR,
          true
        );
        return;
      }
      if (message.type !== "pong") {
        this.failAll(PROTOCOL_REPAIR_ERROR, true);
        return;
      }

      this.handshakeRequestId = undefined;
      this.protocolNegotiated = true;
      this.clearCurrentTimeout();
      this.startTimeout();
      try {
        const port = this.port;
        if (!port) {
          this.failAll("Native host disconnected during negotiation.", false);
          return;
        }
        this.postCurrent(port);
      } catch (error) {
        this.failAll(
          error instanceof Error
            ? error.message
            : "Could not send a request to the native host.",
          true
        );
      }
      return;
    }

    if (!isNativeHostResponse(message)) {
      this.failAll(
        isRecord(message) &&
          (message.type === "pong" || message.type === "status") &&
          typeof message.requestId === "string" &&
          /^[a-f0-9]{64}$/.test(message.requestId)
          ? PROTOCOL_REPAIR_ERROR
          : "Native host returned an invalid response.",
        true
      );
      return;
    }

    if (message.requestId !== entry.requestId) {
      this.failAll("Native host returned a mismatched request ID.", true);
      return;
    }

    if (message.type !== "error" && message.type !== expectedResponseType(entry.request)) {
      this.failAll("Native host returned the wrong response type.", true);
      return;
    }

    if (message.type === "error") {
      this.finishCurrent({ ok: false, error: message.message });
      return;
    }

    this.finishCurrent({ ok: true, response: message });
  }

  private clearCurrentTimeout() {
    if (this.timeoutId !== undefined) {
      clearTimeout(this.timeoutId);
      this.timeoutId = undefined;
    }
  }

  private startTimeout() {
    this.clearCurrentTimeout();
    this.timeoutId = setTimeout(() => {
      this.failAll(
        "Native host timed out after 10 seconds. Please try again.",
        true
      );
    }, NATIVE_REQUEST_TIMEOUT_MS);
  }

  private postCurrent(port: NativePort) {
    const current = this.current;
    if (!current) {
      throw new Error("Native request state was lost.");
    }
    port.postMessage({
      ...current.request,
      requestId: current.requestId,
    });
  }

  private finishCurrent(response: NativeClientResponse) {
    const current = this.current;
    if (!current) {
      return;
    }
    this.clearCurrentTimeout();
    this.current = undefined;
    current.resolve(response);
    this.schedulePump();
  }

  private schedulePump() {
    if (this.pumpScheduled) {
      return;
    }
    this.pumpScheduled = true;
    queueMicrotask(() => {
      this.pumpScheduled = false;
      this.pump();
    });
  }

  private failAll(error: string, disconnect: boolean) {
    this.clearCurrentTimeout();
    const current = this.current;
    this.current = undefined;
    const port = this.port;
    this.port = undefined;
    this.protocolNegotiated = false;
    this.handshakeRequestId = undefined;
    const entries = [
      ...(current ? [current] : []),
      ...this.queue.splice(0),
    ];

    if (disconnect && port) {
      try {
        port.disconnect();
      } catch {
        // The port is already unusable; all associated work still fails below.
      }
    }

    for (const entry of entries) {
      entry.resolve({ ok: false, error });
    }
  }
}

class MatchGrantStore {
  private readonly grants = new Map<
    string,
    {
      grant: MatchGrant;
      state: "available" | "reserved" | "consumed";
    }
  >();

  constructor(
    private readonly now: () => number,
    private readonly ttlMs: number
  ) {}

  add(
    grantId: string,
    tabId: number,
    documentToken: string,
    origin: string,
    entryId: string
  ) {
    this.prune();
    this.grants.set(grantId, {
      grant: {
        tabId,
        documentToken,
        origin,
        entryId,
        expiresAt: this.now() + this.ttlMs,
      },
      state: "available",
    });
    while (this.grants.size > MAX_MATCH_GRANTS) {
      const oldest = this.grants.keys().next().value as string | undefined;
      if (!oldest) {
        break;
      }
      this.grants.delete(oldest);
    }
    return grantId;
  }

  reserve(grantId: string) {
    this.prune();
    const record = this.grants.get(grantId);
    if (!record || record.state !== "available") {
      return undefined;
    }
    record.state = "reserved";
    return record.grant;
  }

  release(grantId: string) {
    this.prune();
    const record = this.grants.get(grantId);
    if (record?.state === "reserved") {
      record.state = "available";
      return true;
    }
    return false;
  }

  consume(grantId: string) {
    this.prune();
    const record = this.grants.get(grantId);
    if (record?.state !== "reserved") {
      return false;
    }
    record.state = "consumed";
    return true;
  }

  invalidate(grantId: string) {
    this.grants.delete(grantId);
  }

  removeTab(tabId: number) {
    for (const [key, record] of this.grants) {
      if (record.grant.tabId === tabId) {
        this.grants.delete(key);
      }
    }
  }

  private prune() {
    const currentTime = this.now();
    for (const [key, record] of this.grants) {
      if (record.grant.expiresAt <= currentTime) {
        this.grants.delete(key);
      }
    }
  }
}

class PendingLoginStore {
  private readonly candidates = new Map<
    number,
    {
      candidate: PendingLogin;
      deadlineId: symbol;
      timerId: ReturnType<typeof setTimeout>;
    }
  >();

  constructor(
    private readonly now: () => number,
    private readonly ttlMs: number
  ) {}

  add(
    tabId: number,
    sourceDocumentToken: string,
    origin: string,
    username: string | null,
    password: string
  ) {
    const previous = this.candidates.get(tabId);
    if (previous) {
      clearTimeout(previous.timerId);
      previous.candidate.password = "";
    }
    const candidate: PendingLogin = {
      tabId,
      sourceDocumentToken,
      origin,
      username,
      password,
      expiresAt: this.now() + this.ttlMs,
      ready: false,
    };
    const deadlineId = Symbol();
    const timerId = setTimeout(() => {
      const current = this.candidates.get(tabId);
      if (current?.deadlineId === deadlineId) {
        current.candidate.password = "";
        this.candidates.delete(tabId);
      }
    }, this.ttlMs);
    this.candidates.set(tabId, { candidate, deadlineId, timerId });
    return candidate;
  }

  get(tabId: number) {
    const candidate = this.candidates.get(tabId)?.candidate;
    if (candidate && candidate.expiresAt <= this.now()) {
      this.remove(candidate);
      return undefined;
    }
    return candidate;
  }

  markReady(candidate: PendingLogin) {
    if (this.candidates.get(candidate.tabId)?.candidate === candidate) {
      candidate.ready = true;
    }
  }

  removeTab(tabId: number) {
    const current = this.candidates.get(tabId);
    if (current) {
      clearTimeout(current.timerId);
      current.candidate.password = "";
    }
    this.candidates.delete(tabId);
  }

  remove(candidate: PendingLogin) {
    const current = this.candidates.get(candidate.tabId);
    if (current?.candidate === candidate) {
      clearTimeout(current.timerId);
      current.candidate.password = "";
      this.candidates.delete(candidate.tabId);
    }
  }
}

function generateOpaqueGrantId() {
  return Array.from(
    crypto.getRandomValues(new Uint8Array(32)),
    (byte) => byte.toString(16).padStart(2, "0")
  ).join("");
}

export function canonicalizeWebOrigin(url: string) {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    throw new Error("Current tab URL is invalid.");
  }

  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw new Error("Current tab must use HTTP or HTTPS.");
  }
  if (parsed.username !== "" || parsed.password !== "") {
    throw new Error("Current tab URL must not contain user information.");
  }

  return parsed.origin;
}

function isMissingContentScriptError(error: unknown) {
  if (!(error instanceof Error)) {
    return false;
  }

  return (
    error.message.includes("Receiving end does not exist") ||
    error.message.includes("Could not establish connection")
  );
}

function parseDocumentToken(response: unknown) {
  if (
    !isRecord(response) ||
    response.ok !== true ||
    typeof response.documentToken !== "string" ||
    !/^[a-f0-9]{64}$/.test(response.documentToken)
  ) {
    throw new Error("Content script returned an invalid document token.");
  }

  return response.documentToken;
}

function isKnownPopupMessage(
  message: unknown
): message is PopupToBackgroundMessage {
  if (!isRecord(message) || typeof message.type !== "string") {
    return false;
  }

  return (
    message.type === "termkey.nativeHost.ping" ||
    message.type === "termkey.nativeHost.status" ||
    message.type === "termkey.nativeHost.findSiteMatches" ||
    message.type === "termkey.content.inspectPageContext" ||
    message.type === "termkey.pendingLogin.get" ||
    message.type === "termkey.pendingLogin.dismiss" ||
    message.type === "termkey.pendingLogin.save" ||
    message.type === "termkey.passwords.generateForPage" ||
    message.type === "termkey.autofill.fillSelectedMatch" ||
    message.type === "termkey.nativeHost.savePasswordEntry" ||
    message.type === "termkey.nativeHost.unlock"
  );
}

type ContentLifecycleMessage = {
  type:
    | "termkey.content.loginSubmitted"
    | "termkey.content.pageContextChanged";
  documentToken: string;
};

function isContentLifecycleMessage(
  message: unknown
): message is ContentLifecycleMessage {
  return (
    isRecord(message) &&
    (message.type === "termkey.content.loginSubmitted" ||
      message.type === "termkey.content.pageContextChanged") &&
    typeof message.documentToken === "string" &&
    /^[a-f0-9]{64}$/.test(message.documentToken)
  );
}

export function isTrustedExtensionPageSender(
  sender: MessageSender,
  chromeApi: Pick<BackgroundChrome, "runtime">
) {
  if (
    sender.id !== chromeApi.runtime.id ||
    typeof sender.url !== "string"
  ) {
    return false;
  }

  try {
    const senderUrl = new URL(sender.url);
    const extensionRoot = new URL(chromeApi.runtime.getURL("/"));
    return (
      senderUrl.protocol === "chrome-extension:" &&
      senderUrl.host === extensionRoot.host &&
      senderUrl.username === "" &&
      senderUrl.password === ""
    );
  } catch {
    return false;
  }
}

export function createBackgroundService(
  chromeApi: BackgroundChrome,
  options: {
    now?: () => number;
    grantTtlMs?: number;
    nativeClient?: NativeHostClient;
    generateGrantId?: () => string;
  } = {}
) {
  const now = options.now ?? Date.now;
  const grants = new MatchGrantStore(
    now,
    options.grantTtlMs ?? MATCH_GRANT_TTL_MS
  );
  const pendingLogins = new PendingLoginStore(now, PENDING_LOGIN_TTL_MS);
  const nativeClient =
    options.nativeClient ??
    new NativeHostClient(
      () => chromeApi.runtime.connectNative(NATIVE_HOST_NAME),
      () =>
        chromeApi.runtime.lastError?.message ??
        "Native host disconnected. Please try again."
    );
  const contentScriptAttempts = new Map<number, Promise<string>>();
  const generateGrantId = options.generateGrantId ?? generateOpaqueGrantId;

  async function getActiveTabOnce() {
    const tabs = await chromeApi.tabs.query({
      active: true,
      currentWindow: true,
    });
    const tab = tabs[0];
    if (typeof tab?.id !== "number" || typeof tab.url !== "string") {
      throw new Error("No active tab is available.");
    }
    return {
      tabId: tab.id,
      origin: canonicalizeWebOrigin(tab.url),
    };
  }

  async function probeDocument(tabId: number) {
    const response = await chromeApi.tabs.sendMessage(
      tabId,
      { type: "termkey.contentScriptProbe" },
      { frameId: 0 }
    );
    return parseDocumentToken(response);
  }

  async function ensureContentScript(tabId: number) {
    const existingAttempt = contentScriptAttempts.get(tabId);
    if (existingAttempt) {
      return existingAttempt;
    }

    const attempt = (async () => {
      try {
        return await probeDocument(tabId);
      } catch (error) {
        if (!isMissingContentScriptError(error)) {
          throw error;
        }
      }

      await chromeApi.scripting.executeScript({
        target: { tabId, frameIds: [0] },
        files: ["dist/content.js"],
      });
      return probeDocument(tabId);
    })();
    contentScriptAttempts.set(tabId, attempt);
    try {
      return await attempt;
    } finally {
      if (contentScriptAttempts.get(tabId) === attempt) {
        contentScriptAttempts.delete(tabId);
      }
    }
  }

  async function validateGrant(grant: MatchGrant, entryId: string) {
    if (
      grant.entryId !== entryId ||
      grant.expiresAt <= now()
    ) {
      throw new Error("The selected login match expired. Refresh the popup.");
    }

    const tab = await chromeApi.tabs.get(grant.tabId);
    if (tab.id !== grant.tabId || typeof tab.url !== "string") {
      throw new Error("The matched tab is no longer available.");
    }
    if (canonicalizeWebOrigin(tab.url) !== grant.origin) {
      throw new Error("The matched tab navigated to a different origin.");
    }
    const documentToken = await probeDocument(grant.tabId);
    if (documentToken !== grant.documentToken) {
      throw new Error("The matched page document changed.");
    }
  }

  async function findSiteMatches() {
    const page = await getActiveTabOnce();
    const documentToken = await ensureContentScript(page.tabId);
    const nativeResponse = await nativeClient.request({
      type: "find_site_matches",
      url: page.origin,
    });
    if (!nativeResponse.ok) {
      return nativeResponse;
    }
    if (nativeResponse.response.type !== "site_matches") {
      return {
        ok: false as const,
        error: "Native host returned the wrong response type.",
      };
    }
    if (
      canonicalizeWebOrigin(nativeResponse.response.siteOrigin) !==
      page.origin
    ) {
      return {
        ok: false as const,
        error: "Native host returned matches for a different origin.",
      };
    }

    grants.removeTab(page.tabId);
    const matches = nativeResponse.response.matches.map((match) => ({
      ...match,
      grantId: grants.add(
        generateGrantId(),
        page.tabId,
        documentToken,
        page.origin,
        match.id
      ),
    }));
    return {
      ok: true as const,
      response: {
        ...nativeResponse.response,
        siteUrl: page.origin,
        siteOrigin: page.origin,
        siteHostname: new URL(page.origin).hostname,
        matches,
      },
    };
  }

  async function inspectPageContext() {
    const page = await getActiveTabOnce();
    const documentToken = await ensureContentScript(page.tabId);
    const context = await chromeApi.tabs.sendMessage(
      page.tabId,
      { type: "termkey.inspectPageContext" },
      { frameId: 0 }
    );
    if (
      !isRecord(context) ||
      context.ok !== true ||
      context.documentToken !== documentToken
    ) {
      return {
        ok: false as const,
        error: "Could not inspect the current page.",
      };
    }
    const intent: PopupPageIntent =
      context.intent === "login" ||
      context.intent === "signup" ||
      context.intent === "password_change"
        ? context.intent
        : "unknown";
    return {
      ok: true as const,
      response: {
        type: "page_context" as const,
        context: {
          intent,
          visibleUsername:
            typeof context.visibleUsername === "string"
              ? context.visibleUsername
              : null,
          hasPasswordField: context.hasPasswordField === true,
          hasConfirmationPasswordField:
            context.hasConfirmationPasswordField === true,
          canGeneratePassword: context.canGeneratePassword === true,
        },
      },
    };
  }

  async function generateForPage() {
    const page = await getActiveTabOnce();
    const documentToken = await ensureContentScript(page.tabId);
    const generated = await nativeClient.request({ type: "generate_password" });
    if (!generated.ok) {
      return generated;
    }
    if (generated.response.type !== "generated_password") {
      return {
        ok: false as const,
        error: "Native host returned the wrong response type.",
      };
    }
    const tab = await chromeApi.tabs.get(page.tabId);
    if (
      typeof tab.url !== "string" ||
      canonicalizeWebOrigin(tab.url) !== page.origin ||
      (await probeDocument(page.tabId)) !== documentToken
    ) {
      return {
        ok: false as const,
        error: "The page changed before the generated password could be filled.",
      };
    }
    const fillResponse = await chromeApi.tabs.sendMessage(
      page.tabId,
      {
        type: "termkey.fillGeneratedPassword",
        documentToken,
        password: generated.response.password,
      },
      { frameId: 0 }
    );
    if (!isRecord(fillResponse) || fillResponse.ok !== true) {
      return {
        ok: false as const,
        error:
          isRecord(fillResponse) && typeof fillResponse.error === "string"
            ? fillResponse.error
            : "Content script could not fill generated password fields.",
      };
    }
    return {
      ok: true as const,
      response: {
        type: "generated_password" as const,
        candidate: {
          username:
            typeof fillResponse.username === "string"
              ? fillResponse.username
              : null,
          password: generated.response.password,
          url: page.origin,
        },
        filledPasswordFields:
          typeof fillResponse.filledPasswordFields === "number"
            ? fillResponse.filledPasswordFields
            : 0,
      },
    };
  }

  async function fillSelectedMatch(
    message: Extract<
      PopupToBackgroundMessage,
      { type: "termkey.autofill.fillSelectedMatch" }
    >
  ) {
    const grant = grants.reserve(message.grantId);
    if (!grant) {
      return {
        ok: false as const,
        error: "This login match is already in use or no longer available.",
      };
    }

    try {
      await validateGrant(grant, message.entryId);
    } catch (error) {
      grants.invalidate(message.grantId);
      throw error;
    }
    const nativeResponse = await nativeClient.request({
      type: "get_autofill_entry",
      id: grant.entryId,
      origin: grant.origin,
      secondaryPassword: message.secondaryPassword,
    });
    if (!nativeResponse.ok) {
      grants.release(message.grantId);
      return nativeResponse;
    }
    if (
      nativeResponse.response.type !== "autofill_entry" ||
      nativeResponse.response.entry.id !== grant.entryId
    ) {
      grants.invalidate(message.grantId);
      return {
        ok: false as const,
        error: "Native host returned the wrong autofill entry.",
      };
    }

    try {
      await validateGrant(grant, message.entryId);
    } catch (error) {
      grants.invalidate(message.grantId);
      throw error;
    }
    if (!grants.consume(message.grantId)) {
      return {
        ok: false as const,
        error: "The selected login match expired before delivery.",
      };
    }
    const fillResponse = await chromeApi.tabs.sendMessage(
      grant.tabId,
      {
        type: "termkey-fill-credentials",
        documentToken: grant.documentToken,
        username: nativeResponse.response.entry.username ?? undefined,
        password: nativeResponse.response.entry.password,
      },
      { frameId: 0 }
    );
    if (!isRecord(fillResponse) || fillResponse.ok !== true) {
      return {
        ok: false as const,
        error:
          isRecord(fillResponse) && typeof fillResponse.error === "string"
            ? fillResponse.error
            : "Content script could not fill the page.",
      };
    }
    return {
      ok: true as const,
      response: {
        type: "fill_result" as const,
        entryName: nativeResponse.response.entry.name,
        filledFields:
          typeof fillResponse.filledFields === "number"
            ? fillResponse.filledFields
            : 0,
        filledUsername: fillResponse.filledUsername === true,
        filledPassword: fillResponse.filledPassword === true,
      },
    };
  }

  async function getTrustedContentPage(sender: MessageSender) {
    if (
      sender.id !== chromeApi.runtime.id ||
      sender.frameId !== 0 ||
      typeof sender.tab?.id !== "number" ||
      typeof sender.url !== "string"
    ) {
      throw new Error("Unauthorized content message sender.");
    }
    const tab = await chromeApi.tabs.get(sender.tab.id);
    if (
      tab.id !== sender.tab.id ||
      typeof tab.url !== "string" ||
      canonicalizeWebOrigin(tab.url) !== canonicalizeWebOrigin(sender.url)
    ) {
      throw new Error("The content page changed before capture.");
    }
    return {
      tabId: sender.tab.id,
      origin: canonicalizeWebOrigin(tab.url),
    };
  }

  async function inspectPendingLogin(
    candidate: PendingLogin,
    expectedDocumentToken?: string
  ) {
    let tab: { id?: number; url?: string };
    try {
      tab = await chromeApi.tabs.get(candidate.tabId);
      if (
        tab.id !== candidate.tabId ||
        typeof tab.url !== "string" ||
        canonicalizeWebOrigin(tab.url) !== candidate.origin
      ) {
        pendingLogins.remove(candidate);
        return;
      }
    } catch {
      pendingLogins.remove(candidate);
      return;
    }

    let context: unknown;
    try {
      context = await chromeApi.tabs.sendMessage(
        candidate.tabId,
        { type: "termkey.inspectPageContext" },
        { frameId: 0 }
      );
    } catch {
      return;
    }
    if (
      !isRecord(context) ||
      context.ok !== true ||
      typeof context.documentToken !== "string" ||
      !/^[a-f0-9]{64}$/.test(context.documentToken) ||
      (expectedDocumentToken !== undefined &&
        context.documentToken !== expectedDocumentToken)
    ) {
      return;
    }
    if (context.hasVisibleLoginFailure === true) {
      pendingLogins.remove(candidate);
      return;
    }
    if (
      context.intent !== "login" &&
      context.hasPasswordField !== true &&
      context.hasVisibleLoginFailure === false
    ) {
      pendingLogins.markReady(candidate);
    }
  }

  async function handleContentLifecycleMessage(
    message: ContentLifecycleMessage,
    sender: MessageSender
  ) {
    const page = await getTrustedContentPage(sender);
    if (message.type === "termkey.content.loginSubmitted") {
      const capture = await chromeApi.tabs.sendMessage(
        page.tabId,
        {
          type: "termkey.captureSubmittedLogin",
          documentToken: message.documentToken,
        },
        { frameId: 0 }
      );
      if (
        !isRecord(capture) ||
        capture.ok !== true ||
        (typeof capture.username !== "string" && capture.username !== null) ||
        typeof capture.password !== "string" ||
        capture.password.trim() === ""
      ) {
        return {
          ok: false as const,
          error:
            isRecord(capture) && typeof capture.error === "string"
              ? capture.error
              : "Could not capture the submitted login.",
        };
      }
      pendingLogins.add(
        page.tabId,
        message.documentToken,
        page.origin,
        capture.username,
        capture.password
      );
      return { ok: true as const };
    }

    const candidate = pendingLogins.get(page.tabId);
    if (
      candidate &&
      candidate.origin === page.origin &&
      candidate.sourceDocumentToken === message.documentToken
    ) {
      await inspectPendingLogin(candidate, message.documentToken);
    }
    return { ok: true as const };
  }

  async function handleTabUpdated(
    tabId: number,
    changeInfo: { status?: string; url?: string }
  ) {
    const candidate = pendingLogins.get(tabId);
    if (!candidate) {
      return;
    }
    if (changeInfo.url !== undefined) {
      try {
        if (canonicalizeWebOrigin(changeInfo.url) !== candidate.origin) {
          pendingLogins.remove(candidate);
          return;
        }
      } catch {
        pendingLogins.remove(candidate);
        return;
      }
    }
    let tab: { id?: number; url?: string };
    try {
      tab = await chromeApi.tabs.get(tabId);
      if (
        tab.id !== tabId ||
        typeof tab.url !== "string" ||
        canonicalizeWebOrigin(tab.url) !== candidate.origin
      ) {
        pendingLogins.remove(candidate);
        return;
      }
    } catch {
      pendingLogins.remove(candidate);
      return;
    }
    if (changeInfo.status === "complete") {
      await inspectPendingLogin(candidate);
    }
  }

  async function getActivePendingLogin(requireReady: boolean) {
    const tabs = await chromeApi.tabs.query({
      active: true,
      currentWindow: true,
    });
    const active = tabs[0];
    if (typeof active?.id !== "number") {
      return undefined;
    }
    const candidate = pendingLogins.get(active.id);
    if (!candidate || (requireReady && !candidate.ready)) {
      return undefined;
    }
    try {
      const tab = await chromeApi.tabs.get(candidate.tabId);
      if (
        tab.id !== candidate.tabId ||
        typeof tab.url !== "string" ||
        canonicalizeWebOrigin(tab.url) !== candidate.origin
      ) {
        pendingLogins.remove(candidate);
        return undefined;
      }
    } catch {
      pendingLogins.remove(candidate);
      return undefined;
    }
    const current = pendingLogins.get(candidate.tabId);
    if (
      current !== candidate ||
      (requireReady && !current.ready)
    ) {
      return undefined;
    }
    return candidate;
  }

  async function getPendingLogin() {
    const candidate = await getActivePendingLogin(true);
    if (!candidate) {
      return {
        ok: true as const,
        response: {
          type: "pending_login" as const,
          candidate: null,
        },
      };
    }
    const matches = await nativeClient.request({
      type: "find_site_matches",
      url: candidate.origin,
    });
    if (!matches.ok) {
      return matches;
    }
    if (
      matches.response.type !== "site_matches" ||
      canonicalizeWebOrigin(matches.response.siteOrigin) !== candidate.origin
    ) {
      return {
        ok: false as const,
        error: "Native host returned matches for a different origin.",
      };
    }
    if ((await getActivePendingLogin(true)) !== candidate) {
      return {
        ok: true as const,
        response: {
          type: "pending_login" as const,
          candidate: null,
        },
      };
    }
    const matchingEntries =
      candidate.username === null
        ? []
        : matches.response.matches.filter(
            (match) => match.username === candidate.username
          );
    candidate.updateEntryId =
      matchingEntries.length === 1 ? matchingEntries[0].id : undefined;
    const mode: "save" | "update" =
      candidate.updateEntryId === undefined ? "save" : "update";
    return {
      ok: true as const,
      response: {
        type: "pending_login" as const,
        candidate: {
          username: candidate.username,
          url: candidate.origin,
          mode,
        },
      },
    };
  }

  async function dismissPendingLogin() {
    const candidate = await getActivePendingLogin(false);
    if (candidate) {
      pendingLogins.remove(candidate);
    }
    return {
      ok: true as const,
      response: {
        type: "pending_login" as const,
        candidate: null,
      },
    };
  }

  async function savePendingLogin(
    message: Extract<
      PopupToBackgroundMessage,
      { type: "termkey.pendingLogin.save" }
    >
  ) {
    const candidate = await getActivePendingLogin(true);
    if (!candidate) {
      return {
        ok: false as const,
        error: "No pending login is available to save.",
      };
    }
    const response =
      candidate.updateEntryId === undefined
        ? await nativeClient.request({
            type: "save_password_entry",
            name: message.name,
            username: message.username,
            password: candidate.password,
            url: candidate.origin,
            secondaryPassword: message.secondaryPassword,
          })
        : await nativeClient.request({
            type: "update_password_entry",
            id: candidate.updateEntryId,
            origin: candidate.origin,
            name: message.name,
            username: message.username,
            password: candidate.password,
            url: candidate.origin,
            secondaryPassword: message.secondaryPassword,
          });
    if (!response.ok) {
      return response;
    }
    if (response.response.type !== "save_entry") {
      return {
        ok: false as const,
        error: "Native host returned the wrong response type.",
      };
    }
    pendingLogins.remove(candidate);
    return {
      ok: true as const,
      response: {
        type: "save_entry_result" as const,
        entryName: response.response.entryName,
      },
    };
  }

  async function handleTrustedMessage(
    message: PopupToBackgroundMessage
  ): Promise<PopupToBackgroundResponse> {
    switch (message.type) {
      case "termkey.nativeHost.ping":
        return nativeClient.request({
          type: "ping",
          protocolVersion: NATIVE_PROTOCOL_VERSION,
        });
      case "termkey.nativeHost.status":
        return nativeClient.request({
          type: "status",
          protocolVersion: NATIVE_PROTOCOL_VERSION,
        });
      case "termkey.nativeHost.findSiteMatches":
        return findSiteMatches();
      case "termkey.content.captureSubmittedLogin":
        return {
          ok: false,
          error: "Submitted login capture is available only to the background.",
        };
      case "termkey.content.inspectPageContext":
        return inspectPageContext();
      case "termkey.pendingLogin.get":
        return getPendingLogin();
      case "termkey.pendingLogin.dismiss":
        return dismissPendingLogin();
      case "termkey.pendingLogin.save":
        return savePendingLogin(message);
      case "termkey.passwords.generateForPage":
        return generateForPage();
      case "termkey.autofill.fillSelectedMatch":
        return fillSelectedMatch(message);
      case "termkey.nativeHost.savePasswordEntry": {
        const url =
          message.url === undefined
            ? undefined
            : canonicalizeWebOrigin(message.url);
        const response = await nativeClient.request({
          type: "save_password_entry",
          name: message.name,
          username: message.username,
          password: message.password,
          url,
          secondaryPassword: message.secondaryPassword,
        });
        if (!response.ok) {
          return response;
        }
        if (response.response.type !== "save_entry") {
          return {
            ok: false,
            error: "Native host returned the wrong response type.",
          };
        }
        return {
          ok: true,
          response: {
            type: "save_entry_result",
            entryName: response.response.entryName,
          },
        };
      }
      case "termkey.nativeHost.unlock":
        return nativeClient.request({
          type: "unlock",
          password: message.password,
        });
    }
  }

  async function handleMessage(
    message: PopupToBackgroundMessage,
    sender: MessageSender
  ): Promise<PopupToBackgroundResponse> {
    if (!isTrustedExtensionPageSender(sender, chromeApi)) {
      return {
        ok: false,
        error: "Unauthorized extension message sender.",
      };
    }
    if (!isKnownPopupMessage(message)) {
      return { ok: false, error: "Unsupported extension message." };
    }
    try {
      return await handleTrustedMessage(message);
    } catch (error) {
      return {
        ok: false,
        error:
          error instanceof Error
            ? error.message
            : "The extension request failed.",
      };
    }
  }

  const service = {
    handleMessage,
    handleTabRemoved(tabId: number) {
      grants.removeTab(tabId);
      pendingLogins.removeTab(tabId);
    },
    handleTabUpdated,
  };

  chromeApi.tabs.onRemoved?.addListener((tabId) => {
    service.handleTabRemoved(tabId);
  });
  chromeApi.tabs.onUpdated?.addListener((tabId, changeInfo) =>
    service.handleTabUpdated(tabId, changeInfo)
  );
  chromeApi.runtime.onMessage?.addListener(
    (message, sender, sendResponse) => {
      if (isContentLifecycleMessage(message)) {
        void handleContentLifecycleMessage(message, sender)
          .catch((error) => ({
            ok: false as const,
            error:
              error instanceof Error
                ? error.message
                : "The content lifecycle request failed.",
          }))
          .then(sendResponse);
        return true;
      }
      if (!isKnownPopupMessage(message)) {
        return false;
      }
      void service.handleMessage(message, sender).then(sendResponse);
      return true;
    }
  );

  return service;
}

const runtimeChrome =
  typeof chrome === "undefined"
    ? undefined
    : (chrome as unknown as BackgroundChrome);

if (runtimeChrome?.runtime?.onMessage) {
  createBackgroundService(runtimeChrome);
  console.log("Core connected:", coreReady);
  console.log("Extension ID:", runtimeChrome.runtime.id);
}
