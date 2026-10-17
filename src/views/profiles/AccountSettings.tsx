import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, RefreshCw, TriangleAlert } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { ErrorNote, Modal, SectionCard, Spinner, ToggleRow } from "../../components/ui";
import {
  ACCOUNT_CHANGED,
  accountAddonsApply,
  accountAddonsPreview,
  accountLinkPoll,
  accountLinkStart,
  accountLoginPassword,
  accountLogout,
  accountSetSync,
  accountStatus,
  accountSyncNow,
  errorKind,
  errorMessage,
} from "../../lib/ipc";
import { useProfiles } from "../../lib/profiles";
import { useAddons } from "../../lib/store";
import type { AccountStatus, AddonsChoice, AddonsPreview, LinkCode } from "../../lib/types";
import { PRIMARY_BUTTON, SECONDARY_BUTTON, SMALL_BUTTON } from "../addons/common";

const PRIVACY =
  "While you are signed in, Zeiler sends Stremio (api.strem.io) your library (titles, progress, watched marks) and, if addon sync is on, your addons' addresses, including any keys inside configured ones. Your password, if you use it, goes only to api.strem.io. Nothing is sent anywhere else, and nothing while you are signed out. Zeiler is not affiliated with Stremio.";

function when(ms: number | null): string {
  if (!ms) return "never";
  const minutes = Math.round((Date.now() - ms) / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  return new Date(ms).toLocaleString();
}

/** Settings → Profiles & account: the active profile's Stremio account. */
export function AccountSettings() {
  const { state: profiles } = useProfiles();
  const [status, setStatus] = useState<AccountStatus | null>(null);
  const [signingIn, setSigningIn] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [confirmOut, setConfirmOut] = useState(false);
  const active = profiles?.profiles.find((p) => p.id === profiles.activeId);

  useEffect(() => {
    accountStatus()
      .then(setStatus)
      .catch((e) => setError(errorMessage(e)));
    const unlisten = listen<AccountStatus>(ACCOUNT_CHANGED, (e) => setStatus(e.payload));
    return () => {
      void unlisten.then((off) => off());
    };
  }, []);

  if (!status) return <Spinner label="Loading…" />;

  return (
    <SectionCard
      title="Stremio account"
      description={`Optional. Sign this profile${active ? ` (${active.name})` : ""} in to a Stremio account to keep its library, and if you like its addons, the same as in Stremio's own apps.`}
    >
      {!status.signedIn ? (
        <div className="flex flex-col gap-3">
          {status.lastError && <ErrorNote message={status.lastError} />}
          <div className="flex flex-wrap gap-3">
            <button className={PRIMARY_BUTTON} onClick={() => setSigningIn(true)}>
              Sign in to Stremio
            </button>
            <button
              className={SECONDARY_BUTTON}
              onClick={() => void openUrl("https://www.stremio.com/register").catch(() => {})}
            >
              <ExternalLink size={14} /> Create a Stremio account
            </button>
          </div>
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center gap-3 rounded-xl border border-edge-soft bg-canvas/40 px-4 py-3">
            <div className="min-w-0 flex-1">
              <div className="text-[12px] text-ink-subtle">Signed in as</div>
              <MaskedEmail email={status.user?.email ?? ""} />
            </div>
            {status.user?.premium && (
              <span className="rounded-full bg-accent/15 px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wider text-accent">
                Premium
              </span>
            )}
            <button className={SMALL_BUTTON} onClick={() => setConfirmOut(true)}>
              Sign out
            </button>
          </div>
          {status.storage === "memory" && (
            <p className="text-[12.5px] text-ink-muted">
              Kept until Zeiler quits: you didn't ask to be remembered, or this system's
              keychain isn't available.
            </p>
          )}
          {status.needsAddonChoice && <AddonQuestion />}
          <ToggleRow
            label="Sync my library"
            sub={`Continue Watching, the library and watched marks. Newest change wins per title, like Stremio's apps. Last synced ${when(status.lastLibrarySync)}.`}
            on={status.syncLibrary}
            onChange={(on) =>
              void accountSetSync({ library: on }).then(setStatus).catch((e) => setError(errorMessage(e)))
            }
          />
          {!status.needsAddonChoice && (
            <ToggleRow
              label="Sync my addons"
              sub={
                status.syncAddons
                  ? `Your Stremio account's addons are this profile's addons; installs, removals and reordering here go to your account. Last synced ${when(status.lastAddonsSync)}.`
                  : "Off. Turn on to use your Stremio account's addons here (you'll be asked how to combine the two lists)."
              }
              on={status.syncAddons}
              onChange={(on) => {
                if (on) {
                  // asks how to combine the two lists first
                  setStatus({ ...status, needsAddonChoice: true });
                } else {
                  void accountSetSync({ addons: false }).then(setStatus).catch((e) => setError(errorMessage(e)));
                }
              }}
            />
          )}
          {status.addonsLocked && (
            <p className="flex items-center gap-2 text-[12.5px] text-[oklch(0.75_0.12_25)]">
              <TriangleAlert size={14} /> Addon changes aren't sent to Stremio until Zeiler can
              read your account's addons again (Sync now retries).
            </p>
          )}
          {status.lastError && <ErrorNote message={status.lastError} />}
          <div>
            <button
              className={SMALL_BUTTON}
              disabled={syncing}
              onClick={async () => {
                setSyncing(true);
                setError(null);
                try {
                  await accountSyncNow();
                } catch (e) {
                  setError(errorMessage(e));
                } finally {
                  setSyncing(false);
                }
              }}
            >
              <RefreshCw size={14} className={syncing ? "animate-spin" : ""} /> Sync now
            </button>
          </div>
        </div>
      )}
      {error && <ErrorNote message={error} />}
      <p className="text-[12px] leading-relaxed text-ink-subtle">{PRIVACY}</p>
      {signingIn && <SignIn onClose={() => setSigningIn(false)} onDone={setStatus} />}
      {confirmOut && (
        <Modal title="Sign out of Stremio?" onClose={() => setConfirmOut(false)}>
          <p className="text-[13.5px] leading-relaxed text-ink-muted">
            This profile stops syncing. Its library and addons stay on this computer as they
            are now.
          </p>
          <div className="mt-6 flex justify-end gap-3">
            <button className={SECONDARY_BUTTON} onClick={() => setConfirmOut(false)}>
              Cancel
            </button>
            <button
              className={PRIMARY_BUTTON}
              onClick={async () => {
                setConfirmOut(false);
                try {
                  setStatus(await accountLogout());
                } catch (e) {
                  setError(errorMessage(e));
                }
              }}
            >
              Sign out
            </button>
          </div>
        </Modal>
      )}
    </SectionCard>
  );
}

function MaskedEmail({ email }: { email: string }) {
  const [shown, setShown] = useState(false);
  const [name, domain] = email.split("@");
  const masked = name ? `${name.slice(0, 1)}•••@${domain ?? ""}` : email;
  return (
    <button
      className="truncate text-left text-[14px] font-medium text-ink"
      title={shown ? "Hide" : "Show"}
      onClick={() => setShown((s) => !s)}
    >
      {shown ? email : masked}
    </button>
  );
}

/** Sign-in: a code confirmed in the browser (recommended; the only way for accounts
 * made with Facebook or Apple), or email and password. */
function SignIn({ onClose, onDone }: { onClose: () => void; onDone: (s: AccountStatus) => void }) {
  const [tab, setTab] = useState<"link" | "password">("link");
  const [remember, setRemember] = useState(true);
  const [error, setError] = useState<string | null>(null);
  return (
    <Modal title="Sign in to Stremio" onClose={onClose} wide>
      <div role="group" className="mb-4 flex gap-1 border-b border-edge-soft">
        {(
          [
            ["link", "With your browser"],
            ["password", "Email & password"],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            aria-pressed={tab === id}
            onClick={() => {
              setTab(id);
              setError(null);
            }}
            className={`-mb-px border-b-2 px-3 pb-2 text-[13.5px] font-medium ${
              tab === id ? "border-accent text-ink" : "border-transparent text-ink-muted hover:text-ink"
            }`}
          >
            {label}
          </button>
        ))}
      </div>
      {tab === "link" ? (
        <LinkSignIn remember={remember} onError={setError} onDone={onDone} onClose={onClose} />
      ) : (
        <PasswordSignIn remember={remember} onError={setError} onDone={onDone} onClose={onClose} />
      )}
      <label className="mt-4 flex items-center gap-2 text-[12.5px] text-ink-muted">
        <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
        Stay signed in (kept in this computer's keychain)
      </label>
      {error && (
        <div className="mt-3">
          <ErrorNote message={error} />
        </div>
      )}
    </Modal>
  );
}

const POLL_MS = 3000;
const POLL_FOR_MS = 5 * 60 * 1000;

function LinkSignIn({
  remember,
  onError,
  onDone,
  onClose,
}: {
  remember: boolean;
  onError: (e: string | null) => void;
  onDone: (s: AccountStatus) => void;
  onClose: () => void;
}) {
  const [code, setCode] = useState<LinkCode | null>(null);
  const [expired, setExpired] = useState(false);
  const rememberRef = useRef(remember);
  rememberRef.current = remember;

  async function start() {
    onError(null);
    setExpired(false);
    try {
      setCode(await accountLinkStart());
    } catch (e) {
      onError(errorMessage(e));
    }
  }

  useEffect(() => {
    void start();
    // one code per open; polling below follows it
  }, []);

  // the callbacks change identity on every render of the page (an account event
  // re-renders it); the poll follows the code alone
  const callbacks = useRef({ onDone, onClose, onError });
  callbacks.current = { onDone, onClose, onError };

  useEffect(() => {
    if (!code) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const until = Date.now() + POLL_FOR_MS;
    const tick = async () => {
      if (stopped) return;
      if (Date.now() > until) {
        setExpired(true);
        return;
      }
      try {
        const status = await accountLinkPoll(code.code, rememberRef.current);
        if (status) {
          // signed in: never poll this code again, even if this view is gone
          stopped = true;
          callbacks.current.onDone(status);
          callbacks.current.onClose();
          return;
        }
        callbacks.current.onError(null);
      } catch (e) {
        // a network hiccup: keep waiting (the code is still good); anything else ends it
        if (errorKind(e) !== "network") {
          if (!stopped) {
            callbacks.current.onError(errorMessage(e));
            setExpired(true);
          }
          return;
        }
        callbacks.current.onError(`${errorMessage(e)} Still trying…`);
      }
      if (!stopped) timer = setTimeout(() => void tick(), POLL_MS);
    };
    timer = setTimeout(() => void tick(), POLL_MS);
    return () => {
      stopped = true;
      if (timer) clearTimeout(timer);
    };
  }, [code]);

  if (!code) return <Spinner label="Getting a sign-in code…" />;
  return (
    <div className="flex flex-col items-center gap-4 text-center">
      <p className="text-[13.5px] leading-relaxed text-ink-muted">
        Open the link, sign in to Stremio there and confirm this code. Your password never
        reaches Zeiler.
      </p>
      <div className="font-mono text-[32px] font-semibold tracking-[0.3em] text-ink">{code.code}</div>
      {code.qrcode && <img src={code.qrcode} alt="" className="h-36 w-36 rounded-xl bg-white p-2" />}
      {expired ? (
        <button className={PRIMARY_BUTTON} onClick={() => void start()}>
          Get a new code
        </button>
      ) : (
        <>
          <button className={PRIMARY_BUTTON} onClick={() => void openUrl(code.link).catch(() => {})}>
            <ExternalLink size={14} /> Open the sign-in page
          </button>
          <span className="flex items-center gap-2 text-[12px] text-ink-subtle">
            <Spinner /> Waiting for you to confirm…
          </span>
        </>
      )}
    </div>
  );
}

function PasswordSignIn({
  remember,
  onError,
  onDone,
  onClose,
}: {
  remember: boolean;
  onError: (e: string | null) => void;
  onDone: (s: AccountStatus) => void;
  onClose: () => void;
}) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={async (e) => {
        e.preventDefault();
        setBusy(true);
        onError(null);
        try {
          const status = await accountLoginPassword(email, password, remember);
          setPassword("");
          onDone(status);
          onClose();
        } catch (err) {
          onError(errorMessage(err));
        } finally {
          setBusy(false);
        }
      }}
    >
      <input
        type="email"
        autoComplete="username"
        placeholder="Email"
        value={email}
        onChange={(e) => setEmail(e.target.value)}
        className="h-11 rounded-xl border border-edge bg-canvas px-3 text-[14px] text-ink outline-none focus:border-ink"
      />
      <input
        type="password"
        autoComplete="current-password"
        placeholder="Password"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        className="h-11 rounded-xl border border-edge bg-canvas px-3 text-[14px] text-ink outline-none focus:border-ink"
      />
      <p className="text-[12px] text-ink-subtle">
        Sent only to api.strem.io. Accounts made with Facebook or Apple sign in with the
        browser instead.
      </p>
      <button type="submit" className={PRIMARY_BUTTON} disabled={busy || !email.trim() || !password}>
        {busy ? <Spinner /> : "Sign in"}
      </button>
    </form>
  );
}

