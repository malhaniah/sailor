/**
 * Settings → Player & quality: skipping intros, recaps and credits (`skip{}`), and the
 * next episode (`bingeWatching`, `upNextSeconds`). Every change patches just the field
 * it changes.
 */
import { ChipPicker, SectionCard, ToggleRow } from "../components/ui";
import { useSettings } from "../lib/store";

const UP_NEXT_CHOICES = [10, 15, 20, 30, 45] as const;

export function SkipAndNextCards() {
  const { settings, save } = useSettings();
  if (!settings) return null;
  const { skip } = settings;
  const upNext = UP_NEXT_CHOICES.includes(settings.upNextSeconds as (typeof UP_NEXT_CHOICES)[number])
    ? settings.upNextSeconds
    : 15;

  return (
    <>
      <SectionCard
        title="Intros, recaps and credits"
        description="While one plays, a Skip button shows in the player (Enter takes it). Skipping by itself shows an Undo for a few seconds."
      >
        <ToggleRow
          label="Show the Skip button"
          on={skip.showButton}
          onChange={(on) => void save({ skip: { showButton: on } })}
        />
        <ToggleRow
          label="Skip intros by themselves"
          on={skip.autoSkipIntro}
          onChange={(on) => void save({ skip: { autoSkipIntro: on } })}
        />
        <ToggleRow
          label="Skip recaps by themselves"
          on={skip.autoSkipRecap}
          onChange={(on) => void save({ skip: { autoSkipRecap: on } })}
        />
        <ToggleRow
          label="Skip credits by themselves"
          sub="With a next episode, it starts instead (or its stream picker opens)."
          on={skip.autoSkipCredits}
          onChange={(on) => void save({ skip: { autoSkipCredits: on } })}
        />
        <ToggleRow
          label="Look up skip times online"
          sub="Off: only the video's own chapter names (“Intro”, “Credits”…) count."
          on={skip.onlineLookup}
          onChange={(on) => void save({ skip: { onlineLookup: on } })}
        />
        <p className="text-[12.5px] leading-relaxed text-ink-subtle">
          When a video starts, Zeiler asks a community database where its intro, recap and
          credits are: AniSkip for anime (after Kitsu turns a Kitsu id into a MyAnimeList id),
          TheIntroDB for movies and shows with an IMDb or TMDB id. They get the title's id, the
          season and episode, and the video's length — so they can tell what you watch.
          Nothing else is sent, and the video never waits for them. Skipping by itself uses
          only their times and chapters named exactly for it, never a guess.
        </p>
      </SectionCard>

      <SectionCard
        title="Next episode"
        description="Near the end of an episode an Up Next card shows the next one (n plays it, p the one before)."
      >
        <ToggleRow
          label="Play the next episode by itself"
          sub="When an episode ends — unless you closed the Up Next card. It plays from the same addon and source (the stream's bingeGroup); when there is none, its stream picker opens."
          on={settings.bingeWatching}
          onChange={(on) => void save({ bingeWatching: on })}
        />
        <div className="flex flex-col gap-2">
          <span className="text-[13px] text-ink-muted">Show the Up Next card this long before the end</span>
          <ChipPicker
            options={[...UP_NEXT_CHOICES]}
            value={upNext}
            onChange={(seconds) => void save({ upNextSeconds: seconds })}
            format={(seconds) => `${seconds} s`}
          />
        </div>
      </SectionCard>
    </>
  );
}
