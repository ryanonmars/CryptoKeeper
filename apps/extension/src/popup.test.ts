// @vitest-environment jsdom

import { afterEach, expect, it, vi } from "vitest";

afterEach(() => {
  document.body.innerHTML = "";
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.resetModules();
});

it("does not expose a manual save action", async () => {
  vi.stubGlobal("chrome", {
    runtime: { lastError: undefined, sendMessage: vi.fn() },
    tabs: {
      query: (
        _query: unknown,
        callback: (tabs: Array<{ url?: string }>) => void
      ) => callback([{ url: "https://example.test/account" }]),
    },
  });

  await import("./popup");

  expect(document.querySelector("#save-login")).toBeNull();
});

type PendingLoginPopupOptions = {
  canGeneratePassword?: boolean;
  hasEmptyLoginField?: boolean;
  matches?: Array<{
    id: string;
    grantId: string;
    name: string;
    username: string | null;
    url: string;
    matchType: "exact_origin";
    hasSecondaryPassword: boolean;
  }>;
};

const match = {
  id: "entry-1",
  grantId: "grant-1",
  name: "Example account",
  username: "sam@example.test",
  url: "https://example.test",
  matchType: "exact_origin" as const,
  hasSecondaryPassword: false,
};

async function openPopupWithSiteContext({
  hasEmptyLoginField,
  matches,
  pageContextValues = [hasEmptyLoginField],
}: {
  hasEmptyLoginField: boolean;
  matches: typeof match[];
  pageContextValues?: boolean[];
}) {
  let pageContextRequestCount = 0;
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
            locked: false,
          },
        });
      } else if (message.type === "termkey.content.inspectPageContext") {
        const contextValue =
          pageContextValues[pageContextRequestCount++] ?? hasEmptyLoginField;
        callback({
          ok: true,
          response: {
            type: "page_context",
            context: {
              intent: "login",
              visibleUsername: null,
              hasPasswordField: true,
              hasEmptyLoginField: contextValue,
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
            matches,
          },
        });
      } else if (message.type === "termkey.pendingLogin.get") {
        callback({ ok: true, response: { type: "pending_login", candidate: null } });
      } else if (message.type === "termkey.autofill.fillSelectedMatch") {
        callback({
          ok: true,
          response: {
            type: "fill_result",
            entryName: "Example account",
            filledFields: 2,
            filledUsername: true,
            filledPassword: true,
          },
        });
      }
    }
  );
  vi.stubGlobal("chrome", {
    runtime: { lastError: undefined, sendMessage },
    tabs: {
      query: (
        _query: unknown,
        callback: (tabs: Array<{ url?: string }>) => void
      ) => callback([{ url: "https://example.test/account" }]),
    },
  });

  await import("./popup");
  return sendMessage;
}

async function openPendingLoginPopup(
  mode: "save" | "update" | "unlock",
  handleMessage: (
    message: { type: string },
    callback: (response: unknown) => void,
    dismissCandidate: () => void
  ) => void,
  options: PendingLoginPopupOptions = {}
) {
  let candidate: {
    username: string | null;
    url: string;
    mode: "save" | "update" | "unlock";
  } | null = {
    username: "sam@example.test",
    url: "https://example.test",
    mode,
  };
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
            locked: mode === "unlock",
          },
        });
      } else if (message.type === "termkey.content.inspectPageContext") {
        callback({
          ok: true,
          response: {
            type: "page_context",
            context: {
              intent: "unknown",
              visibleUsername: null,
              hasPasswordField: false,
              hasEmptyLoginField: options.hasEmptyLoginField ?? false,
              hasConfirmationPasswordField: false,
              canGeneratePassword: options.canGeneratePassword ?? false,
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
            matches: options.matches ?? [],
          },
        });
      } else if (message.type === "termkey.pendingLogin.get") {
        callback({ ok: true, response: { type: "pending_login", candidate } });
      } else {
        handleMessage(message, callback, () => {
          candidate = null;
        });
      }
    }
  );
  vi.stubGlobal("chrome", {
    runtime: { lastError: undefined, sendMessage },
    tabs: {
      query: (
        _query: unknown,
        callback: (tabs: Array<{ url?: string }>) => void
      ) => callback([{ url: "https://example.test/account" }]),
    },
  });

  await import("./popup");
  return sendMessage;
}

it("hides Fill when the eligible login fields are already filled", async () => {
  await openPopupWithSiteContext({ hasEmptyLoginField: false, matches: [match] });

  expect(
    document.querySelector<HTMLButtonElement>("#fill-best-match")?.hidden
  ).toBe(true);
});

it("shows Fill when an eligible login field is empty", async () => {
  await openPopupWithSiteContext({ hasEmptyLoginField: true, matches: [match] });

  expect(
    document.querySelector<HTMLButtonElement>("#fill-best-match")?.hidden
  ).toBe(false);
});

