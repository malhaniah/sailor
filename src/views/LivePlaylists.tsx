// The Live room's Playlists tab: the user's own IPTV sources. The layout (sources, a
// group list, a channel grid with now/next) follows Harbor's src/views/live/*
// (Copyright (c) 2026 Harbor, MIT; see THIRD-PARTY-NOTICES.md).
import { Plus, RefreshCw, Search, Star, Trash2, TriangleAlert } from "lucide-react";
import { type FormEvent, type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { EmptyState, Modal, Spinner } from "../components/ui";
import { errorMessage } from "../lib/ipc";
import {
  type IptvChannel,
  type IptvKind,
  type IptvProgram,
  type IptvSource,
  iptvChannels,
  iptvEpg,
  iptvFavoriteSet,
  iptvSourceAdd,
  iptvSourceRemove,
  iptvSources,
  playChannel,
} from "../lib/iptv";

/** Channels drawn at once; search or a group narrows the rest. */
const GRID_MAX = 300;
const ALL = "\u0000all";
const FAVORITES = "\u0000favorites";

export function LivePlaylists() {
  const [sources, setSources] = useState<IptvSource[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<IptvSource | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await iptvSources();
      setError(null);
      setSources(list);
      setSelected((current) =>
        current && list.some((s) => s.id === current) ? current : (list[0]?.id ?? null),
      );
    } catch (e) {
      setError(errorMessage(e));
      setSources([]);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (sources === null) return <Spinner label="Loading your playlists…" />;
  const source = sources.find((s) => s.id === selected) ?? null;

  return (
    <div className="flex flex-col gap-6">
      {error && <p className="text-[13px] text-danger">{error}</p>}
      {sources.length === 0 ? (
        <EmptyState title="No playlists yet">
          Add an M3U playlist or an Xtream Codes account you have access to. Zeiler ships no
          channels: what you add, and whether you may watch it, is up to you.
          <div className="mt-5">
            <button
              onClick={() => setAdding(true)}
              className="h-11 rounded-full bg-ink px-6 text-[14px] font-semibold text-canvas transition-transform hover:scale-[1.03]"
            >
              Add a playlist
            </button>
          </div>
        </EmptyState>
      ) : (
        <div className="no-scrollbar flex gap-2 overflow-x-auto py-1">
          {sources.map((s) => (
            <button
              key={s.id}
              onClick={() => setSelected(s.id)}
              aria-pressed={s.id === selected}
              className={`flex h-10 shrink-0 items-center gap-2 rounded-full border px-4 text-[13px] font-medium transition-colors ${
                s.id === selected
                  ? "border-ink bg-ink text-canvas"
                  : "border-edge-soft text-ink-muted hover:border-ink hover:text-ink"
              }`}
            >
              {s.name}
              <span className="opacity-60">· {s.host}</span>
              {s.storage !== "os" && <TriangleAlert size={13} aria-label="Not saved" />}
            </button>
          ))}
          <button
            onClick={() => setAdding(true)}
            className="flex h-10 shrink-0 items-center gap-1.5 rounded-full border border-dashed border-edge px-4 text-[13px] font-medium text-ink-muted hover:border-ink hover:text-ink"
          >
            <Plus size={14} /> Add
          </button>
        </div>
      )}

      {source && (
        <SourceChannels
          key={source.id}
          source={source}
          onRemove={() => setRemoving(source)}
          onFavorites={(favorites) =>
            setSources((list) =>
              list ? list.map((s) => (s.id === source.id ? { ...s, favorites } : s)) : list,
            )
          }
        />
      )}

      {adding && (
        <AddSource
          onClose={() => setAdding(false)}
          onAdded={(added) => {
            setAdding(false);
            setSelected(added.id);
            void load();
          }}
        />
      )}
      {removing && (
        <Modal title={`Remove ${removing.name}?`} onClose={() => setRemoving(null)}>
          <p className="text-[14px] leading-relaxed text-ink-muted">
            Its address and account are deleted from this computer's keychain, and its
            favourites are forgotten.
          </p>
          <div className="mt-6 flex justify-end gap-2">
            <button
              onClick={() => setRemoving(null)}
              className="h-10 rounded-full border border-edge-soft px-5 text-[13px] font-medium text-ink-muted hover:border-ink hover:text-ink"
            >
              Cancel
            </button>
            <button
              onClick={async () => {
                const id = removing.id;
                setRemoving(null);
                try {
                  await iptvSourceRemove(id);
                } catch (e) {
                  setError(errorMessage(e));
                }
                void load();
              }}
              className="h-10 rounded-full bg-danger/80 px-5 text-[13px] font-semibold text-canvas"
            >
              Remove
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}

function SourceChannels({
  source,
  onRemove,
  onFavorites,
}: {
  source: IptvSource;
  onRemove: () => void;
  onFavorites: (favorites: string[]) => void;
}) {
  const [channels, setChannels] = useState<IptvChannel[] | null>(null);
  const [groups, setGroups] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [group, setGroup] = useState(ALL);
  const [query, setQuery] = useState("");
  const [guide, setGuide] = useState<Record<string, IptvProgram[]>>({});
  const [playError, setPlayError] = useState<string | null>(null);
  const [reloading, setReloading] = useState(false);
  // whether the source has a guide, as of its last channel load
  const [hasEpg, setHasEpg] = useState(source.hasEpg);

  const load = useCallback(
    async (force: boolean) => {
      setError(null);
      try {
        const list = await iptvChannels(source.id, force);
        setChannels(list.channels);
        setGroups(list.groups);
        setHasEpg(list.hasEpg);
      } catch (e) {
        setError(errorMessage(e));
        setChannels((current) => current ?? []);
      }
    },
    [source.id],
  );

  useEffect(() => {
    void load(false);
  }, [load]);

  const favorites = useMemo(() => new Set(source.favorites), [source.favorites]);
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (channels ?? []).filter(
      (c) =>
        (group === ALL || (group === FAVORITES ? favorites.has(c.id) : c.group === group)) &&
        (!q || c.name.toLowerCase().includes(q)),
    );
  }, [channels, group, query, favorites]);
  const visible = shown.slice(0, GRID_MAX);

  // what is on now and next, for the channels on screen
  const tvgKey = visible
    .map((c) => c.tvgId)
    .filter(Boolean)
    .join("\n");
  useEffect(() => {
    if (!hasEpg || !tvgKey) return;
    let cancelled = false;
    // after typing settles (Rust downloads a guide once per source and keeps it)
    const timer = setTimeout(() => {
      const now = Date.now();
      iptvEpg(source.id, tvgKey.split("\n"), now, now + 3 * 3600 * 1000)
        .then((programs) => {
          if (!cancelled) setGuide((g) => ({ ...g, ...programs }));
        })
        // a guide that fails to load leaves the channels as they are
        .catch(() => undefined);
    }, 400);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [source.id, hasEpg, tvgKey]);

  if (channels === null && !error) return <Spinner label="Loading channels…" />;

  return (
    <div className="flex flex-col gap-4">
      {source.storage !== "os" && (
        <p className="flex items-center gap-2 rounded-xl bg-elevated/60 px-4 py-2.5 text-[13px] text-ink-muted">
          <TriangleAlert size={15} className="shrink-0" />
          {source.storage === "memory"
            ? "The system keychain wasn't available, so this playlist's address is kept only until Zeiler quits."
            : "Zeiler can't find this playlist's address in the system keychain any more. Remove it and add it again."}
        </p>
      )}
      <div className="flex flex-wrap items-center gap-3">
        <label className="flex h-10 min-w-[220px] flex-1 items-center gap-2 rounded-full border border-edge-soft bg-elevated/40 px-4">
          <Search size={15} className="text-ink-subtle" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={`Search ${channels?.length ?? 0} channels`}
            aria-label="Search channels"
            className="h-full flex-1 bg-transparent text-[14px] text-ink outline-none placeholder:text-ink-subtle"
          />
        </label>
        <button
          onClick={async () => {
            setReloading(true);
            await load(true);
            setReloading(false);
          }}
          disabled={reloading}
          className="flex h-10 items-center gap-1.5 rounded-full border border-edge-soft px-4 text-[13px] font-medium text-ink-muted hover:border-ink hover:text-ink disabled:opacity-50"
        >
          <RefreshCw size={14} className={reloading ? "animate-spin" : ""} /> Refresh
        </button>
        <button
          onClick={onRemove}
          className="flex h-10 items-center gap-1.5 rounded-full border border-edge-soft px-4 text-[13px] font-medium text-ink-muted hover:border-danger hover:text-danger"
        >
          <Trash2 size={14} /> Remove
        </button>
      </div>
      {error && <p className="text-[13px] text-danger">{error}</p>}
      {playError && <p className="text-[13px] text-danger">{playError}</p>}

      <div className="grid grid-cols-[180px_minmax(0,1fr)] gap-6">
        <nav aria-label="Channel groups" className="flex max-h-[70vh] flex-col gap-1 overflow-y-auto pr-1">
          <GroupButton on={group === ALL} onClick={() => setGroup(ALL)}>
            All channels
          </GroupButton>
          <GroupButton on={group === FAVORITES} onClick={() => setGroup(FAVORITES)}>
            Favourites ({favorites.size})
          </GroupButton>
          {groups.map((g) => (
            <GroupButton key={g} on={group === g} onClick={() => setGroup(g)}>
              {g}
            </GroupButton>
          ))}
        </nav>
        <div className="flex flex-col gap-3">
          {visible.length === 0 ? (
            <p className="py-10 text-center text-[13px] text-ink-subtle">No channels here.</p>
          ) : (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-3">
              {visible.map((channel) => (
                <ChannelCard
                  key={channel.id}
                  channel={channel}
                  programs={channel.tvgId ? guide[channel.tvgId] : undefined}
                  favorite={favorites.has(channel.id)}
                  onPlay={() => {
                    setPlayError(null);
                    playChannel(source, channel).catch((e) => setPlayError(errorMessage(e)));
                  }}
                  onFavorite={async () => {
                    try {
                      onFavorites(await iptvFavoriteSet(source.id, channel.id, !favorites.has(channel.id)));
                    } catch (e) {
                      setPlayError(errorMessage(e));
                    }
                  }}
                />
              ))}
            </div>
          )}
          {shown.length > GRID_MAX && (
            <p className="text-center text-[12px] text-ink-subtle">
              Showing {GRID_MAX} of {shown.length}: search or pick a group to see the others.
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

function GroupButton({ on, onClick, children }: { on: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      onClick={onClick}
      aria-pressed={on}
      className={`truncate rounded-lg px-3 py-2 text-left text-[13px] transition-colors ${
        on ? "bg-elevated text-ink" : "text-ink-muted hover:bg-elevated/50 hover:text-ink"
      }`}
    >
      {children}
    </button>
  );
}

function ChannelCard({
  channel,
  programs,
  favorite,
  onPlay,
  onFavorite,
}: {
  channel: IptvChannel;
  programs?: IptvProgram[];
  favorite: boolean;
  onPlay: () => void;
  onFavorite: () => void;
}) {
  const now = Date.now();
  const current = programs?.find((p) => p.startMs <= now && p.endMs > now);
  const next = programs?.find((p) => p.startMs >= (current?.endMs ?? now));
  const time = (ms: number) =>
    new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return (
    <div className="group relative flex items-center gap-3 rounded-xl border border-edge-soft bg-elevated/40 p-3 transition-colors hover:border-ink/40">
      <button onClick={onPlay} className="flex min-w-0 flex-1 items-center gap-3 text-left" title={`Play ${channel.name}`}>
        <span className="flex h-12 w-16 shrink-0 items-center justify-center overflow-hidden rounded-lg bg-canvas/60">
          {channel.logo ? (
            <img src={channel.logo} alt="" loading="lazy" className="max-h-full max-w-full object-contain" />
          ) : (
            <span className="text-[13px] font-semibold text-ink-subtle">
              {channel.name.slice(0, 3).toUpperCase()}
            </span>
          )}
        </span>
        <span className="flex min-w-0 flex-col">
          <span className="truncate text-[14px] font-medium text-ink">{channel.name}</span>
          {current ? (
            <span className="truncate text-[12px] text-ink-muted">Now: {current.title}</span>
          ) : (
            channel.group && <span className="truncate text-[12px] text-ink-subtle">{channel.group}</span>
          )}
          {next && (
            <span className="truncate text-[11.5px] text-ink-subtle">
              {time(next.startMs)} {next.title}
            </span>
          )}
        </span>
      </button>
      <button
        onClick={onFavorite}
        aria-pressed={favorite}
        aria-label={favorite ? `Remove ${channel.name} from favourites` : `Add ${channel.name} to favourites`}
        className={`shrink-0 rounded-full p-1.5 transition-colors ${
          favorite ? "text-accent" : "text-ink-subtle opacity-0 hover:text-ink group-hover:opacity-100 focus:opacity-100"
        }`}
      >
        <Star size={16} fill={favorite ? "currentColor" : "none"} />
      </button>
    </div>
  );
}

function AddSource({
  onClose,
  onAdded,
}: {
  onClose: () => void;
  onAdded: (source: IptvSource) => void;
}) {
  const [kind, setKind] = useState<IptvKind>("m3u");
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [epgUrl, setEpgUrl] = useState("");
  const [server, setServer] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [userAgent, setUserAgent] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const added = await iptvSourceAdd({
        name: name.trim() || (kind === "m3u" ? "My playlist" : "My Xtream account"),
        kind,
        url: kind === "m3u" ? url : undefined,
        xtream: kind === "xtream" ? { server, username, password } : undefined,
        epgUrl: epgUrl || undefined,
        userAgent: userAgent || undefined,
      });
      onAdded(added);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title="Add a playlist" onClose={onClose} wide>
      <form onSubmit={submit} className="flex flex-col gap-4">
        <div className="flex gap-2" role="radiogroup" aria-label="Kind">
          {(["m3u", "xtream"] as const).map((k) => (
            <button
              type="button"
              key={k}
              role="radio"
              aria-checked={kind === k}
              onClick={() => setKind(k)}
              className={`h-9 rounded-full border px-4 text-[13px] font-medium ${
                kind === k ? "border-ink bg-ink text-canvas" : "border-edge-soft text-ink-muted hover:border-ink"
              }`}
            >
              {k === "m3u" ? "M3U playlist" : "Xtream Codes"}
            </button>
          ))}
        </div>
        <Field label="Name">
          <input value={name} onChange={(e) => setName(e.target.value)} maxLength={60} placeholder="My playlist" className={inputClass} />
        </Field>
        {kind === "m3u" ? (
          <Field label="Playlist address (M3U)">
            <input value={url} onChange={(e) => setUrl(e.target.value)} required type="url" placeholder="https://…/playlist.m3u" className={inputClass} />
          </Field>
        ) : (
          <>
            <Field label="Server">
              <input value={server} onChange={(e) => setServer(e.target.value)} required type="url" placeholder="http://provider.example:8080" className={inputClass} />
            </Field>
            <div className="grid grid-cols-2 gap-3">
              <Field label="Username">
                <input value={username} onChange={(e) => setUsername(e.target.value)} required autoComplete="off" className={inputClass} />
              </Field>
              <Field label="Password">
                <input value={password} onChange={(e) => setPassword(e.target.value)} required type="password" autoComplete="off" className={inputClass} />
              </Field>
            </div>
          </>
        )}
        <Field label="Programme guide (XMLTV), optional">
          <input value={epgUrl} onChange={(e) => setEpgUrl(e.target.value)} type="url" placeholder={kind === "xtream" ? "The server's own guide is used when empty" : "https://…/guide.xml.gz"} className={inputClass} />
        </Field>
        <details className="text-[13px] text-ink-muted">
          <summary className="cursor-pointer">Advanced</summary>
          <div className="mt-3">
            <Field label="User agent (some providers only answer certain players)">
              <input value={userAgent} onChange={(e) => setUserAgent(e.target.value)} maxLength={256} placeholder="Zeiler's own" className={inputClass} />
            </Field>
          </div>
        </details>
        <p className="text-[12px] leading-relaxed text-ink-subtle">
          Zeiler ships no channels: add only sources you are allowed to watch. The
          addresses and the account are kept in this computer's keychain, not in Zeiler's
          files. Zeiler loads the channel list once now to check it.
        </p>
        {error && <p className="text-[13px] text-danger">{error}</p>}
        <div className="flex justify-end gap-2">
          <button type="button" onClick={onClose} className="h-10 rounded-full border border-edge-soft px-5 text-[13px] font-medium text-ink-muted hover:border-ink hover:text-ink">
            Cancel
          </button>
          <button type="submit" disabled={busy} className="h-10 rounded-full bg-ink px-5 text-[13px] font-semibold text-canvas disabled:opacity-60">
            {busy ? "Checking…" : "Add"}
          </button>
        </div>
      </form>
    </Modal>
  );
}

const inputClass =
  "h-10 w-full rounded-xl border border-edge-soft bg-canvas/60 px-3 text-[14px] text-ink outline-none placeholder:text-ink-subtle focus:border-ink";

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1.5 text-[12.5px] font-medium text-ink-muted">
      {label}
      {children}
    </label>
  );
}
