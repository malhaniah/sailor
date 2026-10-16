import { invoke } from "@tauri-apps/api/core";
import type {
  AddonStreams,
  CatalogResponse,
  Descriptor,
  IpcError,
  MetaLookup,
  ResolvedStream,
  Settings,
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

export const addonPreview = (url: string) => invoke<Descriptor>("addon_preview", { url });
export const addonAdd = (url: string) => invoke<Descriptor>("addon_add", { url });
export const addonRemove = (transportUrl: string) =>
  invoke<void>("addon_remove", { transportUrl });
export const addonList = () => invoke<Descriptor[]>("addon_list");

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
/** Merges just these keys server-side; resolves to the full new settings. */
export const settingsPatch = (patch: Partial<Settings>) =>
  invoke<Settings>("settings_patch", { patch });
/** @deprecated replaces every setting (lost-update prone); use settingsPatch */
export const settingsSet = (settings: Settings) =>
  invoke<Settings>("settings_set", { settings });

/** Emitted by Rust with the full new settings after every successful change. */
export const SETTINGS_CHANGED = "settings://changed";
