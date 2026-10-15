use sailor_addon::{Descriptor, normalize_transport_input};
use tauri::State;

use crate::error::IpcError;
use crate::state::AppState;

/// Fetch and parse a manifest without installing — feeds the install-confirmation dialog.
#[tauri::command]
pub async fn addon_preview(
    state: State<'_, AppState>,
    url: String,
) -> Result<Descriptor, IpcError> {
    let transport_url = normalize_transport_input(&url)?;
    Ok(state.client.descriptor(&transport_url).await?)
}

/// Install (or update in place) an addon by transport URL.
#[tauri::command]
pub async fn addon_add(state: State<'_, AppState>, url: String) -> Result<Descriptor, IpcError> {
    let transport_url = normalize_transport_input(&url)?;
    let descriptor = state.client.descriptor(&transport_url).await?;

    if descriptor.manifest.behavior_hints.configuration_required {
        let configure_url = transport_url
            .as_str()
            .replace("/manifest.json", "/configure");
        return Err(IpcError::InvalidInput(format!(
            "This addon must be configured first. Open {configure_url} in a browser, then install the configured URL it gives you."
        )));
    }

    {
        let mut addons = state.addons.write().await;
        if let Some(existing) = addons
            .iter_mut()
            .find(|d| d.transport_url == descriptor.transport_url)
        {
            *existing = descriptor.clone();
        } else {
            addons.push(descriptor.clone());
        }
    }
    state.persist_addons().await?;
    Ok(descriptor)
}

#[tauri::command]
pub async fn addon_remove(
    state: State<'_, AppState>,
    transport_url: String,
) -> Result<(), IpcError> {
    {
        let mut addons = state.addons.write().await;
        let before = addons.len();
        addons.retain(|d| d.transport_url.as_str() != transport_url);
        if addons.len() == before {
            return Err(IpcError::NotFound("addon is not installed".into()));
        }
    }
    state.persist_addons().await
}

#[tauri::command]
pub async fn addon_list(state: State<'_, AppState>) -> Result<Vec<Descriptor>, IpcError> {
    Ok(state.effective_addons().await)
}
