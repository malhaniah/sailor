//! The skip prompt inside external mpv's own window (M7).
//!
//! With external mpv the video is in mpv's window, where Zeiler's Skip button cannot be
//! seen. So while a skip (or Next episode) is on offer, mpv shows it as an OSD message
//! and Enter in its window sends `script-message zeiler-skip`, which comes back as a
//! `client-message` event and does what the button does. The key binding lives in an
//! input section of its own (`define-section … force`, then `enable-section`), so
//! disabling the section gives the user's own Enter binding back.
//!
//! `show-text`, `define-section` and the others stay off the webview's command list
//! (`validate.rs`): this command sends only fixed strings.

use tauri::State;
use zeiler_player::Backend;

use super::PlayerState;
use crate::error::IpcError;

const SECTION: &str = "zeiler-skip";
const BINDINGS: &str = "ENTER script-message zeiler-skip\nKP_ENTER script-message zeiler-skip";
/// How long mpv shows the prompt (it is shown again for the next segment).
const PROMPT_MS: &str = "5000";

/// The prompt's text for each offer.
fn prompt_text(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "intro" => "Skip intro — press Enter",
        "recap" => "Skip recap — press Enter",
        "outro" => "Skip credits — press Enter",
        "next" => "Next episode — press Enter",
        _ => return None,
    })
}

/// The mpv commands for an offer of `kind` (`None`: the offer ended).
fn prompt_commands(kind: Option<&str>) -> Result<Vec<Vec<String>>, IpcError> {
    let argv = |args: &[&str]| args.iter().map(|a| (*a).to_string()).collect::<Vec<_>>();
    Ok(match kind {
        None => vec![argv(&["disable-section", SECTION])],
        Some(kind) => {
            let text = prompt_text(kind).ok_or_else(|| {
                IpcError::InvalidInput(format!("\"{kind}\" is not something to skip."))
            })?;
            vec![
                argv(&["define-section", SECTION, BINDINGS, "force"]),
                argv(&["enable-section", SECTION]),
                argv(&["show-text", text, PROMPT_MS]),
            ]
        }
    })
}

/// Offer a skip in external mpv's window (`kind`: `intro`, `recap`, `outro` or `next`),
/// or take the offer back (`null`). Built-in playback has no window of its own (the
/// Skip button is on top of the video), so it is a no-op there. A command an older mpv
/// lacks costs only the prompt.
#[tauri::command]
pub async fn player_osd_prompt(
    player: State<'_, PlayerState>,
    kind: Option<String>,
) -> Result<(), IpcError> {
    let commands = prompt_commands(kind.as_deref())?;
    let Ok(session) = player.session() else {
        return Ok(());
    };
    if session.backend() != Backend::External {
        return Ok(());
    }
    for command in commands {
        let _ = session.command(command).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_binds_enter_in_its_own_section() {
        let offer = prompt_commands(Some("intro")).unwrap();
        assert_eq!(offer[0][0], "define-section");
        assert_eq!(offer[0][3], "force", "over the user's own Enter binding");
        assert_eq!(offer[1], ["enable-section", "zeiler-skip"]);
        assert_eq!(offer[2][1], "Skip intro — press Enter");
        assert_eq!(
            prompt_commands(None).unwrap(),
            [["disable-section", "zeiler-skip"]]
        );
        assert!(prompt_commands(Some("run")).is_err());
    }
}
