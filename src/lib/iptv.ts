// The user's IPTV sources (Rust: src-tauri/src/commands/iptv.rs). Addresses and accounts
// stay in Rust and the OS keychain; the webview sees names, hosts and channels, and a
// stream URL only for the channel it plays (through the one-shot player launch).
import { invoke } from "@tauri-apps/api/core";
import { type PlayerLaunch, openPlayer } from "./playback-handoff";
import type { PlaybackRequest } from "./player-ipc";

export type IptvKind = "m3u" | "xtream";

export interface IptvSource {
  id: string;
  name: string;
  kind: IptvKind;
  /** the server's host name, to tell sources apart */
  host: string;
  hasEpg: boolean;
  favorites: string[];
  /** where its address is kept; null when Zeiler can't find it any more */
  storage: "os" | "memory" | null;
}

export interface IptvSourceInput {
  name: string;
  kind: IptvKind;
  url?: string;
  epgUrl?: string;
  xtream?: { server: string; username: string; password: string };
  userAgent?: string;
}

export interface IptvChannel {
  id: string;
  name: string;
  logo: string | null;
  group: string | null;
  tvgId: string | null;
}

export interface IptvProgram {
  startMs: number;
  endMs: number;
  title: string;
  desc?: string;
  category?: string;
}

export const iptvSources = () => invoke<IptvSource[]>("iptv_sources");
export const iptvSourceAdd = (source: IptvSourceInput) =>
  invoke<IptvSource>("iptv_source_add", { source });
export const iptvSourceRemove = (id: string) => invoke<void>("iptv_source_remove", { id });
export const iptvChannels = (id: string, force = false) =>
  invoke<{ channels: IptvChannel[]; groups: string[]; hasEpg: boolean }>("iptv_channels", {
    id,
    force,
  });
export const iptvEpg = (id: string, tvgIds: string[], fromMs: number, toMs: number) =>
  invoke<Record<string, IptvProgram[]>>("iptv_epg", { id, tvgIds, fromMs, toMs });
export const iptvFavoriteSet = (id: string, channelId: string, on: boolean) =>
  invoke<string[]>("iptv_favorite_set", { id, channelId, on });
const iptvResolve = (sourceId: string, channelId: string) =>
  invoke<PlaybackRequest>("iptv_resolve", { sourceId, channelId });

/** Play a channel: its stream goes to the Player screen in a one-shot launch marked
 * live (never through the location hash, never tracked in the library). */
export async function playChannel(source: IptvSource, channel: IptvChannel): Promise<void> {
  const request = await iptvResolve(source.id, channel.id);
  const launch: PlayerLaunch = {
    source: {
      kind: "url",
      url: request.url,
      headers: request.headers ?? [],
      userAgent: request.userAgent ?? null,
      referrer: request.referrer ?? null,
    },
    title: channel.name,
    startAtSec: null,
    meta: null,
    video: null,
    stream: null,
    transportUrl: null,
    addonName: source.name,
    live: true,
    backend: "auto",
  };
  openPlayer(launch);
}
