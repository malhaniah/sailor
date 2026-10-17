import { useState } from "react";
import { ErrorNote, Modal, Spinner } from "../../components/ui";
import { errorMessage } from "../../lib/ipc";
import { PRIMARY_BUTTON, SECONDARY_BUTTON } from "../addons/common";

/** Asks for a 4–8 digit PIN and hands it to `onSubmit`, which throws on a wrong one
 * (the error is shown and the field cleared). */
export function PinEntry({
  title,
  note,
  onSubmit,
  onClose,
}: {
  title: string;
  note?: string;
  onSubmit: (pin: string) => Promise<void>;
  onClose: () => void;
}) {
  const [pin, setPin] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const valid = /^\d{4,8}$/.test(pin);
  return (
    <Modal title={title} onClose={() => !busy && onClose()}>
      <form
        className="flex flex-col gap-4"
        onSubmit={async (e) => {
          e.preventDefault();
          if (!valid) return;
          setBusy(true);
          setError(null);
          try {
            await onSubmit(pin);
          } catch (err) {
            setError(errorMessage(err));
            setPin("");
          } finally {
            setBusy(false);
          }
        }}
      >
        {note && <p className="text-[13px] leading-relaxed text-ink-muted">{note}</p>}
        <input
          autoFocus
          type="password"
          inputMode="numeric"
          autoComplete="off"
          aria-label="PIN"
          maxLength={8}
          value={pin}
          onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
          className="h-12 rounded-xl border border-edge bg-canvas px-4 text-center font-mono text-[22px] tracking-[0.5em] text-ink outline-none focus:border-ink"
        />
        {error && <ErrorNote message={error} />}
        <div className="flex justify-end gap-3">
          <button type="button" className={SECONDARY_BUTTON} onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="submit" className={PRIMARY_BUTTON} disabled={!valid || busy}>
            {busy ? <Spinner /> : "Continue"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
