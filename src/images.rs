use std::sync::Arc;

use crate::state::AppState;

pub async fn load_base(state: &AppState, url: Option<&str>) -> Option<Vec<u8>> {
    let url = url?.trim();
    if url.is_empty() {
        return None;
    }
    if let Some(cached) = state.images.get(url).await {
        return Some((*cached).clone());
    }
    let response = state.http.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    if bytes.is_empty() || bytes.len() > 8_000_000 {
        return None;
    }
    let owned = Arc::new(bytes.to_vec());
    state.images.insert(url.to_string(), Arc::clone(&owned)).await;
    Some((*owned).clone())
}
