//! The provider's model list, for picking a model where Wis asks for one.

use std::time::Duration;

use futures_util::StreamExt;
use wis_core::ai_models::{parse_models, ModelInfo};
use wis_core::{WisError, WisResult, WorkspaceId};

use crate::ai::{self, Endpoint, Route};
use crate::state::AppState;

const LIST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_LIST_BYTES: usize = 4 * 1024 * 1024;

fn failed(message: String) -> WisError {
    WisError::InvalidInput { message }
}

pub(crate) async fn list_models(state: &AppState, workspace: WorkspaceId) -> WisResult<Vec<ModelInfo>> {
    if !ai::ai_supported() {
        return Err(failed("the assistant is not available on this platform".into()));
    }

    let (provider, base_url) = {
        let browser = state.browser.lock().await;
        (browser.settings.ai.provider, browser.settings.ai.base_url.clone())
    };
    let Endpoint {
        url,
        headers,
        host,
        client,
    } = ai::endpoint(state, workspace, provider, base_url.as_deref(), Route::Models).await?;

    let read = async {
        let response = client
            .get(url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| failed(format!("request failed: {} ({host})", ai::transport_error(&e))))?;
        let status = response.status();
        let mut body = Vec::new();
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(|e| failed(format!("request failed: {} ({host})", ai::transport_error(&e))))?;
            if body.len() + chunk.len() > MAX_LIST_BYTES {
                return Err(failed(format!("the model list from {host} is too large")));
            }

            body.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            let detail = String::from_utf8_lossy(&body);
            return Err(failed(format!(
                "provider error {} from {host}: {}",
                status.as_u16(),
                ai::first_line(&detail, 200)
            )));
        }

        let json: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|_| failed(format!("{host} did not send a model list")))?;
        Ok(parse_models(provider, &json))
    };

    tokio::time::timeout(LIST_TIMEOUT, read)
        .await
        .map_err(|_| failed(format!("the model list from {host} took too long")))?
}
