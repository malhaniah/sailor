/**
 * Subtitles and audio for one playback, on either backend: the entries the menus list,
 * the automatic pick when a file starts, and the choices remembered per title.
 *
 * - **Sources.** Embedded tracks (mpv's `track-list`), the stream's own subtitles, every
 *   subtitle addon (`subtitles_get`, asked while the video opens when subtitles show by
 *   themselves, else when the menu first opens), files the user picks or drops. Addon
 *   and stream subtitles are listed but downloaded only when chosen (or picked
 *   automatically): Rust fetches, normalizes and adds them (`player_subtitle_add`).
 *   When the stream names no OpenSubtitles hash, the addons are asked once the file
 *   has loaded, with the hash Rust reads from the stream (`player_video_hash`).
 * - **Automatic pick** (`pickAutomatic` in lib/subtitles.ts), once per file, a moment
 *   after it loads (so mpv's own selection has arrived), language by language: for the
 *   first preferred language that has a full subtitle anywhere, what mpv chose if it is
 *   that language, else an embedded track, else the best candidate. While the addons are
 *   still answering it waits, unless it already has one in the first language. A
 *   candidate whose download fails gives way to the next (three tries). The title's
 *   remembered language comes first; "off", and automatic subtitles switched off, mean
 *   no pick (Rust already told mpv `sid=no`). A subtitle the user chose for the file
 *   keeps the pick out. The title's delay, speed and audio language apply at once.
 * - **Remembered per title** (`player_prefs_*`): the subtitle language picked (or off),
 *   where that subtitle came from (the next episode's pick prefers that source), and
 *   the audio language, when the user picks them; the subtitle delay and the speed
 *   whenever they change after the file settled (also from keys in the mpv window).
 * - Subtitles added to mpv go away with the next file, so the per-file state resets at
 *   `start-file`.
 * - **Another file.** External mpv lets the user open another file in its window
 *   (dropped in, or through their own bindings). As for Rust's progress tracker
 *   (library/progress.rs), a `start-file` after the launched file started is another
 *   video, unless the file before it ended in a redirect. Until Play again starts the
 *   launch anew, nothing here picks, applies or remembers the title's choices for it,
 *   and the title's stream and addon subtitles are not offered.
 * - **Next episode.** `launch` is the session's current one: when the next episode loads
 *   in the same player (a new run), the addons are asked again for its video, with its
 *   own hints, and its stream's subtitles are offered.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { PlayerLaunch } from "../../lib/playback-handoff";
import {
  type AddedSubtitle,
  errorMessage,
  type MpvTrack,
  onSubtitleDropped,
  playerCommand,
  playerPrefsGet,
  playerPrefsSet,
  playerSetProperty,
  playerSetSubStyle,
  playerSubtitleAdd,
  playerSubtitlePickLocal,
  playerSetAudioNormalize,
  playerSubtitleReencode,
  playerVideoHash,
  settingsGet,
  settingsPatch,
  subtitlesGet,
  type TitlePrefs,
} from "../../lib/player-ipc";
import {
  buildCandidates,
  channelLabel,
  codecLabel,
  effectiveLanguages,
  isMainSubtitle,
  langRank,
  languageName,
  normalizeLang,
  type AutomaticPick,
  pickAutomatic,
  type SubtitleCandidate,
} from "../../lib/subtitles";
import type { AddonSubtitles, AudioSettings, SubtitleSettings, SubtitleStyle } from "../../lib/types";
import type { MpvState } from "./mpv-state";
import type { PlayerEventListener } from "./usePlaybackSession";

/** How long after `file-loaded` the automatic pick waits for mpv's own selection. */
const SETTLE_MS = 400;
/** Delay and speed changes are saved once they rest this long. */
const SAVE_AFTER_MS = 1000;
/** How many subtitles an automatic pick downloads before giving up (each one failed). */
const MAX_AUTO_TRIES = 3;
/** How long a list waits for the file to load (for its hash) before asking without. */
const HASH_WAIT_MS = 20000;

export type SubtitleKind = "embedded" | "stream" | "addon" | "local";

