import type { ReactNode } from "react";
import { useState } from "react";

export function Spinner({ label }: { label?: string }) {
  return (
    <div className="flex items-center gap-3 text-mist">
      <div className="h-5 w-5 animate-spin rounded-full border-2 border-rig border-t-buoy" />
      {label && <span className="text-sm">{label}</span>}
    </div>
  );
}

export function ErrorNote({ message }: { message: string }) {
  return (
    <div className="rounded-lg border border-coral/40 bg-coral/10 px-4 py-3 text-sm text-coral">
      {message}
    </div>
  );
}

export function EmptyState({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 py-24 text-center">
      <div className="text-4xl">⚓</div>
      <div className="text-lg font-semibold text-foam">{title}</div>
      <div className="max-w-md text-sm text-mist">{children}</div>
    </div>
  );
}

export function Badge({ children, tone = "default" }: { children: ReactNode; tone?: "default" | "warn" | "good" }) {
  const tones = {
    default: "bg-rig text-mist",
    warn: "bg-flare/20 text-flare",
    good: "bg-buoy/20 text-buoy",
  } as const;
  return (
    <span className={`inline-block rounded px-1.5 py-0.5 text-[11px] font-medium ${tones[tone]}`}>
      {children}
    </span>
  );
}

export function Modal({
  title,
  onClose,
  children,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
}) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-abyss/80 p-6"
      onClick={onClose}
    >
      <div
        className="max-h-[85vh] w-full max-w-xl overflow-y-auto rounded-2xl border border-rig bg-deep p-6 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-lg font-semibold">{title}</h2>
          <button
            className="rounded px-2 py-1 text-mist hover:bg-rig hover:text-foam"
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
        className="w-full rounded-lg border border-rig bg-hull px-3 py-2 text-xs text-foam"
      />
      <button
        className="shrink-0 rounded-lg bg-rig px-3 py-2 text-xs font-medium hover:bg-rig/70"
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(value);
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          } catch {
            // secure-context quirks: user can still select + copy manually
          }
        }}
      >
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
  );
}
