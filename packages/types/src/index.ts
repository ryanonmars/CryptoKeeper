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

export type PopupCapturedLoginResponse = {
  type: "captured_login";
  candidate: {
    username: string | null;
    password: string;
    url: string;
  };
  usedStoredUsername?: boolean;
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
    hasConfirmationPasswordField: boolean;
    canGeneratePassword: boolean;
    hasPendingSaveUsername: boolean;
    pendingUsername: string | null;
  };
};

export type PopupCapturedLoginStepResponse = {
  type: "captured_login_step";
  step: "username_only";
  username: string;
  url: string;
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
      type: "termkey.content.captureVisibleCredentials";
    }
  | {
      type: "termkey.content.inspectPageContext";
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
        | PopupCapturedLoginResponse
        | PopupPageContextResponse
        | PopupCapturedLoginStepResponse
        | PopupGeneratedPasswordResponse
        | PopupFillResultResponse
        | PopupSaveResultResponse;
    }
  | {
      ok: false;
      error: string;
    };
