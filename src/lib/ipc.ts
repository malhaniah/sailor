import { invoke } from "@tauri-apps/api/core";
import type {
  AddonStreams,
  CatalogResponse,
  Descriptor,
  IpcError,
  MetaLookup,
  PlayerStatus,
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

export const playerStatus = () => invoke<PlayerStatus>("player_status");

export const playerPlay = (request: {
  url: string;
  title?: string | null;
  headers?: [string, string][];
  userAgent?: string | null;
  referrer?: string | null;
  startAtSec?: number | null;
}) => invoke<void>("player_play", { request });

export const playerStop = () => invoke<void>("player_stop");

export const settingsGet = () => invoke<Settings>("settings_get");
export const settingsSet = (settings: Settings) =>
  invoke<Settings>("settings_set", { settings });