it("hides the match picker when eligible login fields are already filled", async () => {
  await openPopupWithSiteContext({
    hasEmptyLoginField: false,
    matches: [match, { ...match, id: "entry-2", grantId: "grant-2" }],
  });

  expect(document.querySelector<HTMLElement>("#match-picker")?.hidden).toBe(
    true
  );
});

it("shows the match picker when eligible login fields are empty", async () => {
  await openPopupWithSiteContext({
    hasEmptyLoginField: true,
    matches: [match, { ...match, id: "entry-2", grantId: "grant-2" }],
  });

  expect(document.querySelector<HTMLElement>("#match-picker")?.hidden).toBe(
    false
  );
});

it("refreshes page context after filling and hides Fill when the fields are filled", async () => {
  const sendMessage = await openPopupWithSiteContext({
    hasEmptyLoginField: true,
    matches: [match],
    pageContextValues: [true, false],
  });

  document.querySelector<HTMLButtonElement>("#fill-best-match")?.click();

  expect(document.querySelector<HTMLButtonElement>("#fill-best-match")?.hidden).toBe(
    true
  );
  expect(
    sendMessage.mock.calls.filter(
      ([message]) =>
        (message as { type: string }).type ===
        "termkey.content.inspectPageContext"
    )
  ).toHaveLength(2);
});

it.each([
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

it("supports an HTTP active tab", async () => {
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
      ) => callback([{ url: "http://qbittorrent.truenas/" }]),
    },
  });

  await import("./popup");

  expect(document.querySelector<HTMLElement>("#site-panel")?.hidden).toBe(false);
  expect(document.querySelector("#site-hostname")?.textContent).toBe(
    "http://qbittorrent.truenas"
  );
});

it("offers a submitted login for saving without receiving its password", async () => {
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
            locked: false,
          },
        });
      } else if (message.type === "termkey.content.inspectPageContext") {
        callback({
          ok: true,
          response: {
            type: "page_context",
            context: {
              intent: "unknown",
              visibleUsername: null,
              hasPasswordField: false,
              hasConfirmationPasswordField: false,
              canGeneratePassword: false,
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
      } else if (message.type === "termkey.pendingLogin.get") {
        callback({
          ok: true,
          response: {
            type: "pending_login",
            candidate: {
              username: "sam@example.test",
              url: "https://example.test",
              mode: "save",
            },
          },
        });
      }
    }
  );
  vi.stubGlobal("chrome", {
    runtime: { lastError: undefined, sendMessage },
    tabs: {
      query: (
        _query: unknown,
        callback: (tabs: Array<{ url?: string }>) => void
      ) => callback([{ url: "https://example.test/account" }]),
    },
  });

  await import("./popup");

  expect(document.querySelector<HTMLElement>("#save-section")?.hidden).toBe(false);
  expect(document.querySelector("#save-panel-label")?.textContent).toBe(
    "Save this login?"
  );
  expect(document.querySelector("#submit-save")?.textContent).toBe(
    "Save login"
  );
  expect(document.querySelector<HTMLInputElement>("#save-username")?.value).toBe(
    "sam@example.test"
  );
});

it("labels a submitted login that matches an existing username as an update", async () => {
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
            locked: false,
          },
        });
      } else if (message.type === "termkey.content.inspectPageContext") {
        callback({
          ok: true,
          response: {
            type: "page_context",
            context: {
              intent: "unknown",
              visibleUsername: null,
              hasPasswordField: false,
              hasConfirmationPasswordField: false,
              canGeneratePassword: false,
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
            matches: [
              {
                id: "entry-1",
                grantId: "grant-1",
                name: "Example account",
                username: "sam@example.test",
                url: "https://example.test",
                matchType: "exact_origin",
                hasSecondaryPassword: false,
              },
            ],
          },
        });
      } else if (message.type === "termkey.pendingLogin.get") {
        callback({
          ok: true,
          response: {
            type: "pending_login",
            candidate: {
              username: "sam@example.test",
              url: "https://example.test",
              mode: "update",
            },
          },
        });
      }
    }
  );
  vi.stubGlobal("chrome", {
    runtime: { lastError: undefined, sendMessage },
    tabs: {
      query: (
        _query: unknown,
        callback: (tabs: Array<{ url?: string }>) => void
      ) => callback([{ url: "https://example.test/account" }]),
    },
  });

  await import("./popup");

  expect(document.querySelector("#save-panel-label")?.textContent).toBe(
    "Update the saved login for this site?"
  );
  expect(document.querySelector("#submit-save")?.textContent).toBe(
    "Update login"
  );
});

