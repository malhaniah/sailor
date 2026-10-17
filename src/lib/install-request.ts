// Where installs start: the URL bar, a directory listing, a stremio:// link, an update or
// a reconfigure. Every one of them ends in the same confirmation dialog (InstallHost);
// nothing installs without it.
import { useSyncExternalStore } from "react";

export type InstallSource = "pasted" | "directory" | "link" | "update" | "reconfigure";

export interface InstallRequest {
  /** what to preview; empty for a reconfigure that starts on the setup step */
  url: string;
  source: InstallSource;
  /** the directory listing it came from (its manifest is compared with the live one) */
  listingKey?: string;
  /** an installed addon's transport URL this one would take the place of */
  replace?: string;
  /** shown in a reconfigure's heading */
  name?: string;
  /** a reconfigure: the setup page to open */
  configureUrl?: string;
  /** an update: what changed since the installed version */
  changes?: string[];
}

let current: InstallRequest | null = null;
const listeners = new Set<() => void>();

function set(next: InstallRequest | null) {
  current = next;
  for (const listener of listeners) listener();
}

/** Open the install confirmation. Ignored while one is already open (a second
 * stremio:// link, a double click), so the open dialog is never swapped under the user. */
export function requestInstall(request: InstallRequest): boolean {
  if (current) return false;
  set(request);
  return true;
}

export function closeInstall() {
  set(null);
}

export function useInstallRequest(): InstallRequest | null {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
  );
}
