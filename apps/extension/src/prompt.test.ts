// @vitest-environment jsdom

import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type {
  PendingLoginPromptMetadata,
  PopupToBackgroundResponse,
} from "@termkey/types";

const candidateId = "b".repeat(64);
const fallbackInstruction =
  "Click the TermKey toolbar icon to continue. This login is still available.";

let sendMessage: ReturnType<typeof vi.fn>;

function metadata(
  mode: PendingLoginPromptMetadata["mode"] = "save",
  overrides: Partial<PendingLoginPromptMetadata> = {}
): PendingLoginPromptMetadata {
  return {
    candidateId,
    origin: "https://example.test",
    hostname: "example.test",
    username: "sam@example.test",
    defaultName: "example.test • sam@example.test",
    mode,
    isHttp: false,
    ...overrides,
  };
}

function metadataResponse(
  candidate: PendingLoginPromptMetadata | null
): PopupToBackgroundResponse {
  return {
    ok: true,
    response: {
      type: "pending_login_prompt",
      candidate,
    },
  };
}

function actionResponse(
  outcome:
    | "saved"
    | "updated"
    | "dismissed"
    | "popup-opened"
    | "popup-required",
  overrides: { entryName?: string; fallbackInstruction?: string } = {}
): PopupToBackgroundResponse {
  return {
    ok: true,
    response: {
      type: "pending_login_prompt_result",
      outcome,
      ...overrides,
    },
  };
}

async function renderPrompt(candidate: PendingLoginPromptMetadata | null) {
  sendMessage.mockResolvedValueOnce(metadataResponse(candidate));
  await import("./prompt");
  await vi.waitFor(() => {
    expect(
      sendMessage.mock.calls.some(
        ([message]) =>
          (message as { type?: string }).type ===
          "termkey.pendingLoginPrompt.get"
      )
    ).toBe(true);
  });
}

function button(selector: string) {
  const element = document.querySelector<HTMLButtonElement>(selector);
  if (!element) {
    throw new Error(`Missing prompt button: ${selector}`);
  }
  return element;
}

function statusText() {
  const status = document.querySelector<HTMLElement>('[role="status"]');
  if (!status) {
    throw new Error("Missing prompt status.");
  }
  return status.textContent;
}

beforeEach(() => {
  vi.resetModules();
  document.body.textContent = "";
  window.history.replaceState(null, "", `/#candidate=${candidateId}`);
  sendMessage = vi.fn();
  vi.stubGlobal("chrome", {
    runtime: {
      sendMessage,
    },
  });
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.textContent = "";
});

it.each(["", "#candidate=short", `#candidate=${"G".repeat(64)}`])(
  "rejects a missing or malformed candidate fragment without sending an action: %s",
  async (fragment) => {
    window.history.replaceState(null, "", `/${fragment}`);

    await import("./prompt");

    expect(document.body.textContent).toContain(
      "This login is no longer available"
    );
    expect(sendMessage).not.toHaveBeenCalled();
  }
);

it("renders an unavailable state when the candidate metadata is gone", async () => {
  await renderPrompt(null);

  await vi.waitFor(() => {
    expect(document.body.textContent).toContain(
      "This login is no longer available"
    );
  });
  expect(sendMessage).toHaveBeenCalledTimes(1);
});

it("renders safe save metadata and the required controls", async () => {
  await renderPrompt(metadata());

  await vi.waitFor(() => {
    expect(document.body.textContent).toContain("Save this login?");
  });
  expect(document.body.textContent).toContain("sam@example.test");
  expect(document.body.textContent).toContain("example.test");
  expect(button('[data-action="primary"]').textContent).toBe("Save");
  expect(button('[data-action="more-options"]').textContent).toBe(
    "More options"
  );
  expect(button('[aria-label="Dismiss"]').textContent).toBe("×");
});

it("renders update metadata with an Update action", async () => {
  await renderPrompt(metadata("update"));

  await vi.waitFor(() => {
    expect(document.body.textContent).toContain("Update existing login");
  });
  expect(button('[data-action="primary"]').textContent).toBe("Update");
});

it("renders unlock metadata with an Unlock & Save action", async () => {
  await renderPrompt(metadata("unlock"));

  await vi.waitFor(() => {
    expect(button('[data-action="primary"]').textContent).toBe(
      "Unlock & Save"
    );
  });
});

it("explains that a protected update continues in the toolbar popup", async () => {
  await renderPrompt(metadata("protected-update"));

  await vi.waitFor(() => {
    expect(document.body.textContent).toContain("Update existing login");
  });
  expect(button('[data-action="primary"]').textContent).toBe("Update");
  expect(document.body.textContent).toContain(
    "The toolbar popup will request the secondary password."
  );
});

it("visibly warns when prompt metadata identifies an HTTP site", async () => {
  await renderPrompt(
    metadata("save", {
      origin: "http://example.test",
      isHttp: true,
    })
  );

  await vi.waitFor(() => {
    expect(document.body.textContent).toContain("Unencrypted HTTP site");
  });
});