it("saves a submitted login with editable metadata but never a candidate password", async () => {
  let saveRequest: unknown;
  await openPendingLoginPopup("save", (message, callback, dismissCandidate) => {
    if (message.type === "termkey.pendingLogin.save") {
      saveRequest = message;
      dismissCandidate();
      callback({
        ok: true,
        response: { type: "save_entry_result", entryName: "Edited account" },
      });
    }
  });

  const name = document.querySelector<HTMLInputElement>("#save-entry-name");
  const username = document.querySelector<HTMLInputElement>("#save-username");
  const useSecondary = document.querySelector<HTMLInputElement>(
    "#save-use-secondary-password"
  );
  const secondary = document.querySelector<HTMLInputElement>("#save-secondary-password");
  const confirmation = document.querySelector<HTMLInputElement>(
    "#save-secondary-password-confirm"
  );
  const submit = document.querySelector<HTMLButtonElement>("#submit-save");
  if (!name || !username || !useSecondary || !secondary || !confirmation || !submit) {
    throw new Error("Save controls did not initialize.");
  }

  name.value = "Edited account";
  username.value = "edited@example.test";
  useSecondary.checked = true;
  useSecondary.dispatchEvent(new Event("change"));
  secondary.value = "secondary-secret";
  confirmation.value = "secondary-secret";
  submit.click();

  expect(saveRequest).toEqual({
    type: "termkey.pendingLogin.save",
    name: "Edited account",
    username: "edited@example.test",
    secondaryPassword: "secondary-secret",
  });
  expect(document.querySelector<HTMLElement>("#save-section")?.hidden).toBe(true);
});

it("offers a locked submitted login for unlock and save", async () => {
  await openPendingLoginPopup("unlock", () => {});

  expect(document.querySelector("#save-panel-label")?.textContent).toBe(
    "Unlock to save this login"
  );
  expect(document.querySelector("#submit-save")?.textContent).toBe(
    "Unlock & Save"
  );
  expect(document.querySelector<HTMLButtonElement>("#unlock-vault")?.hidden).toBe(
    true
  );
});

it("unlocks and saves a submitted login without exposing its website password", async () => {
  let unlockAndSaveRequest: unknown;
  await openPendingLoginPopup("unlock", (message, callback, dismissCandidate) => {
    if (message.type === "termkey.pendingLogin.unlockAndSave") {
      unlockAndSaveRequest = message;
      dismissCandidate();
      callback({
        ok: true,
        response: {
          type: "unlock_and_save_result",
          unlocked: true,
          saved: true,
          mode: "save",
          entryName: "Edited account",
        },
      });
    }
  });

  const name = document.querySelector<HTMLInputElement>("#save-entry-name");
  const username = document.querySelector<HTMLInputElement>("#save-username");
  const masterPassword = document.querySelector<HTMLInputElement>("#master-password");
  const submit = document.querySelector<HTMLButtonElement>("#submit-save");
  if (!name || !username || !masterPassword || !submit) {
    throw new Error("Unlock and save controls did not initialize.");
  }

  name.value = "Edited account";
  username.value = "edited@example.test";
  masterPassword.value = "master-secret";
  masterPassword.dispatchEvent(new Event("input"));
  submit.click();

  expect(unlockAndSaveRequest).toEqual({
    type: "termkey.pendingLogin.unlockAndSave",
    name: "Edited account",
    username: "edited@example.test",
    masterPassword: "master-secret",
    secondaryPassword: undefined,
  });
  expect(document.querySelector<HTMLElement>("#save-section")?.hidden).toBe(true);
  expect(document.querySelector("#native-host-status")?.textContent).toContain(
    "Saved Edited account."
  );
});

it("keeps the locked save prompt focused after an unlock failure", async () => {
  await openPendingLoginPopup("unlock", (message, callback) => {
    if (message.type === "termkey.pendingLogin.unlockAndSave") {
      callback({
        ok: true,
        response: {
          type: "unlock_and_save_result",
          unlocked: false,
          saved: false,
          error: "Incorrect master password.",
        },
      });
    }
  });

  const masterPassword = document.querySelector<HTMLInputElement>("#master-password");
  if (!masterPassword) {
    throw new Error("Master password control did not initialize.");
  }
  masterPassword.value = "wrong-master-secret";
  masterPassword.dispatchEvent(new Event("input"));
  document.querySelector<HTMLButtonElement>("#submit-save")?.click();

  expect(document.querySelector<HTMLElement>("#save-section")?.hidden).toBe(false);
  expect(document.activeElement).toBe(masterPassword);
  expect(document.querySelector("#native-host-status")?.textContent).toContain(
    "Incorrect master password."
  );
});

