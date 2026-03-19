use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::pi_state::PiSessionState;

pub type SharedPiStateStore = Arc<RwLock<HashMap<String, PiSessionState>>>;

pub fn new_shared_pi_state_store() -> SharedPiStateStore {
    Arc::new(RwLock::new(HashMap::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pi_state::PiSessionState;

    #[tokio::test]
    async fn store_can_insert_and_fetch_state() {
        let store = new_shared_pi_state_store();
        let state = PiSessionState::new(
            "issue-1".into(),
            "repo".into(),
            "/repo".into(),
            "issue-1".into(),
            None,
        );
        store
            .write()
            .await
            .insert(state.session_name.clone(), state.clone());
        let fetched = store.read().await.get("issue-1").cloned();
        assert_eq!(fetched, Some(state));
    }
}