it("keeps protected-update guidance visible for an HTTP site", async () => {
  await renderPrompt(
    metadata("protected-update", {
      origin: "http://example.test",
      isHttp: true,
    })
  );

  await vi.waitFor(() => {
    expect(document.body.textContent).toContain("Unencrypted HTTP site");
  });
  expect(document.body.textContent).toContain(
    "The toolbar popup will request the secondary password."
  );
});

it("never renders a password input", async () => {
  await renderPrompt(metadata());

  await vi.waitFor(() => {
    expect(document.querySelector('[data-action="primary"]')).not.toBeNull();
  });
  expect(document.querySelector('input[type="password"]')).toBeNull();
  expect(document.querySelector("input")).toBeNull();
});

it("does not render or send unexpected website, master, or secondary passwords", async () => {
  const unsafeMetadata = {
    ...metadata(),
    websitePassword: "website-secret",
    masterPassword: "master-secret",
    secondaryPassword: "secondary-secret",
  } as PendingLoginPromptMetadata;
  await renderPrompt(unsafeMetadata);
  sendMessage.mockResolvedValueOnce({
    ...actionResponse("saved", { entryName: "Example login" }),
    websitePassword: "action-website-secret",
    masterPassword: "action-master-secret",
    secondaryPassword: "action-secondary-secret",
  });

  button('[data-action="primary"]').click();

  await vi.waitFor(() => {
    expect(statusText()).toBe("Saved");
  });
  const rendered = document.body.textContent ?? "";
  expect(rendered).not.toContain("website-secret");
  expect(rendered).not.toContain("master-secret");
  expect(rendered).not.toContain("secondary-secret");
  expect(sendMessage).toHaveBeenLastCalledWith({
    type: "termkey.pendingLoginPrompt.save",
    candidateId,
  });
});

it("disables the primary action while a request is in flight", async () => {
  let resolveAction:
    | ((response: PopupToBackgroundResponse) => void)
    | undefined;
  const pendingAction = new Promise<PopupToBackgroundResponse>((resolve) => {
    resolveAction = resolve;
  });
  await renderPrompt(metadata());
  sendMessage.mockReturnValueOnce(pendingAction);

  const primary = button('[data-action="primary"]');
  primary.click();

  expect(primary.disabled).toBe(true);
  resolveAction?.(actionResponse("saved"));
  await vi.waitFor(() => {
    expect(statusText()).toBe("Saved");
  });
});

it.each([
  ["save", "saved", "Saved"],
  ["update", "updated", "Updated"],
] as const)("renders %s success as %s", async (mode, outcome, expected) => {
  await renderPrompt(metadata(mode));
  sendMessage.mockResolvedValueOnce(actionResponse(outcome));

  button('[data-action="primary"]').click();

  await vi.waitFor(() => {
    expect(statusText()).toBe(expected);
  });
});

it("leaves the candidate actionable after a retryable native error", async () => {
  await renderPrompt(metadata());
  sendMessage
    .mockResolvedValueOnce({
      ok: false,
      error: "TermKey could not save this login. Try again.",
    })
    .mockResolvedValueOnce(actionResponse("saved"));
  const primary = button('[data-action="primary"]');

  primary.click();
  await vi.waitFor(() => {
    expect(statusText()).toBe("TermKey could not save this login. Try again.");
  });
  expect(primary.disabled).toBe(false);

  primary.click();
  await vi.waitFor(() => {
    expect(statusText()).toBe("Saved");
  });
});

it("displays the exact manual toolbar fallback instruction", async () => {
  await renderPrompt(metadata("resolve"));
  sendMessage.mockResolvedValueOnce(
    actionResponse("popup-required", { fallbackInstruction })
  );

  button('[data-action="primary"]').click();

  await vi.waitFor(() => {
    expect(statusText()).toBe(fallbackInstruction);
  });
});

it("dismisses only the current candidate", async () => {
  await renderPrompt(metadata());
  sendMessage.mockResolvedValueOnce(actionResponse("dismissed"));

  button('[aria-label="Dismiss"]').click();

  await vi.waitFor(() => {
    expect(sendMessage).toHaveBeenLastCalledWith({
      type: "termkey.pendingLoginPrompt.dismiss",
      candidateId,
    });
  });
});

it("hands More options to the trusted popup", async () => {
  await renderPrompt(metadata());
  sendMessage.mockResolvedValueOnce(actionResponse("popup-opened"));

  button('[data-action="more-options"]').click();

  await vi.waitFor(() => {
    expect(sendMessage).toHaveBeenLastCalledWith({
      type: "termkey.pendingLoginPrompt.openPopup",
      candidateId,
      reason: "more-options",
    });
  });
});

it.each([
  ["unlock", "unlock"],
  ["protected-update", "secondary-password"],
  ["resolve", "retry"],
] as const)("hands %s primary actions to the popup with reason %s", async (
  mode,
  reason
) => {
  await renderPrompt(metadata(mode));
  sendMessage.mockResolvedValueOnce(actionResponse("popup-opened"));

  button('[data-action="primary"]').click();

  await vi.waitFor(() => {
    expect(sendMessage).toHaveBeenLastCalledWith({
      type: "termkey.pendingLoginPrompt.openPopup",
      candidateId,
      reason,
    });
  });
});