it("dismisses a submitted login before clearing its save prompt", async () => {
  let promptWasVisibleDuringDismiss = false;
  await openPendingLoginPopup("save", (message, callback, dismissCandidate) => {
    if (message.type === "termkey.pendingLogin.dismiss") {
      promptWasVisibleDuringDismiss =
        document.querySelector<HTMLElement>("#save-section")?.hidden === false;
      dismissCandidate();
      callback({ ok: true, response: { type: "pending_login", candidate: null } });
    }
  });

  document.querySelector<HTMLButtonElement>("#cancel-save")?.click();

  expect(promptWasVisibleDuringDismiss).toBe(true);
  expect(document.querySelector<HTMLElement>("#save-section")?.hidden).toBe(true);
});

it("keeps the submitted login prompt available after a save failure", async () => {
  await openPendingLoginPopup("save", (message, callback) => {
    if (message.type === "termkey.pendingLogin.save") {
      callback({ ok: false, error: "Vault remains locked." });
    }
  });

  document.querySelector<HTMLButtonElement>("#submit-save")?.click();

  expect(document.querySelector<HTMLElement>("#save-section")?.hidden).toBe(false);
  expect(document.querySelector("#native-host-status")?.textContent).toContain(
    "Save failed: Vault remains locked."
  );
});

it.each([
  {
    actionSelector: "#generate-password",
    requestType: "termkey.passwords.generateForPage",
    options: { canGeneratePassword: true },
  },
  {
    actionSelector: "#fill-best-match",
    requestType: "termkey.autofill.fillSelectedMatch",
    options: {
      hasEmptyLoginField: true,
      matches: [
        {
          id: "entry-1",
          grantId: "grant-1",
          name: "Example account",
          username: "sam@example.test",
          url: "https://example.test",
          matchType: "exact_origin" as const,
          hasSecondaryPassword: false,
        },
      ],
    },
  },
])(
  "dismisses the submitted login before starting $requestType",
  async ({ actionSelector, requestType, options }) => {
    const sendMessage = await openPendingLoginPopup(
      "save",
      (message, callback, dismissCandidate) => {
        if (message.type === "termkey.pendingLogin.dismiss") {
          dismissCandidate();
          callback({
            ok: true,
            response: { type: "pending_login", candidate: null },
          });
        }
      },
      options
    );

    document.querySelector<HTMLButtonElement>(actionSelector)?.click();

    const relevantRequests = sendMessage.mock.calls
      .map(([message]) => (message as { type: string }).type)
      .filter(
        (type) =>
          type === "termkey.pendingLogin.dismiss" || type === requestType
      );
    expect(relevantRequests).toEqual([
      "termkey.pendingLogin.dismiss",
      requestType,
    ]);
  }
);

it.each([
  { name: "reports a runtime error", runtimeError: "Background service worker stopped." },
  { name: "returns no response", runtimeError: undefined },
])(
  "keeps a submitted login retryable when its save transport $name",
  async ({ runtimeError }) => {
    await openPendingLoginPopup("save", (message, callback) => {
      if (message.type === "termkey.pendingLogin.save") {
        if (runtimeError) {
          (
            chrome.runtime as unknown as {
              lastError?: { message: string };
            }
          ).lastError = { message: runtimeError };
        }
        callback(undefined);
        (
          chrome.runtime as unknown as {
            lastError?: { message: string };
          }
        ).lastError = undefined;
      }
    });

    document.querySelector<HTMLButtonElement>("#submit-save")?.click();

    expect(document.querySelector<HTMLElement>("#save-section")?.hidden).toBe(false);
    expect(document.querySelector("#submit-save")?.textContent).toBe("Save login");
  }
);

it("saves a generated-password candidate through the native save request", async () => {
    const candidate = { password: "generated-password", url: "https://example.test" };
    let nativeSave: unknown;
    await openPendingLoginPopup(
      "save",
      (message, callback, dismissCandidate) => {
        if (message.type === "termkey.pendingLogin.dismiss") {
          dismissCandidate();
          callback({
            ok: true,
            response: { type: "pending_login", candidate: null },
          });
          return;
        }
        if (message.type === "termkey.passwords.generateForPage") {
          callback({
            ok: true,
            response: {
              type: "generated_password",
              candidate: {
                username: "manual@example.test",
                password: candidate.password,
                url: candidate.url,
              },
              filledPasswordFields: 1,
            },
          });
          return;
        }
        if (message.type === "termkey.nativeHost.savePasswordEntry") {
          nativeSave = message;
          callback({
            ok: true,
            response: { type: "save_entry_result", entryName: "Manual login" },
          });
        }
      },
      { canGeneratePassword: true }
    );

    document.querySelector<HTMLButtonElement>("#generate-password")?.click();
    document.querySelector<HTMLButtonElement>("#submit-save")?.click();

    expect(nativeSave).toMatchObject({
      type: "termkey.nativeHost.savePasswordEntry",
      password: candidate.password,
      url: candidate.url,
    });
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
              hasEmptyLoginField: true,
              hasConfirmationPasswordField: false,
              canGeneratePassword: false,
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
            protocolVersion: 3,
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
