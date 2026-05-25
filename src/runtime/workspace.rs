use std::path::Path;

use crate::Result;

use super::{DurabilityFormat, SessionState};

/// Runtime-level graph workspace.
///
/// A workspace is the resumable GRM unit: graph data plus runtime schema,
/// carried together through the current session snapshot mechanics.
pub struct Workspace {
    state: SessionState,
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            state: SessionState::new(),
        }
    }

    pub fn from_state(state: SessionState) -> Self {
        Self { state }
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut SessionState {
        &mut self.state
    }

    pub fn into_state(self) -> SessionState {
        self.state
    }

    pub fn save(&self, format: DurabilityFormat, path: impl AsRef<Path>) -> Result<()> {
        self.state.save_workspace(format, path)
    }

    pub fn load(format: DurabilityFormat, path: impl AsRef<Path>) -> Result<Self> {
        let mut state = SessionState::new();
        state.load_workspace(format, path)?;
        Ok(Self { state })
    }

    pub fn open(format: DurabilityFormat, path: impl AsRef<Path>) -> Result<Self> {
        Self::load(format, path)
    }
}
