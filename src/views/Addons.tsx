import { useState } from "react";
import { ErrorNote } from "../components/ui";
import { requestInstall } from "../lib/install-request";
import { BrowseTab } from "./addons/BrowseTab";
import { BuiltInTab } from "./addons/BuiltInTab";
import { InstalledTab } from "./addons/InstalledTab";

type Tab = "installed" | "builtIn" | "browse";

const TABS: { id: Tab; label: string }[] = [
  { id: "installed", label: "Installed" },
  { id: "builtIn", label: "Built-in" },
  { id: "browse", label: "Browse" },
];

// the tab survives leaving the page (the detail frame unmounts nothing, Home does)
let lastTab: Tab = "installed";

export function AddonsView() {
  const [tab, setTabState] = useState<Tab>(lastTab);
  const [url, setUrl] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const setTab = (next: Tab) => {
    lastTab = next;
    setTabState(next);
  };

  return (
    <main className="flex-1 overflow-y-auto px-12 pb-20 pt-28">
      <div className="mx-auto flex max-w-4xl flex-col gap-8">
        <header className="flex flex-col gap-2">
          <span className="text-[11px] font-bold uppercase tracking-[0.42em] text-ink-subtle">
            Collections
          </span>
          <h1 className="font-display text-[44px] font-medium leading-[1.05] tracking-tight text-ink">
            Addons
          </h1>
          <p className="max-w-2xl text-[15px] leading-relaxed text-ink-muted">
            Zeiler ships no stream addons. Paste the address of an addon you trust (it ends in{" "}
            <code className="rounded-md bg-elevated px-1.5 py-0.5 font-mono text-[12.5px]">
              /manifest.json
            </code>
            ; stremio:// links and setup-page links work too), or browse the directory.
          </p>
        </header>

        <form
          className="flex gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            const input = url.trim();
            if (!input) return;
            setNote(null);
            if (requestInstall({ url: input, source: "pasted" })) setUrl("");
            else setNote("Finish or cancel the open install first.");
          }}
        >
          <input
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://example.com/manifest.json"
            className="h-12 w-full rounded-xl border border-edge bg-canvas px-4 text-[14px] text-ink outline-none placeholder:text-ink-subtle focus:border-ink"
          />
          <button
            type="submit"
            disabled={!url.trim()}
            className="flex h-12 shrink-0 items-center rounded-xl bg-ink px-6 text-[13.5px] font-semibold text-canvas transition-transform hover:scale-[1.02] active:scale-[0.97] disabled:opacity-50"
          >
            Add addon
          </button>
        </form>
        {note && <ErrorNote message={note} />}

        <div role="group" aria-label="Addon views" className="-mb-2 flex gap-1 border-b border-edge-soft">
          {TABS.map(({ id, label }) => (
            <button
              key={id}
              aria-pressed={tab === id}
              onClick={() => setTab(id)}
              className={`-mb-px border-b-2 px-4 pb-3 pt-1 text-[14.5px] font-medium transition-colors ${
                tab === id ? "border-accent text-ink" : "border-transparent text-ink-muted hover:text-ink"
              }`}
            >
              {label}
            </button>
          ))}
        </div>

        {tab === "installed" && <InstalledTab />}
        {tab === "builtIn" && <BuiltInTab />}
        {tab === "browse" && <BrowseTab />}
      </div>
    </main>
  );
}
