import type { ReactNode } from "react";
import { useState } from "react";

export function Spinner({ label }: { label?: string }) {
  return (
    <div className="flex items-center gap-3 text-ink-muted">
      <div className="h-5 w-5 animate-spin rounded-full border-2 border-edge border-t-accent" />
      {label && <span className="text-sm">{label}</span>}
    </div>
  );
}

export function ErrorNote({ message }: { message: string }) {
  return (
    <div className="rounded-2xl border border-danger/30 bg-danger/15 px-5 py-4 text-[13.5px] text-ink">
      {message}
    </div>
  );
}

export function EmptyState({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center gap-3 rounded-[24px] border border-edge-soft/70 bg-canvas/80 px-9 py-16 text-center">
      <div className="text-[10.5px] font-semibold uppercase tracking-[0.42em] text-ink-subtle">
        Sailor
      </div>
      <div className="font-display text-[28px] font-medium leading-tight text-ink">{title}</div>
      <div className="max-w-md text-[13.5px] leading-relaxed text-ink-muted">{children}</div>
    </div>
  );
}

export function Badge({
  children,
  tone = "default",
}: {
  children: ReactNode;
  tone?: "default" | "warn" | "good" | "accent";
}) {
  const tones = {
    default: "bg-raised text-ink-muted",
    warn: "bg-danger/15 text-[oklch(0.75_0.12_25)]",
    good: "bg-accent/15 text-accent",
    accent: "bg-accent/15 text-accent",
  } as const;
  return (
    <span
      className={`inline-block rounded-md px-1.5 py-0.5 text-[10px] font-semibold tracking-wide ${tones[tone]}`}
    >
      {children}
    </span>
  );
}

export function Modal({
  title,
  onClose,
  children,
  wide = false,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  wide?: boolean;
}) {
  return (
    <div
      className="fixed inset-0 z-[140] flex items-center justify-center bg-black/65 p-6 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className={`animate-modal-in max-h-[85vh] w-full overflow-y-auto rounded-[24px] border border-edge-soft bg-elevated/95 px-7 py-7 shadow-[0_30px_80px_-25px_rgba(0,0,0,0.85)] ${
          wide ? "max-w-xl" : "max-w-[420px]"
        }`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-[19px] font-medium tracking-tight">{title}</h2>
          <button
            className="flex h-8 w-8 items-center justify-center rounded-lg text-ink-muted hover:bg-raised hover:text-ink"
            onClick={onClose}
          >
            ✕
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

export function CopyField({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="flex items-center gap-2">
      <input
        readOnly
        value={value}
        onFocus={(e) => e.currentTarget.select()}
        className="h-10 w-full rounded-xl border border-edge bg-canvas px-3 font-mono text-[11.5px] text-ink outline-none"
      />
      <button
        className="flex h-10 shrink-0 items-center rounded-xl bg-ink px-4 text-[12.5px] font-semibold text-canvas transition-transform hover:scale-[1.02] active:scale-[0.97]"
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(value);
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          } catch {
            /* select + copy manually */
          }
        }}
      >
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
  );
}

// ---- settings building blocks (Harbor recipes) ----------------------------------------

export function SectionCard({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <section className="flex flex-col gap-4 rounded-2xl border border-edge-soft bg-elevated/40 p-7">
      <div className="flex flex-col gap-1">
        <h2 className="text-[19px] font-medium tracking-tight text-ink">{title}</h2>
        {description && (
          <p className="text-[13.5px] leading-relaxed text-ink-muted">{description}</p>
        )}
      </div>
      {children}
    </section>
  );
}

export function Toggle({ on, onChange }: { on: boolean; onChange: (on: boolean) => void }) {
  return (
    <span
      role="switch"
      aria-checked={on}
      onClick={(e) => {
        e.stopPropagation();
        onChange(!on);
      }}
      className={`relative inline-block h-6 w-10 shrink-0 cursor-pointer rounded-full transition-colors ${
        on ? "bg-ink" : "bg-edge"
      }`}
    >
      <span
        className={`absolute top-0.5 h-5 w-5 rounded-full bg-canvas transition-transform ${
          on ? "translate-x-[18px]" : "translate-x-0.5"
        }`}
      />
    </span>
  );
}

export function ToggleRow({
  label,
  sub,
  on,
  onChange,
  lockReason,
}: {
  label: string;
  sub?: string;
  on: boolean;
  onChange: (on: boolean) => void;
  lockReason?: string;
}) {
  const locked = !!lockReason;
  return (
    <button
      disabled={locked}
      onClick={() => onChange(!on)}
      className={`flex items-center justify-between gap-4 rounded-xl border border-edge-soft bg-canvas/40 px-4 py-3 text-left transition-colors hover:border-edge ${
        locked ? "cursor-not-allowed opacity-60" : ""
      }`}
    >
      <span className="flex min-w-0 flex-col gap-0.5">
        <span className="text-[14px] font-medium text-ink">{label}</span>
        {locked ? (
          <span className="text-[12.5px] text-accent">{lockReason}</span>
        ) : (
          sub && <span className="text-[12.5px] leading-snug text-ink-muted">{sub}</span>
        )}
      </span>
      <Toggle on={on && !locked} onChange={onChange} />
    </button>
  );
}

export function ChipPicker<T extends string | number>({
  options,
  value,
  onChange,
  format,
}: {
  options: T[];
  value: T;
  onChange: (value: T) => void;
  format?: (option: T) => string;
}) {
  return (
    <div className="flex flex-wrap gap-2">
      {options.map((option) => (
        <button
          key={String(option)}
          onClick={() => onChange(option)}
          className={`rounded-full px-3.5 py-1.5 text-[12px] font-semibold transition-colors ${
            option === value
              ? "bg-ink text-canvas"
              : "bg-raised text-ink-muted hover:bg-elevated hover:text-ink"
          }`}
        >
          {format ? format(option) : String(option)}
        </button>
      ))}
    </div>
  );
}

/** Harbor's "DesktopOnlyBlock" recipe, reused for Harbor-only features Sailor lacks. */
export function ComingLaterBlock({ label = "Coming later", children }: { label?: string; children: ReactNode }) {
  return (
    <div className="relative">
      <div className="pointer-events-none opacity-40">{children}</div>
      <div className="absolute inset-0 flex items-center justify-center">
        <span className="rounded-full border border-edge-soft bg-elevated/95 px-3 py-1 text-[10.5px] font-semibold uppercase tracking-[0.16em] text-ink-muted">
          {label}
        </span>
      </div>
    </div>
  );
}
