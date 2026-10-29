use tauri::{AppHandle, State};
use wis_core::{TabId, WisError, WisResult};

#[cfg(target_os = "macos")]
use crate::commands::mutate_unit;
use crate::state::AppState;

/// Translate a tab's page on-device (`enabled`) or restore the original. A
/// page Wis won't translate (already in the reader's language, models not
/// downloaded, no declared language) leaves the tab as it was and the error
/// says why; a start overtaken by a newer press or a navigation changes
/// nothing. The `translated` flag mirrors this for the UI; a navigation resets it.
#[tauri::command]
pub async fn tab_set_translated(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    enabled: bool,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        let label = format!("tab-{}", id.as_uuid());
        if !enabled {
            super::session::disable(&app, &label);
            return mutate_unit(&app, &state, |s| s.with_tab_translated(id, false)).await;
        }

        let started = super::session::enable(&app, &label)
            .await
            .map_err(|message| WisError::InvalidInput { message })?;
        let Some(gen) = started else {
            return Ok(());
        };
        mutate_unit(&app, &state, |s| {
            if super::session::is_current(&label, gen) {
                s.with_tab_translated(id, true)
            } else {
                Ok(s.clone())
            }
        })
        .await
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, id, enabled);
        Err(WisError::InvalidInput {
            message: "Translation isn't available on this platform".into(),
        })
    }
}
