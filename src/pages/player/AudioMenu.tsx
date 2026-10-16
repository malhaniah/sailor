/**
 * The audio menu: the file's audio tracks by language, with their title, channel layout
 * (Stereo, 5.1, 7.1), codec and Default/File flags, and the audio delay. The track
 * picked is remembered for the title, so the next episode starts with the same
 * language.
 */
import { AudioLines } from "lucide-react";
import { playerSetProperty } from "../../lib/player-ipc";
import { DelayRow, PanelHeading, PopupMenu } from "./menu";
import type { MpvState } from "./mpv-state";
import type { Tracks } from "./useTracks";

export function AudioMenu({ tracks, mpv }: { tracks: Tracks; mpv: MpvState }) {
  return (
    <PopupMenu
      label={
        <>
          <AudioLines size={15} />
          Audio
        </>
      }
      title="Audio"
      width="w-[23rem]"
    >
      {() => (
        <div className="flex flex-col pb-1">
          <PanelHeading>Audio</PanelHeading>
          <div className="flex flex-col py-0.5" role="listbox" aria-label="Audio tracks">
            {tracks.audio.map((entry) => (
              <button
                key={entry.id}
                role="option"
                aria-selected={entry.selected}
                onClick={() => tracks.selectAudio(entry)}
                className={`flex w-full items-baseline gap-2 px-3 py-1.5 text-left text-sm hover:bg-elevated ${
                  entry.selected ? "text-accent" : "text-ink-muted"
                }`}
              >
                <span className="w-4 shrink-0">{entry.selected ? "✓" : ""}</span>
                <span className="truncate">{entry.title}</span>
                <span className="ml-auto shrink-0 truncate text-[10.5px] text-ink-subtle">
                  {entry.tags.join(" · ")}
                </span>
              </button>
            ))}
            {tracks.audio.length === 0 && (
              <div className="px-3 py-2 text-xs text-ink-subtle">No audio tracks</div>
            )}
          </div>
          <div className="border-t border-edge-soft pt-1">
            <DelayRow
              label="Delay"
              value={mpv.audioDelay}
              onChange={(v) => void playerSetProperty("audio-delay", v).catch(() => undefined)}
              hint="Positive plays the sound later."
            />
          </div>
        </div>
      )}
    </PopupMenu>
  );
}
