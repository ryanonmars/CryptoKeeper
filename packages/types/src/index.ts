export type NativeHostRequest =
  | {
      type: "ping";
      protocolVersion: number;
    }
  | {
      type: "status";
      protocolVersion?: number;
    }
  | {
      type: "get_autofill_entry";
      id: string;
      origin: string;
      secondaryPassword?: string;
    }
  | {
      type: "find_site_matches";
      url: string;
    }
  | {
      type: "generate_password";
    }
  | {
      type: "save_password_entry";
      name: string;
      username?: string;
      password: string;
      url?: string;
      secondaryPassword?: string;
    }
  | {
      type: "update_password_entry";
      id: string;
      origin: string;
      name: string;
      username?: string;
      password: string;
      url?: string;
      secondaryPassword?: string;
    }
  | {
      type: "list_entries";
    }
  | {
      type: "unlock";
      password: string;
    };

export type NativeHostWireRequest = NativeHostRequest & {
  requestId: string;
};

export type NativeHostPongResponse = {
  type: "pong";
  app: "termkey";
  version: string;
} & ProtocolInfo;

export type NativeHostStatusResponse = {
  type: "status";
  app: "termkey";
  version: string;
  vaultPath: string;
  vaultExists: boolean;
  firstRunComplete: boolean;
  recoveryConfigured: boolean;
  locked: boolean;
} & ProtocolInfo;

export type NativeHostEntrySummary = {
  id: string;
  name: string;
  secretType: string;
  network: string;
  hasSecondaryPassword: boolean;
  publicAddress: string | null;
  username: string | null;
  url: string | null;
};

export type NativeHostSiteMatch = {
  id: string;
  name: string;
  username: string | null;
  matchType:
    | "exact_origin"
    | "exact_host"
    | "subdomain"
    | "registrable_domain";
  hasSecondaryPassword: boolean;
};

export type PopupSiteMatch = NativeHostSiteMatch & {
  grantId: string;
};

export type PopupSiteMatchesResponse = {
  type: "site_matches";
  siteUrl: string;
  siteOrigin: string;
  siteHostname: string;
  matches: PopupSiteMatch[];
};

export type NativeHostAutofillEntry = {
  id: string;
  name: string;
  username: string | null;
  password: string;
};

export type ProtocolInfo = {
  protocolVersion: number;
  capabilities: string[];
};

export type FillCredentialsMessage = {
  type: "termkey-fill-credentials";
  documentToken: string;
  username?: string;
  password: string;
};

export type PopupPageIntent =
  | "login"
  | "signup"
  | "password_change"
  | "unknown";

export type PopupPageContextResponse = {
  type: "page_context";
  context: {
    intent: PopupPageIntent;
    visibleUsername: string | null;
    hasPasswordField: boolean;
    hasEmptyLoginField: boolean;
    hasConfirmationPasswordField: boolean;
    canGeneratePassword: boolean;
  };
};

export type PopupPendingLoginResponse = {
  type: "pending_login";
  candidate: {
    username: string | null;
    url: string;
    mode: "save" | "update" | "unlock";
    requiresSecondaryPassword?: boolean;
    existingEntryName?: string;
  } | null;
};

export type PopupGeneratedPasswordResponse = {
  type: "generated_password";
  candidate: {
    username: string | null;
    password: string;
    url: string;
  };
  filledPasswordFields: number;
};

export type PopupFillResultResponse = {
  type: "fill_result";
  entryName: string;
  filledFields: number;
  filledUsername: boolean;
  filledPassword: boolean;
};

export type PopupSaveResultResponse = {
  type: "save_entry_result";
  entryName: string;
};

export type PendingLoginPromptMode =
  | "save"
  | "update"
  | "unlock"
  | "protected-update"
  | "resolve";

export type PendingLoginPromptMetadata = {
  candidateId: string;
  origin: string;
  hostname: string;
  username: string | null;
  defaultName: string;
  mode: PendingLoginPromptMode;
  isHttp: boolean;
};

