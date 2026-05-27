use std::path::{Path, PathBuf};

use crate::{GrmError, Result};

use super::{DurabilityFormat, RuntimeDispatchOutcome, RuntimeRequest, SessionState};

const AUTOCOMMIT_CHECKPOINT_INTERVAL: usize = 8;

#[derive(Debug, Clone)]
struct WorkspaceAutocommitTarget {
    format: DurabilityFormat,
    path: PathBuf,
    pending_entries: usize,
}

/// Runtime-level graph workspace.
///
/// A workspace is the resumable GRM unit: graph data plus runtime schema,
/// carried together through the current session snapshot mechanics.
pub struct Workspace {
    state: SessionState,
    autocommit: Option<WorkspaceAutocommitTarget>,
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
            autocommit: None,
        }
    }

    pub fn from_state(state: SessionState) -> Self {
        Self {
            state,
            autocommit: None,
        }
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

    /// Enable local autocommit for mutations executed through `Workspace::execute_runtime`.
    ///
    /// Enabling autocommit immediately checkpoints the current workspace state to
    /// establish a replay base. Later successful runtime mutations append their
    /// returned durable operations in order. Direct `state_mut()` mutations are a
    /// lower-level escape hatch and are not autocommitted by this workspace.
    pub fn enable_autocommit(
        &mut self,
        format: DurabilityFormat,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        self.state.checkpoint_durable(format, &path)?;
        self.autocommit = Some(WorkspaceAutocommitTarget {
            format,
            path,
            pending_entries: 0,
        });
        Ok(())
    }

    pub fn open_autocommit(format: DurabilityFormat, path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut workspace = Self::open(format, path)?;
        workspace.enable_autocommit(format, path)?;
        Ok(workspace)
    }

    /// Execute a structured runtime request through the workspace.
    ///
    /// This is the preferred workspace mutation path. When autocommit is enabled,
    /// successful mutations append the durable operations returned by
    /// `SessionState::execute_runtime`. Requests that return no durable
    /// operations do not touch the append log.
    pub async fn execute_runtime(
        &mut self,
        request: RuntimeRequest,
    ) -> Result<RuntimeDispatchOutcome> {
        let outcome = self.state.execute_runtime(request).await?;
        self.persist_autocommit_ops(&outcome.durable_ops)?;
        Ok(outcome)
    }

    pub fn save(&self, format: DurabilityFormat, path: impl AsRef<Path>) -> Result<()> {
        self.state.save_workspace(format, path)
    }

    pub fn checkpoint(&self, format: DurabilityFormat, path: impl AsRef<Path>) -> Result<()> {
        self.state.checkpoint_durable(format, path)
    }

    pub fn load(format: DurabilityFormat, path: impl AsRef<Path>) -> Result<Self> {
        let mut state = SessionState::new();
        state.load_workspace(format, path)?;
        Ok(Self {
            state,
            autocommit: None,
        })
    }

    pub fn open(format: DurabilityFormat, path: impl AsRef<Path>) -> Result<Self> {
        Self::load(format, path)
    }

    fn persist_autocommit_ops(&mut self, ops: &[super::DurableOperation]) -> Result<()> {
        if ops.is_empty() {
            return Ok(());
        }

        let Some(target) = &mut self.autocommit else {
            return Ok(());
        };

        for op in ops {
            self.state
                .append_durable_operation(&target.path, op)
                .map_err(|err| {
                    GrmError::Backend(format!(
                        "workspace autocommit failed for '{}': {}",
                        target.path.display(),
                        err
                    ))
                })?;
            target.pending_entries += 1;
        }

        if target.pending_entries >= AUTOCOMMIT_CHECKPOINT_INTERVAL {
            self.checkpoint_autocommit()?;
        }

        Ok(())
    }

    fn checkpoint_autocommit(&mut self) -> Result<()> {
        let Some(target) = &mut self.autocommit else {
            return Ok(());
        };

        self.state
            .checkpoint_durable(target.format, &target.path)
            .map_err(|err| {
                GrmError::Backend(format!(
                    "workspace autocommit failed for '{}': {}",
                    target.path.display(),
                    err
                ))
            })?;
        target.pending_entries = 0;
        Ok(())
    }
}