export interface SubtitleEntry {
  key: string;
  kind: SubtitleKind;
  lang: string | null;
  /** "English", "English 2", a track or file title */
  title: string;
  /** small tags: Forced, SDH, Default, the format, the addon */
  tags: string[];
  /** mpv's track id once it is in the track list */
  trackId: number | null;
  selected: boolean;
  status: "ready" | "loading" | "error";
  error: string | null;
  /** an added file (can be read again as another encoding) and how it was read */
  file: string | null;
  encoding: string | null;
}

export interface AudioEntry {
  id: number;
  lang: string | null;
  title: string;
  tags: string[];
  selected: boolean;
}

export interface Tracks {
  /** another file than the launched one plays (opened in external mpv's own window) */
  foreign: boolean;
  /** subtitles already in the player (in the video or added), for the second subtitle */
  secondaryChoices: SubtitleEntry[];
  /** show a second subtitle above the first (null: none) */
  selectSecondary: (entry: SubtitleEntry | null) => void;
  /** volume normalization is on (null until settings load) */
  normalize: boolean | null;
  setNormalize: (on: boolean) => void;
  /** the loudest the volume slider goes, in percent */
  volumeMax: number;
  subtitles: SubtitleEntry[];
  /** the addons' list: not asked yet, being asked, answered */
  listing: "idle" | "loading" | "done";
  /** "<addon>: <reason>" for addons that failed */
  listErrors: string[];
  /** a subtitle is showing */
  enabled: boolean;
  selectSubtitle: (entry: SubtitleEntry) => void;
  subtitlesOff: () => void;
  subtitlesOn: () => void;
  pickLocal: () => void;
  /** read the showing (added) subtitle again as `encoding` (null: detect) */
  reencode: (encoding: string | null) => void;
  /** ask the subtitle addons now, if that has not happened (the menu opened) */
  ensureListed: () => void;
  audio: AudioEntry[];
  selectAudio: (entry: AudioEntry) => void;
  /** the saved subtitle style (null until settings load) */
  style: SubtitleStyle | null;
  /** apply style fields now and save them */
  setStyle: (patch: Partial<SubtitleStyle>) => void;
  /** a passing message (a dropped file was added, a pick failed…) */
  notice: string | null;
  dismissNotice: () => void;
}

interface Added {
  file: string;
  encoding: string;
}

