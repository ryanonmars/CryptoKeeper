// @vitest-environment jsdom

import { afterEach, expect, it, vi } from "vitest";

afterEach(() => {
  document.body.innerHTML = "";
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.resetModules();
});

it.each([
  ["HTTP", "http://example.test/login"],
  ["URL userinfo", "https://user:password@example.test/login"],
  ["malformed", "https://[invalid/login"],
])("rejects %s active tab URLs", async (_caseName, activeUrl) => {
  const sendMessage = vi.fn(
    (
      message: { type: string },
      callback: (response: unknown) => void
    ) => {
      if (message.type === "termkey.nativeHost.status") {
        callback({
          ok: true,
          response: {
            type: "status",
            app: "termkey",
            version: "1.0.0",
            vaultPath: "/vault",
            vaultExists: true,
            firstRunComplete: true,
            recoveryConfigured: true,
            locked: true,
          },
        });
      }
    }
  );
  vi.stubGlobal("chrome", {
    runtime: {
      lastError: undefined,
      sendMessage,
    },
    tabs: {
      query: (
        _query: unknown,
        callback: (tabs: Array<{ url?: string }>) => void
      ) => callback([{ url: activeUrl }]),
    },
  });

  await import("./popup");

  const sitePanel = document.querySelector<HTMLElement>("#site-panel");
  expect(sitePanel?.hidden).toBe(true);
  expect(document.querySelector("#site-hostname")?.textContent).toBe(
    "Waiting for page..."
  );
  expect(sendMessage.mock.calls.map(([message]) => message.type)).toEqual([
    "termkey.nativeHost.status",
  ]);
});

it("retries a protected fill with the same grant after a wrong secondary password", async () => {
  const sendMessage = vi.fn(
    (
      message: { type: string; secondaryPassword?: string },
      callback: (response: unknown) => void
    ) => {
      if (message.type === "termkey.nativeHost.status") {
        callback({
          ok: true,
          response: {
            type: "status",
            app: "termkey",
            version: "1.0.0",
            vaultPath: "/vault",
            vaultExists: true,
            firstRunComplete: true,
            recoveryConfigured: true,
            locked: false,
          },
        });
        return;
      }
      if (message.type === "termkey.content.inspectPageContext") {
        callback({
          ok: true,
          response: {
            type: "page_context",
            context: {
              intent: "login",
              visibleUsername: "person@example.test",
              hasPasswordField: true,
              hasConfirmationPasswordField: false,
              canGeneratePassword: false,
              hasPendingSaveUsername: false,
              pendingUsername: null,
            },
          },
        });
        return;
      }
      if (message.type === "termkey.nativeHost.findSiteMatches") {
        callback({
          ok: true,
          response: {
            type: "site_matches",
            siteUrl: "https://example.test",
            siteOrigin: "https://example.test",
            siteHostname: "example.test",
            matches: [
              {
                id: "entry-1",
                grantId: "retry-grant",
                name: "Example",
                username: "person@example.test",
                url: "https://example.test",
                matchType: "exact_origin",
                hasSecondaryPassword: true,
              },
            ],
          },
        });
        return;
      }
      if (message.type === "termkey.autofill.fillSelectedMatch") {
        callback(
          message.secondaryPassword === "correct"
            ? {
                ok: true,
                response: {
                  type: "fill_result",
                  entryName: "Example",
                  filledFields: 2,
                  filledUsername: true,
                  filledPassword: true,
                },
              }
            : { ok: false, error: "Invalid secondary password" }
        );
      }
    }
  );
  vi.stubGlobal("chrome", {
    runtime: {
      lastError: undefined,
      sendMessage,
    },
    tabs: {
      query: (
        _query: unknown,
        callback: (tabs: Array<{ url?: string }>) => void
      ) =>
        callback([
          {
            url: "https://example.test/login?next=%2Fvault#password",
          },
        ]),
    },
  });

  await import("./popup");
  expect(document.querySelector("#site-hostname")?.textContent).toBe(
    "https://example.test"
  );
  const fillButton = document.querySelector<HTMLButtonElement>(
    "#fill-best-match"
  );
  const secondaryPassword = document.querySelector<HTMLInputElement>(
    "#secondary-password"
  );
  const submit = document.querySelector<HTMLButtonElement>("#unlock-vault");
  if (!fillButton || !secondaryPassword || !submit) {
    throw new Error("Popup controls did not initialize.");
  }

  fillButton.click();
  secondaryPassword.value = "wrong";
  submit.click();
  expect(document.querySelector("#native-host-status")?.textContent).toContain(
    "Invalid secondary password"
  );

  secondaryPassword.value = "correct";
  submit.click();
  expect(document.querySelector("#native-host-status")?.textContent).toContain(
    "Filled Example"
  );
  const fillRequests = sendMessage.mock.calls
    .map(([message]) => message)
    .filter(
      (message) =>
        message.type === "termkey.autofill.fillSelectedMatch"
    );
  expect(fillRequests).toEqual([
    {
      type: "termkey.autofill.fillSelectedMatch",
      grantId: "retry-grant",
      entryId: "entry-1",
      secondaryPassword: "wrong",
    },
    {
      type: "termkey.autofill.fillSelectedMatch",
      grantId: "retry-grant",
      entryId: "entry-1",
      secondaryPassword: "correct",
    },
  ]);
});