export type PendingLoginPromptToBackgroundMessage =
  | {
      type: "termkey.pendingLoginPrompt.get";
      candidateId: string;
    }
  | {
      type: "termkey.pendingLoginPrompt.save";
      candidateId: string;
    }
  | {
      type: "termkey.pendingLoginPrompt.dismiss";
      candidateId: string;
    }
  | {
      type: "termkey.pendingLoginPrompt.openPopup";
      candidateId: string;
      reason: "unlock" | "secondary-password" | "more-options" | "retry";
    };

export type PendingLoginPromptActionResponse =
  | {
      type: "pending_login_prompt";
      candidate: PendingLoginPromptMetadata | null;
    }
  | {
      type: "pending_login_prompt_result";
      outcome:
        | "saved"
        | "updated"
        | "dismissed"
        | "popup-opened"
        | "popup-required";
      entryName?: string;
      fallbackInstruction?: string;
    };

export type PopupUnlockAndSaveResponse = {
  type: "unlock_and_save_result";
  unlocked: boolean;
  saved: boolean;
  mode?: "save" | "update";
  entryName?: string;
  recoveryNotice?: string;
  error?: string;
};

export type NativeHostResponse = (
  | NativeHostPongResponse
  | NativeHostStatusResponse
  | {
      type: "autofill_entry";
      entry: NativeHostAutofillEntry;
    }
  | {
      type: "generated_password";
      password: string;
    }
  | {
      type: "save_entry";
      entryName: string;
    }
  | {
      type: "site_matches";
      siteUrl: string;
      siteOrigin: string;
      siteHostname: string;
      matches: NativeHostSiteMatch[];
    }
  | {
      type: "list_entries";
      entries: NativeHostEntrySummary[];
    }
  | {
      type: "unlock";
      unlocked: true;
      recoveryNotice?: string;
    }
  | {
      type: "error";
      message: string;
    }
) & {
  requestId: string;
};

export type PopupToBackgroundMessage =
  | PendingLoginPromptToBackgroundMessage
  | {
      type: "termkey.nativeHost.ping";
    }
  | {
      type: "termkey.nativeHost.status";
    }
  | {
      type: "termkey.nativeHost.findSiteMatches";
    }
  | {
      type: "termkey.content.captureSubmittedLogin";
    }
  | {
      type: "termkey.content.inspectPageContext";
    }
  | {
      type: "termkey.pendingLogin.get";
    }
  | {
      type: "termkey.pendingLogin.dismiss";
    }
  | {
      type: "termkey.pendingLogin.save";
      name: string;
      username?: string;
      secondaryPassword?: string;
    }
  | {
      type: "termkey.pendingLogin.unlockAndSave";
      name: string;
      username?: string;
      masterPassword: string;
      secondaryPassword?: string;
    }
  | {
      type: "termkey.passwords.generateForPage";
    }
  | {
      type: "termkey.autofill.fillSelectedMatch";
      grantId: string;
      entryId: string;
      secondaryPassword?: string;
    }
  | {
      type: "termkey.nativeHost.savePasswordEntry";
      name: string;
      username?: string;
      password: string;
      url?: string;
      secondaryPassword?: string;
    }
  | {
      type: "termkey.nativeHost.unlock";
      password: string;
    };

export type PopupToBackgroundResponse =
  | {
      ok: true;
      response:
        | Exclude<NativeHostResponse, { type: "site_matches" }>
        | PopupSiteMatchesResponse
        | PopupPageContextResponse
        | PopupPendingLoginResponse
        | PopupGeneratedPasswordResponse
        | PopupFillResultResponse
        | PopupSaveResultResponse
        | PopupUnlockAndSaveResponse
        | PendingLoginPromptActionResponse;
    }
  | {
      ok: false;
      error: string;
    };
