import { invoke } from "@tauri-apps/api/core";
import type {
  AddonStreams,
  AddonUpdate,
  DeepLinkStatus,
  DirectoryCategory,
  DirectoryQuery,
  InstallPreview,
  MarketDetail,
  MarketPage,
  ProfilesState,
  AccountStatus,
  AddonsChoice,
  AddonsPreview,
  LinkCode,
  SyncReport,
  CatalogResponse,
  Descriptor,
  IpcError,
  MetaLookup,
  ResolvedStream,
  Settings,
  SettingsPatch,
} from "./types";

export function errorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as IpcError).message);
  }
  return String(error);
}

export function errorKind(error: unknown): string | undefined {
  if (error && typeof error === "object" && "kind" in error) {
    return String((error as IpcError).kind);
  }
  return undefined;
}

/** `listingKey`: the directory listing the install started from (compared with the live
 * manifest). */
export const addonPreview = (url: string, listingKey?: string | null) =>
  invoke<InstallPreview>("addon_preview", { url, listingKey: listingKey ?? null });
/** Only from the install confirmation. `replace`: an installed addon's transport URL that
 * this one takes the place of (keeping its position). */
export const addonAdd = (url: string, replace?: string | null) =>
  invoke<Descriptor>("addon_add", { url, replace: replace ?? null });
export const addonRemove = (transportUrl: string) =>
  invoke<void>("addon_remove", { transportUrl });
export const addonList = () => invoke<Descriptor[]>("addon_list");
/** `order`: every user addon's transport URL exactly once; resolves to addon_list. */
export const addonReorder = (order: string[]) => invoke<Descriptor[]>("addon_reorder", { order });
export const addonCheckUpdates = (force: boolean) =>
  invoke<AddonUpdate[]>("addon_check_updates", { force });
/** One-click update; rejects with kind "conflict" when capabilities changed (review it). */
export const addonRefresh = (transportUrl: string) =>
  invoke<Descriptor>("addon_refresh", { transportUrl });

export const marketplaceList = (query: DirectoryQuery, page: number, limit: number) =>
  invoke<MarketPage>("marketplace_list", { query, page, limit });
export const marketplaceCategories = () => invoke<DirectoryCategory[]>("marketplace_categories");
export const marketplaceDetail = (key: string) => invoke<MarketDetail>("marketplace_detail", { key });
export const marketplaceRefresh = () => invoke<void>("marketplace_refresh");

export const deeplinkTakePending = () => invoke<string | null>("deeplink_take_pending");
export const deeplinkStremioStatus = () => invoke<DeepLinkStatus>("deeplink_stremio_status");
export const deeplinkSetStremio = (enabled: boolean) =>
  invoke<DeepLinkStatus>("deeplink_set_stremio", { enabled });
/** Emitted by Rust with a stremio:// link the OS handed to Zeiler. */
export const STREMIO_LINK = "deeplink://stremio";

export const catalogLoad = (args: {
  transportUrl: string;
  type: string;
  id: string;
  extra?: { name: string; value: string }[];
}) => invoke<CatalogResponse>("catalog_load", { args: { extra: [], ...args } });

export const metaGet = (type: string, id: string) =>
  invoke<MetaLookup>("meta_get", { type, id });

export const streamsGet = (type: string, id: string) =>
  invoke<AddonStreams[]>("streams_get", { type, id });

export const engineResolve = (request: {
  infoHash: string;
  fileIdx?: number | null;
  sources?: string[];
  filename?: string | null;
  fileMustInclude?: string[];
}) => invoke<ResolvedStream>("engine_resolve", { request });

export const engineStats = (infoHash: string) =>
  invoke<Record<string, unknown>>("engine_stats", { infoHash });

export const engineRemove = (infoHash: string, deleteFiles: boolean) =>
  invoke<void>("engine_remove", { infoHash, deleteFiles });

// Player commands live in ./player-ipc.ts (the unified, backend-neutral player API).

export interface UpdateStatus {
  current: string;
  /** downloaded, installs on next launch */
  ready?: string | null;
}
export const updateStatus = () => invoke<UpdateStatus>("update_status");
/** checks and downloads; resolves to the new version, or null when up to date */
export const updateCheck = () => invoke<string | null>("update_check");
export const updateInstallNow = () => invoke<void>("update_install_now");

export const settingsGet = () => invoke<Settings>("settings_get");
/** Merges just these keys server-side (a group, such as `subtitles`, field by field);
 * resolves to the full new settings. */
export const settingsPatch = (patch: SettingsPatch) =>
  invoke<Settings>("settings_patch", { patch });
/** @deprecated replaces every setting (lost-update prone); use settingsPatch */
export const settingsSet = (settings: Settings) =>
  invoke<Settings>("settings_set", { settings });

/** Emitted by Rust with the full new settings after every successful change. */
export const SETTINGS_CHANGED = "settings://changed";

// ---- local profiles -------------------------------------------------------------------

export const profileList = () => invoke<ProfilesState>("profile_list");
export const profileCreate = (name: string, color: string) =>
  invoke<ProfilesState>("profile_create", { name, color });
export const profileUpdate = (
  id: string,
  change: { name?: string; color?: string },
  pin?: string | null,
) =>
  invoke<ProfilesState>("profile_update", {
    id,
    name: change.name ?? null,
    color: change.color ?? null,
    pin: pin ?? null,
  });
/** `newPin: null` removes the PIN; a profile with one needs `currentPin`. */
export const profileSetPin = (id: string, currentPin: string | null, newPin: string | null) =>
  invoke<ProfilesState>("profile_set_pin", { id, currentPin, newPin });
export const profileSetAvatar = (id: string, dataUrl: string | null, pin?: string | null) =>
  invoke<ProfilesState>("profile_set_avatar", { id, dataUrl, pin: pin ?? null });
/** Rejects with kind "locked" for a wrong or missing PIN, "conflict" while playing. */
export const profileSelect = (id: string, pin?: string | null) =>
  invoke<ProfilesState>("profile_select", { id, pin: pin ?? null });
export const profileDelete = (id: string, pin?: string | null) =>
  invoke<ProfilesState>("profile_delete", { id, pin: pin ?? null });
/** Emitted by Rust with the new ProfilesState after every profile change. */
export const PROFILES_CHANGED = "profiles://changed";

// ---- Stremio account ------------------------------------------------------------------

export const accountStatus = () => invoke<AccountStatus>("account_status");
export const accountLinkStart = () => invoke<LinkCode>("account_link_start");
/** null while the code waits for confirmation in the browser */
export const accountLinkPoll = (code: string, remember: boolean) =>
  invoke<AccountStatus | null>("account_link_poll", { code, remember });
export const accountLoginPassword = (email: string, password: string, remember: boolean) =>
  invoke<AccountStatus>("account_login_password", { email, password, remember });
export const accountLogout = () => invoke<AccountStatus>("account_logout");
export const accountSetSync = (change: { library?: boolean; addons?: false }) =>
  invoke<AccountStatus>("account_set_sync", {
    library: change.library ?? null,
    addons: change.addons ?? null,
  });
export const accountAddonsPreview = () => invoke<AddonsPreview>("account_addons_preview");
export const accountAddonsApply = (choice: AddonsChoice) =>
  invoke<AccountStatus>("account_addons_apply", { choice });
export const accountSyncNow = () => invoke<SyncReport>("account_sync_now");
/** Emitted by Rust with the new AccountStatus after sign-in, sign-out and every sync. */
export const ACCOUNT_CHANGED = "account://changed";
/** Emitted by Rust when an account sync replaced the active profile's addons. */
export const ADDONS_CHANGED = "addons://changed";