export function useTracks({
  launch,
  mpv,
  running,
  subscribeEvents,
}: {
  launch: PlayerLaunch;
  mpv: MpvState;
  /** the player runs and takes commands (not loading, not gone) */
  running: boolean;
  subscribeEvents: (listener: PlayerEventListener) => () => void;
}): Tracks {
  const meta = launch.meta;
  const [settings, setSettings] = useState<{ subtitles: SubtitleSettings; audio: AudioSettings } | null>(null);
  const [titlePrefs, setTitlePrefs] = useState<TitlePrefs | null | undefined>(undefined);
  const [groups, setGroups] = useState<AddonSubtitles[]>([]);
  const [listing, setListing] = useState<Tracks["listing"]>("idle");
  // per file: candidates added (by candidate key), downloads in flight or failed, files
  // the user added themselves
  const [added, setAdded] = useState<Record<string, Added>>({});
  const [pending, setPending] = useState<Record<string, { error: string | null }>>({});
  const [local, setLocal] = useState<Record<string, Added>>({});
  const [fileSeq, setFileSeq] = useState(0);
  const [settledSeq, setSettledSeq] = useState(0);
  const [notice, setNotice] = useState<string | null>(null);
  // the file (by `fileSeq`) whose title choices, and whose automatic subtitle, are done
  // (-1: pick again, an automatic download failed)
  const [prefsDoneSeq, setPrefsDoneSeq] = useState(0);
  const [subsDoneSeq, setSubsDoneSeq] = useState(0);
  // the file in which the user chose a subtitle (or Off) themselves
  const userChose = useRef(0);
  // automatic downloads tried for the current file
  const autoTries = useRef(0);
  // another file plays in the player (see "Another file" above); the ref for callbacks
  const [foreign, setForeign] = useState(false);
  const foreignRef = useRef(false);
  // the player start (`run`) the events come from, whether its file started, and why
  // the last file ended
  const playback = useRef<{ run: number; fileStarted: boolean; lastEnd: string | null }>({
    run: 0,
    fileStarted: false,
    lastEnd: null,
  });
  const mounted = useRef(true);
  useEffect(
    () => () => {
      mounted.current = false;
    },
    [],
  );

  // the query for the subtitle addons: the video the picker asked streams for
  const query = useMemo(() => {
    if (!meta) return null;
    const hints = launch.stream?.behaviorHints;
    return {
      type: meta.type,
      id: launch.video?.id ?? meta.behaviorHints.defaultVideoId ?? meta.id,
      videoHash: hints?.videoHash ?? null,
      videoSize: hints?.videoSize ?? null,
      filename: hints?.filename ?? null,
    };
  }, [meta, launch.video, launch.stream]);

  // settings and the title's choices
  useEffect(() => {
    let live = true;
    settingsGet()
      .then((s) => live && setSettings({ subtitles: s.subtitles, audio: s.audio }))
      .catch(() => live && setSettings(null));
    if (meta) {
      playerPrefsGet(meta.id)
        .then((p) => live && setTitlePrefs(p))
        .catch(() => live && setTitlePrefs(null));
    } else {
      setTitlePrefs(null);
    }
    return () => {
      live = false;
    };
  }, [meta]);

  const listingRef = useRef<Tracks["listing"]>("idle");
  // The video whose file has loaded (by query), and lists waiting for that: without a
  // hash from the stream, the addons are asked once the file loaded, with Rust's.
  const queryKeyRef = useRef<string | null>(null);
  const loadedKey = useRef<string | null>(null);
  const loadWaiters = useRef(new Set<(gone?: boolean) => void>());
  // the page went away: no list waits for a file any more
  useEffect(
    () => () => {
      for (const check of [...loadWaiters.current]) check(true);
    },
    [],
  );
  const whenLoaded = useCallback((key: string | null) => {
    return new Promise<boolean>((resolve) => {
      if (loadedKey.current === key) return resolve(true);
      const timer = window.setTimeout(() => done(false), HASH_WAIT_MS);
      function done(loaded: boolean) {
        window.clearTimeout(timer);
        loadWaiters.current.delete(check);
        resolve(loaded);
      }
      function check(gone = false) {
        if (gone) done(false);
        else if (loadedKey.current === key) done(true);
      }
      loadWaiters.current.add(check);
    });
  }, []);
  // Another video (the next episode) has another list: forget the previous one, and an
  // answer for it that is still on its way. (Before the effect below that asks again.)
  const listedFor = useRef(0);
  const queryKey = query ? JSON.stringify(query) : null;
  queryKeyRef.current = queryKey;
  useEffect(() => {
    listedFor.current += 1;
    listingRef.current = "idle";
    setListing("idle");
    setGroups([]);
  }, [queryKey]);
  const list = useCallback(() => {
    if (!query || foreignRef.current || listingRef.current !== "idle") return;
    listingRef.current = "loading";
    setListing("loading");
    const asked = listedFor.current;
    const done = (result: AddonSubtitles[]) => {
      if (asked !== listedFor.current) return;
      listingRef.current = "done";
      if (!mounted.current) return;
      setGroups(result);
      setListing("done");
    };
    const withHash = async () => {
      if (query.videoHash) return query;
      // (a file that never loaded is not hashed: Rust would read the one before)
      const loaded = await whenLoaded(queryKey);
      const hashed =
        loaded && asked === listedFor.current ? await playerVideoHash().catch(() => null) : null;
      // the hash and the size of the same file, even if the stream named another size
      return hashed ? { ...query, videoHash: hashed.hash, videoSize: hashed.size } : query;
    };
    withHash()
      .then((q) => (asked === listedFor.current ? subtitlesGet(q) : []))
      .then(done)
      .catch((e) => {
        if (asked !== listedFor.current) return;
        done([]);
        if (mounted.current) setNotice(`Could not ask for subtitles: ${errorMessage(e)}`);
      });
  }, [query, queryKey, whenLoaded]);

  // Ask the addons while the video opens when a subtitle will show by itself (the
  // disclosure in Settings says when); otherwise only once the menu opens.
  const wantsSubtitles =
    titlePrefs !== undefined &&
    settings !== null &&
    titlePrefs?.subLang !== "off" &&
    (settings.subtitles.autoLoad || !!titlePrefs?.subLang);
  useEffect(() => {
    if (wantsSubtitles && !foreign) list();
  }, [wantsSubtitles, foreign, list]);

  // per-file state follows mpv's events
  useEffect(() => {
    const setForeignFile = (value: boolean) => {
      if (foreignRef.current === value) return;
      foreignRef.current = value;
      setForeign(value);
      if (value) setNotice("Another file opened in mpv: this title's subtitles and choices stay out of it.");
    };
    return subscribeEvents((event, run) => {
      if (playback.current.run !== run) {
        // Play again: the launched file plays anew
        playback.current = { run, fileStarted: false, lastEnd: null };
        setForeignFile(false);
      }
      const now = playback.current;
      if (event.type === "start-file") {
        if (now.fileStarted && now.lastEnd !== "redirect") setForeignFile(true);
        now.fileStarted = true;
        loadedKey.current = null;
        setAdded({});
        setPending({});
        setLocal({});
      } else if (event.type === "end-file") {
        now.lastEnd = event.reason;
      } else if (event.type === "file-loaded") {
        now.fileStarted = true; // (a missed start-file)
        autoTries.current = 0;
        setFileSeq((n) => n + 1);
        loadedKey.current = queryKeyRef.current;
        for (const check of [...loadWaiters.current]) check();
      }
    });
  }, [subscribeEvents]);
  useEffect(() => {
    if (fileSeq === 0) return;
    const timer = window.setTimeout(() => setSettledSeq(fileSeq), SETTLE_MS);
    return () => window.clearTimeout(timer);
  }, [fileSeq]);

  // files dropped on the window (Rust adds them while a player runs)
  useEffect(() => {
    const off = onSubtitleDropped((outcome) => {
      if ("added" in outcome) {
        setLocal((l) => ({ ...l, [outcome.added.file]: { file: outcome.added.file, encoding: outcome.added.encoding } }));
        setNotice(`Added ${outcome.name}`);
      } else {
        setNotice(`Could not add ${outcome.name}: ${outcome.error}`);
      }
    });
    return () => {
      void off.then((unlisten) => unlisten());
    };
  }, []);

  // --- the entries -----------------------------------------------------------------------

  const prefs = useMemo(() => {
    const remembered =
      !foreign && titlePrefs?.subLang && titlePrefs.subLang !== "off" ? [titlePrefs.subLang] : [];
    return effectiveLanguages([...remembered, ...(settings?.subtitles.langs ?? [])]);
  }, [foreign, titlePrefs, settings]);

  // the launch's subtitles (none for another file)
  const candidates = useMemo(
    () =>
      foreign
        ? []
        : buildCandidates({
            streamSubtitles: launch.stream?.subtitles ?? [],
            streamAddon: launch.addonName,
            groups,
            filename: launch.stream?.behaviorHints.filename ?? null,
            prefs,
          }),
    [foreign, launch.stream, launch.addonName, groups, prefs],
  );

  const subtitles = useMemo(
    () => subtitleEntries({ tracks: mpv.tracks, sid: mpv.sid, candidates, added, pending, local, prefs }),
    [mpv.tracks, mpv.sid, candidates, added, pending, local, prefs],
  );
  const audio = useMemo(() => audioEntries(mpv.tracks, mpv.aid), [mpv.tracks, mpv.aid]);

  // --- actions ---------------------------------------------------------------------------

  const remember = useCallback(
    (patch: Partial<Omit<TitlePrefs, "metaId">>) => {
      // choices made for another file are not the title's
      if (!meta || foreignRef.current) return;
      void playerPrefsSet(meta.id, patch)
        .then((p) => mounted.current && setTitlePrefs(p))
        .catch(() => undefined);
    },
    [meta],
  );

  const fileSeqRef = useRef(fileSeq);
  fileSeqRef.current = fileSeq;
  /** The user picked a subtitle (or Off) for this file: the automatic pick stays out. */
  const chose = useCallback(() => {
    userChose.current = fileSeqRef.current;
  }, []);

  const addCandidate = useCallback(
    (candidate: SubtitleCandidate, title: string, select: boolean, automatic: boolean) => {
      const forFile = fileSeqRef.current;
      setPending((p) => ({ ...p, [candidate.key]: { error: null } }));
      void playerSubtitleAdd({ url: candidate.url, lang: candidate.lang, label: title, select })
        .then((result: AddedSubtitle) => {
          if (!mounted.current) return;
          setAdded((a) => ({ ...a, [candidate.key]: { file: result.file, encoding: result.encoding } }));
          setPending(({ [candidate.key]: _, ...rest }) => rest);
        })
        .catch((e) => {
          if (!mounted.current) return;
          setPending((p) => ({ ...p, [candidate.key]: { error: errorMessage(e) } }));
          // an automatic pick whose download failed tries the next subtitle (a few
          // times at most), unless the file changed or the user chose meanwhile
          if (
            automatic &&
            forFile === fileSeqRef.current &&
            userChose.current !== forFile &&
            autoTries.current < MAX_AUTO_TRIES
          ) {
            setSubsDoneSeq(-1);
          }
        });
    },
    [],
  );

  const show = useCallback(
    (entry: SubtitleEntry, automatic = false) => {
      if (entry.trackId !== null) {
        void playerSetProperty("sid", entry.trackId).catch(() => undefined);
        return;
      }
      const candidate = candidates.find((c) => c.key === entry.key);
      if (candidate && entry.status !== "loading") addCandidate(candidate, entry.title, true, automatic);
    },
    [candidates, addCandidate],
  );

  const selectSubtitle = useCallback(
    (entry: SubtitleEntry) => {
      chose();
      show(entry);
      if (entry.lang) remember({ subLang: entry.lang, subSource: sourceOf(entry.kind) });
    },
    [chose, show, remember],
  );

  const subtitlesOff = useCallback(() => {
    chose();
    void playerSetProperty("sid", "no").catch(() => undefined);
    remember({ subLang: "off", subSource: null });
  }, [chose, remember]);

  const latest = useRef({ mpv, candidates, prefs, subtitles, pending, titlePrefs });
  latest.current = { mpv, candidates, prefs, subtitles, pending, titlePrefs };

  /** The automatic pick now, leaving out subtitles whose download failed for this file. */
  const currentPick = useCallback((): AutomaticPick => {
    const { mpv: now, candidates: offered, prefs: langs, pending: waits, titlePrefs: title } =
      latest.current;
    const remembered =
      !foreignRef.current && title?.subLang && title.subLang !== "off" && title.subSource
        ? { lang: title.subLang, source: title.subSource }
        : null;
    return pickAutomatic({
      tracks: now.tracks,
      sid: now.sid,
      candidates: offered.filter((c) => !waits[c.key]?.error),
      prefs: langs,
      remembered,
    });
  }, []);

  /** Show what a pick chose (an automatic one's failed download tries the next). */
  const showPick = useCallback(
    (pick: AutomaticPick, automatic: boolean) => {
      if (pick.kind === "track") {
        void playerSetProperty("sid", pick.id).catch(() => undefined);
      } else if (pick.kind === "candidate") {
        const entry = latest.current.subtitles.find((e) => e.key === pick.candidate.key);
        if (entry) show(entry, automatic);
      }
    },
    [show],
  );

  const subtitlesOn = useCallback(() => {
    chose();
    const pick = currentPick();
    let lang: string | null = null;
    let source: TitlePrefs["subSource"] = null;
    if (pick.kind !== "none") {
      showPick(pick, false);
      lang = pick.lang;
      source = pick.kind === "candidate" ? pick.candidate.source : pick.kind === "track" ? "embedded" : null;
    } else {
      // nothing in a preferred language: the first subtitle there is
      const entries = latest.current.subtitles;
      const first = entries.find((e) => e.status !== "error") ?? entries[0];
      if (first) {
        show(first);
        lang = first.lang;
        source = sourceOf(first.kind);
      }
    }
    if (lang) remember({ subLang: lang, subSource: source });
  }, [chose, currentPick, showPick, show, remember]);

  const pickLocal = useCallback(() => {
    void playerSubtitlePickLocal()
      .then((result) => {
        if (!result || !mounted.current) return;
        chose();
        setLocal((l) => ({ ...l, [result.file]: { file: result.file, encoding: result.encoding } }));
      })
      .catch((e) => mounted.current && setNotice(`Could not load the file: ${errorMessage(e)}`));
  }, [chose]);

  const reencode = useCallback(
    (encoding: string | null) => {
      const entry = latest.current.subtitles.find((e) => e.selected && e.file);
      if (!entry?.file) return;
      const oldTrack = entry.trackId;
      void playerSubtitleReencode(entry.file, encoding)
        .then((result) => {
          if (!mounted.current) return;
          const next = { file: result.file, encoding: result.encoding };
          if (entry.kind === "local") {
            setLocal((l) => {
              const { [entry.file ?? ""]: _, ...rest } = l;
              return { ...rest, [result.file]: next };
            });
          } else {
            setAdded((a) => ({ ...a, [entry.key]: next }));
          }
          if (oldTrack !== null) void playerCommand("sub-remove", oldTrack).catch(() => undefined);
        })
        .catch((e) => mounted.current && setNotice(`Could not read it that way: ${errorMessage(e)}`));
    },
    [],
  );

  const selectAudio = useCallback(
    (entry: AudioEntry) => {
      void playerSetProperty("aid", entry.id).catch(() => undefined);
      if (entry.lang) remember({ audioLang: entry.lang });
    },
    [remember],
  );

  const secondaryChoices = useMemo(
    () => subtitles.filter((e) => e.trackId !== null && !e.selected),
    [subtitles],
  );
  const selectSecondary = useCallback((entry: SubtitleEntry | null) => {
    void playerSetProperty("secondary-sid", entry?.trackId ?? "no").catch(() => undefined);
  }, []);

  const setNormalize = useCallback((on: boolean) => {
    // (saved either way: it applies from the next file on)
    void playerSetAudioNormalize(on).catch(
      (e) => mounted.current && setNotice(`Could not change the volume normalization now: ${errorMessage(e)}`),
    );
    setSettings((s) => (s ? { ...s, audio: { ...s.audio, normalize: on } } : s));
    void settingsPatch({ audio: { normalize: on } }).catch(() => undefined);
  }, []);

  const setStyle = useCallback((patch: Partial<SubtitleStyle>) => {
    void playerSetSubStyle(patch)
      .then((refused) => {
        if (refused.length > 0 && mounted.current) {
          setNotice(
            `This mpv cannot show that (${refused.join(", ")}). It is saved, and a newer mpv shows it.`,
          );
        }
      })
      .catch(() => undefined);
    setSettings((s) => (s ? { ...s, subtitles: { ...s.subtitles, style: { ...s.subtitles.style, ...patch } } } : s));
    void settingsPatch({ subtitles: { style: patch } }).catch(() => undefined);
  }, []);

  // --- once per file: the title's choices, then the automatic subtitle --------------------

  // Rust already gave mpv the title's languages and whether a subtitle may show (`slang`,
  // `alang`, `sid`). The delay, the speed and an audio track mpv missed come here, at once.
  // Another file's turn passes with nothing done (so Play again's file waits for its own).
  useEffect(() => {
    if (!running || settledSeq === 0 || titlePrefs === undefined) return;
    if (prefsDoneSeq === settledSeq) return;
    setPrefsDoneSeq(settledSeq);
    if (foreign) return;
    if (titlePrefs?.subDelaySec != null) {
      void playerSetProperty("sub-delay", titlePrefs.subDelaySec).catch(() => undefined);
    }
    if (titlePrefs?.rate != null) void playerSetProperty("speed", titlePrefs.rate).catch(() => undefined);
    const audioLang = titlePrefs?.audioLang;
    if (audioLang) {
      const tracks = latest.current.mpv.tracks.filter((t) => t.type === "audio");
      const current = tracks.find((t) => t.selected);
      const wanted = tracks.find((t) => normalizeLang(t.lang) === audioLang);
      if (wanted && normalizeLang(current?.lang) !== audioLang) {
        void playerSetProperty("aid", wanted.id).catch(() => undefined);
      }
    }
  }, [running, settledSeq, prefsDoneSeq, foreign, titlePrefs]);

  useEffect(() => {
    if (!running || settledSeq === 0 || !settings || titlePrefs === undefined) return;
    if (subsDoneSeq === settledSeq) return;
    if (foreign || userChose.current === settledSeq) {
      setSubsDoneSeq(settledSeq);
      return;
    }
    const choice = titlePrefs?.subLang;
    const automatic = choice ? choice !== "off" : settings.subtitles.autoLoad;
    if (!automatic) {
      // Rust asked mpv for no subtitle (`sid=no`); an external mpv's own scripts may differ
      if (choice === "off" && latest.current.mpv.sid !== "no") {
        void playerSetProperty("sid", "no").catch(() => undefined);
      }
      setSubsDoneSeq(settledSeq);
      return;
    }
    const pick = currentPick();
    // While the addons answer, only a subtitle in the first preferred language is sure to
    // be the best one; anything else waits for them. So does everything when the title's
    // subtitle came from an addon last time (it is likely among their answers).
    const addonRemembered = titlePrefs?.subSource === "addon";
    if (listing === "loading" && (pick.kind === "none" || pick.prefIndex > 0 || addonRemembered)) {
      return;
    }
    setSubsDoneSeq(settledSeq);
    if (pick.kind === "candidate") autoTries.current += 1;
    showPick(pick, true);
  }, [running, settledSeq, subsDoneSeq, foreign, settings, titlePrefs, listing, currentPick, showPick]);

  // delay and speed are remembered whenever they change once the file settled (the
  // launched file: another one's are not the title's)
  const settled = !foreign && running && settledSeq > 0 && prefsDoneSeq === settledSeq;
  useRemembered(settled, mpv.subDelay, titlePrefs?.subDelaySec ?? 0, (v) => remember({ subDelaySec: v === 0 ? null : v }));
  useRemembered(settled, mpv.speed, titlePrefs?.rate ?? 1, (v) => remember({ rate: v === 1 ? null : v }));

  return {
    foreign,
    secondaryChoices,
    selectSecondary,
    normalize: settings?.audio.normalize ?? null,
    setNormalize,
    volumeMax: settings?.audio.volumeMax ?? 130,
    subtitles,
    // (the addons were asked about the title, not another file)
    listing: foreign ? "idle" : listing,
    listErrors: foreign ? [] : groups.filter((g) => g.error).map((g) => `${g.addonName}: ${g.error}`),
    enabled: mpv.sid !== "no" && mpv.sid !== "false" && subtitles.some((e) => e.selected),
    selectSubtitle,
    subtitlesOff,
    subtitlesOn,
    pickLocal,
    reencode,
    ensureListed: list,
    audio,
    selectAudio,
    style: settings?.subtitles.style ?? null,
    setStyle,
    notice,
    dismissNotice: useCallback(() => setNotice(null), []),
  };
}