const CHOICES: { id: AddonsChoice; label: string; sub: string }[] = [
  {
    id: "useAccount",
    label: "Use my Stremio addons",
    sub: "This profile's addons are replaced by your account's.",
  },
  {
    id: "merge",
    label: "Combine them",
    sub: "Your account's addons, plus this profile's that it lacks (uploaded to your account, keys included).",
  },
  {
    id: "keepLocal",
    label: "Don't sync addons",
    sub: "Keep this profile's addons; only the library syncs.",
  },
];

/** The sign-in's addon question: how the account's and the profile's lists combine. */
function AddonQuestion() {
  const { reload } = useAddons();
  const [preview, setPreview] = useState<AddonsPreview | null>(null);
  const [busy, setBusy] = useState<AddonsChoice | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    accountAddonsPreview()
      .then(setPreview)
      .catch((e) => setError(errorMessage(e)));
  }, []);
  return (
    <div className="flex flex-col gap-3 rounded-xl border border-accent/30 bg-accent/5 px-4 py-4">
      <div className="text-[14px] font-medium text-ink">Your addons</div>
      {preview ? (
        <div className="grid grid-cols-1 gap-2 text-[12.5px] text-ink-muted md:grid-cols-3">
          <List title="Only in your account" names={preview.accountOnly} />
          <List title="Only on this profile" names={preview.localOnly} />
          <List title="In both" names={preview.both} />
        </div>
      ) : (
        !error && <Spinner label="Reading your account's addons…" />
      )}
      <div className="flex flex-col gap-2">
        {CHOICES.map((choice) => (
          <button
            key={choice.id}
            disabled={busy !== null}
            onClick={async () => {
              setBusy(choice.id);
              setError(null);
              try {
                await accountAddonsApply(choice.id);
                await reload();
              } catch (e) {
                setError(errorMessage(e));
              } finally {
                setBusy(null);
              }
            }}
            className="flex flex-col items-start gap-0.5 rounded-xl border border-edge-soft bg-canvas/40 px-4 py-3 text-left hover:border-edge disabled:opacity-50"
          >
            <span className="text-[13.5px] font-medium text-ink">
              {busy === choice.id ? "Working…" : choice.label}
            </span>
            <span className="text-[12px] text-ink-muted">{choice.sub}</span>
          </button>
        ))}
      </div>
      <p className="text-[12px] text-ink-subtle">
        Zeiler's built-in addons stay under Settings → Content and are never added to your
        account.
      </p>
      {error && <ErrorNote message={error} />}
    </div>
  );
}

function List({ title, names }: { title: string; names: string[] }) {
  return (
    <div>
      <div className="mb-1 font-semibold text-ink">{title}</div>
      {names.length === 0 ? (
        <div className="text-ink-subtle">none</div>
      ) : (
        <ul className="list-disc pl-4">
          {names.slice(0, 8).map((n) => (
            <li key={n} className="truncate">
              {n}
            </li>
          ))}
          {names.length > 8 && <li>and {names.length - 8} more</li>}
        </ul>
      )}
    </div>
  );
}
