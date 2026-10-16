/**
 * The control bar's popup menus: a chip button with a panel above it, and the rows and
 * small controls the panels are built from (ControlsBar, SubtitleMenu, AudioMenu).
 */
import { type ReactNode, useEffect, useRef, useState } from "react";

export const chip =
  "flex h-8 items-center gap-1.5 rounded-md bg-canvas/50 px-3 text-sm text-ink-muted hover:bg-elevated hover:text-ink";

/** A button with a panel above it; closes on a click elsewhere or Escape (which then
 * does not also leave the player), and on a pick unless the panel keeps itself open. */
export function PopupMenu({
  label,
  title,
  width = "w-56",
  onOpen,
  children,
}: {
  label: ReactNode;
  title?: string;
  /** a Tailwind width class for the panel */
  width?: string;
  onOpen?: () => void;
  children: (close: () => void) => ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    // capture phase: runs before the player's window shortcuts, which skip a
    // prevented event
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open]);
  return (
    <div ref={ref} className="relative">
      <button
        className={chip}
        onClick={() => {
          if (!open) onOpen?.();
          setOpen((o) => !o);
        }}
        title={title}
        aria-expanded={open}
      >
        {label}
      </button>
      {open && (
        <div
          className={`absolute bottom-10 right-0 z-10 max-h-[min(30rem,70vh)] overflow-y-auto rounded-lg border border-edge bg-surface py-1 shadow-xl ${width}`}
          onClick={(e) => e.stopPropagation()}
        >
          {children(() => setOpen(false))}
        </div>
      )}
    </div>
  );
}

export function MenuRow({
  label,
  selected,
  onPick,
}: {
  label: string;
  selected: boolean;
  onPick: () => void;
}) {
  return (
    <button
      className={`block w-full truncate px-3 py-1.5 text-left text-sm hover:bg-elevated ${
        selected ? "text-accent" : "text-ink-muted"
      }`}
      onClick={onPick}
    >
      {selected ? "✓ " : ""}
      {label}
    </button>
  );
}

/** A small heading inside a panel. */
export function PanelHeading({ children, aside }: { children: ReactNode; aside?: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-2 px-3 pb-1 pt-2">
      <span className="text-[10.5px] font-semibold uppercase tracking-[0.14em] text-ink-subtle">
        {children}
      </span>
      {aside}
    </div>
  );
}

/** Small toggle chips: one of `options` is current. */
export function Choice<T extends string | number>({
  options,
  value,
  onChange,
  label,
}: {
  options: { value: T; label: string }[];
  value: T | null;
  onChange: (value: T) => void;
  label?: string;
}) {
  return (
    <div className="flex flex-wrap items-center gap-1" role="group" aria-label={label}>
      {options.map((option) => (
        <button
          key={String(option.value)}
          aria-pressed={option.value === value}
          onClick={() => onChange(option.value)}
          className={`rounded-md px-2 py-1 text-[11.5px] font-medium transition-colors ${
            option.value === value
              ? "bg-ink text-canvas"
              : "bg-raised/70 text-ink-muted hover:bg-elevated hover:text-ink"
          }`}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

/** "+0.3 s", "−1.25 s", "0.0 s". */
export function formatDelay(seconds: number): string {
  const rounded = Math.round(seconds * 100) / 100;
  if (rounded === 0) return "0.0 s";
  const tenths = Number.isInteger(Math.round(Math.abs(rounded) * 100) / 10);
  return `${rounded > 0 ? "+" : "−"}${Math.abs(rounded).toFixed(tenths ? 1 : 2)} s`;
}

/** A delay, nudged in 0.1 s and 0.5 s steps, with reset. */
export function DelayRow({
  label,
  value,
  onChange,
  hint,
}: {
  label: string;
  value: number;
  onChange: (seconds: number) => void;
  /** the keyboard shortcut, for the tooltip */
  hint?: string;
}) {
  const step = (delta: number) =>
    onChange(Math.min(600, Math.max(-600, Math.round((value + delta) * 1000) / 1000)));
  const button = (delta: number) => (
    <button
      key={delta}
      onClick={() => step(delta)}
      className="h-7 rounded-md bg-raised/70 px-1.5 font-mono text-[11px] text-ink-muted hover:bg-elevated hover:text-ink"
      aria-label={`${label} ${delta > 0 ? "+" : "−"}${Math.abs(delta)} seconds`}
    >
      {delta > 0 ? "+" : "−"}
      {Math.abs(delta)}
    </button>
  );
  return (
    <div className="flex items-center gap-1 px-3 py-1.5" title={hint}>
      <span className="w-14 shrink-0 text-xs text-ink-subtle">{label}</span>
      {button(-0.5)}
      {button(-0.1)}
      <span
        className="w-[4.5rem] shrink-0 whitespace-nowrap text-center font-mono text-xs text-ink"
        aria-live="polite"
      >
        {formatDelay(value)}
      </span>
      {button(0.1)}
      {button(0.5)}
      <button
        onClick={() => onChange(0)}
        disabled={Math.abs(value) < 1e-6}
        className="ml-auto h-7 rounded-md px-2 text-[11px] text-ink-subtle hover:text-ink disabled:opacity-40"
      >
        Reset
      </button>
    </div>
  );
}
