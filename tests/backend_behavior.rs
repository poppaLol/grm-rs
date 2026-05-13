mod common;

use grm_rs::{InMemoryBackend, Result};

#[tokio::test]
async fn in_memory_backend_satisfies_shared_behavior() -> Result<()> {
    common::run_shared_backend_behavior(InMemoryBackend::new()).await
}
