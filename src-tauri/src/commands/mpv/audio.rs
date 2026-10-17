//! Volume normalization: a fixed `dynaudnorm` audio filter under a label of its own.
//!
//! `af` stays off the webview's property list (`validate.rs`: a filter graph can reach
//! files and devices), so this command adds only this one filter, and only under the
//! `@zeiler-normalize` label: adding it again replaces it, removing it leaves any filter
//! the user's own mpv config set (external mpv). The values are Harbor's
//! (`f=200:g=15:m=10:r=0.6`: a 2 s window, gentle gain, at most 10×, target 60 %).

use tauri::State;
use zeiler_player::Session;

use super::{PlayerState, session_error};
use crate::error::IpcError;

const LABEL: &str = "@zeiler-normalize";
const FILTER: &str = "lavfi=[dynaudnorm=f=200:g=15:m=10:r=0.6]";

/// The `af` command that switches normalization on or off.
fn normalize_command(on: bool) -> Vec<String> {
    if on {
        vec!["af".into(), "add".into(), format!("{LABEL}:{FILTER}")]
    } else {
        vec!["af".into(), "remove".into(), LABEL.into()]
    }
}

/// Apply `on` to a session (also right after it starts, from the `audio.normalize`
/// setting).
pub(crate) async fn apply_normalize(session: &Session, on: bool) -> Result<(), IpcError> {
    session
        .command(normalize_command(on))
        .await
        .map_err(session_error)
}

/// Normalize the volume (quiet dialogue up, loud scenes down) or stop. The UI saves the
/// choice in `audio.normalize`.
#[tauri::command]
pub async fn player_set_audio_normalize(
    player: State<'_, PlayerState>,
    on: bool,
) -> Result<(), IpcError> {
    apply_normalize(&*player.session()?, on).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filter_is_fixed_and_labelled() {
        assert_eq!(
            normalize_command(true),
            [
                "af",
                "add",
                "@zeiler-normalize:lavfi=[dynaudnorm=f=200:g=15:m=10:r=0.6]"
            ]
        );
        assert_eq!(
            normalize_command(false),
            ["af", "remove", "@zeiler-normalize"]
        );
    }
}