/** How a subtitle's source is remembered for the title (a file of the user's is not). */
function sourceOf(kind: SubtitleKind): TitlePrefs["subSource"] {
  return kind === "local" ? null : kind;
}

/** Calls `save(value)` once `value` rests at something other than `saved`. */
function useRemembered(active: boolean, value: number, saved: number, save: (value: number) => void) {
  const saveRef = useRef(save);
  saveRef.current = save;
  useEffect(() => {
    if (!active || Math.abs(value - saved) < 1e-6) return;
    const timer = window.setTimeout(() => saveRef.current(Math.round(value * 1000) / 1000), SAVE_AFTER_MS);
    return () => window.clearTimeout(timer);
  }, [active, value, saved]);
}

// --- building the lists ---------------------------------------------------------------------

const KIND_ORDER: Record<SubtitleKind, number> = { embedded: 0, local: 1, stream: 2, addon: 3 };

/** The file name an mpv track was added from, without its folder. */
function trackFile(track: MpvTrack): string | null {
  const path = track["external-filename"];
  return path ? (path.split(/[\\/]/).pop() ?? null) : null;
}

export function subtitleEntries({
  tracks,
  sid,
  candidates,
  added,
  pending,
  local,
  prefs,
}: {
  tracks: MpvTrack[];
  sid: string;
  candidates: SubtitleCandidate[];
  added: Record<string, Added>;
  pending: Record<string, { error: string | null }>;
  local: Record<string, Added>;
  prefs: string[];
}): SubtitleEntry[] {
  const subs = tracks.filter((t) => t.type === "sub");
  const isSelected = (t: MpvTrack) => isMainSubtitle(t, sid);
  const byFile = new Map<string, MpvTrack>();
  for (const t of subs) {
    const file = trackFile(t);
    if (t.external && file) byFile.set(file, t);
  }
  const claimed = new Set<string>();
  const entries: SubtitleEntry[] = [];

  for (const t of subs.filter((t) => !t.external)) {
    const lang = normalizeLang(t.lang);
    entries.push({
      key: `track|${t.id}`,
      kind: "embedded",
      lang,
      title: t.title?.trim() || languageName(lang),
      tags: [
        t.forced ? "Forced" : null,
        t["hearing-impaired"] ? "SDH" : null,
        t.default ? "Default" : null,
        codecLabel(t.codec),
      ].filter((x): x is string => !!x),
      trackId: t.id,
      selected: isSelected(t),
      status: "ready",
      error: null,
      file: null,
      encoding: null,
    });
  }

  for (const c of candidates) {
    const add = added[c.key];
    const track = add ? byFile.get(add.file) : undefined;
    if (add) claimed.add(add.file);
    const wait = pending[c.key];
    entries.push({
      key: c.key,
      kind: c.source,
      lang: c.lang,
      title: c.label ?? languageName(c.lang),
      tags: [c.hearingImpaired ? "SDH" : null, c.addonName ?? (c.source === "stream" ? "Stream" : null)].filter(
        (x): x is string => !!x,
      ),
      trackId: track?.id ?? null,
      selected: track ? isSelected(track) : false,
      status: wait ? (wait.error ? "error" : "loading") : "ready",
      error: wait?.error ?? null,
      file: add?.file ?? null,
      encoding: add?.encoding ?? null,
    });
  }

  // external tracks that are not one of the candidates: picked, dropped, or added by
  // the user's own mpv (external backend)
  for (const t of subs.filter((t) => t.external)) {
    const file = trackFile(t);
    if (file && claimed.has(file)) continue;
    const mine = file ? local[file] : undefined;
    const lang = normalizeLang(t.lang);
    entries.push({
      key: `file|${file ?? t.id}`,
      kind: "local",
      lang,
      title: t.title?.trim() || file || languageName(lang),
      tags: [lang ? languageName(lang) : null, "File"].filter((x): x is string => !!x),
      trackId: t.id,
      selected: isSelected(t),
      status: "ready",
      error: null,
      file: mine ? file : null,
      encoding: mine?.encoding ?? null,
    });
  }

  entries.sort(
    (a, b) =>
      langRank(a.lang, prefs) - langRank(b.lang, prefs) ||
      (a.lang ?? "~").localeCompare(b.lang ?? "~") ||
      KIND_ORDER[a.kind] - KIND_ORDER[b.kind],
  );
  // number the untitled ones of a language: "English", "English 2", …
  const counts = new Map<string, number>();
  for (const e of entries) {
    if (e.kind !== "addon" && e.kind !== "stream") continue;
    if (e.title !== languageName(e.lang)) continue;
    const n = (counts.get(e.title) ?? 0) + 1;
    counts.set(e.title, n);
    if (n > 1) e.title = `${e.title} ${n}`;
  }
  return entries;
}

export function audioEntries(tracks: MpvTrack[], aid: string): AudioEntry[] {
  return tracks
    .filter((t) => t.type === "audio")
    .map((t) => {
      const lang = normalizeLang(t.lang);
      const named = lang ? languageName(lang) : null;
      return {
        id: t.id,
        lang,
        title: named ?? t.title?.trim() ?? `Track ${t.id}`,
        tags: [
          named && t.title?.trim() ? t.title.trim() : null,
          channelLabel(t),
          codecLabel(t.codec),
          t.default ? "Default" : null,
          t.external ? "File" : null,
        ].filter((x): x is string => !!x),
        selected: String(t.id) === aid || t.selected === true,
      };
    });
}