it("keeps a recovery notice visible after unlock refreshes the page state", async () => {
  const notice =
    "Vault upgraded. Configure a new recovery phrase with `termkey config recovery`.";
  const sendMessage = vi.fn(
    (
      message: { type: string },
      callback: (response: unknown) => void
    ) => {
      if (message.type === "termkey.nativeHost.status") {
        callback({
          ok: true,
          response: {
            type: "status",
            app: "termkey",
            version: "1.0.0",
            protocolVersion: 2,
            capabilities: [],
            vaultPath: "/vault",
            vaultExists: true,
            firstRunComplete: true,
            recoveryConfigured: false,
            locked: true,
          },
        });
      } else if (message.type === "termkey.nativeHost.unlock") {
        callback({
          ok: true,
          response: {
            type: "unlock",
            unlocked: true,
            recoveryNotice: notice,
          },
        });
      } else if (message.type === "termkey.content.inspectPageContext") {
        callback({
          ok: true,
          response: {
            type: "page_context",
            context: {
              intent: "login",
              visibleUsername: null,
              hasPasswordField: true,
              hasConfirmationPasswordField: false,
              canGeneratePassword: false,
              hasPendingSaveUsername: false,
              pendingUsername: null,
            },
          },
        });
      } else if (message.type === "termkey.nativeHost.findSiteMatches") {
        callback({
          ok: true,
          response: {
            type: "site_matches",
            siteUrl: "https://example.test",
            siteOrigin: "https://example.test",
            siteHostname: "example.test",
            matches: [],
          },
        });
      }
    }
  );
  vi.stubGlobal("chrome", {
    runtime: {
      lastError: undefined,
      sendMessage,
    },
    tabs: {
      query: (
        _query: unknown,
        callback: (tabs: Array<{ url?: string }>) => void
      ) => callback([{ url: "https://example.test/login" }]),
    },
  });

  await import("./popup");
  const password =
    document.querySelector<HTMLInputElement>("#master-password");
  const unlock = document.querySelector<HTMLButtonElement>("#unlock-vault");
  if (!password || !unlock) {
    throw new Error("Popup controls did not initialize.");
  }
  password.value = "master-password";
  unlock.click();

  const recoveryNotice =
    document.querySelector<HTMLParagraphElement>("#recovery-notice");
  expect(recoveryNotice?.hidden).toBe(false);
  expect(recoveryNotice?.textContent).toBe(notice);
});
