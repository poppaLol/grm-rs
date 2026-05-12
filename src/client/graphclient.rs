use crate::{
    DecodeFromRow, GraphBackend, GraphTx, RelModel,
    backend::GraphPersistence,
    decode::decode_rel_from_row,
    dsl::{GraphQuery, Query, QueryResult, Return},
    error::{GrmError, Result},
    model::NodeModel,
};

// Returned by `Transaction::execute` so callers can inspect kernel IR + raw kernel rows.
pub struct QueryExecution {
    pub gq: GraphQuery,
    pub qr: QueryResult,
}

impl QueryExecution {
    pub fn decode_all<M: crate::decode::DecodeFromRow>(&self) -> crate::error::Result<Vec<M>> {
        self.qr
            .rows
            .iter()
            .map(|row| M::decode(&self.gq, row))
            .collect()
    }
}

// Wrapper that provides graph persistence access
pub struct GraphPersistenceAccess<'a, B: GraphBackend + GraphPersistence> {
    backend: &'a B,
}

impl<'a, B: GraphBackend + GraphPersistence> GraphPersistenceAccess<'a, B> {
    pub fn new(backend: &'a B) -> Self {
        Self { backend }
    }

    pub fn save_to_file(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        self.backend.save_to_file(path)
    }

    pub fn load_from_file(path: impl AsRef<std::path::Path>) -> Result<B> {
        B::load_from_file(path)
    }

    pub fn save_to_binary_file(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        self.backend.save_to_binary_file(path)
    }

    pub fn load_from_binary_file(path: impl AsRef<std::path::Path>) -> Result<B> {
        B::load_from_binary_file(path)
    }
}

// Cheap-to-clone entrypoint (pool/connector).
#[derive(Clone)]
pub struct GraphClient<B: GraphBackend> {
    backend: B,
}

impl<B: GraphBackend> GraphClient<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    pub async fn execute<R: NodeModel>(&self, q: Query<R>) -> Result<QueryExecution> {
        let mut tx = self.transaction().await?;
        let exec = tx.execute(q).await?;
        tx.commit().await?;
        Ok(exec)
    }

    // Optional familiarity facade (like “session/connection” in other libs).
    pub fn connection(&self) -> GraphConnection<B> {
        GraphConnection {
            backend: self.backend.clone(),
        }
    }

    pub async fn transaction(&self) -> Result<Transaction<B::Tx>> {
        let tx = self.backend.begin_tx().await?;
        Ok(Transaction { inner: Some(tx) })
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Returns a persistence accessor for the graph backend
    pub fn persistence(&self) -> Option<GraphPersistenceAccess<'_, B>>
    where
        B: GraphPersistence,
    {
        Some(GraphPersistenceAccess::new(&self.backend))
    }
}

//GraphConnection is the expression of a "live session" with a db
#[derive(Clone)]
pub struct GraphConnection<B: GraphBackend> {
    backend: B,
}

impl<B: GraphBackend> GraphConnection<B> {
    pub async fn transaction(&self) -> Result<Transaction<B::Tx>> {
        let tx = self.backend.begin_tx().await?;
        Ok(Transaction { inner: Some(tx) })
    }
}

// Primary work surface: everything executes through a transaction.
// `inner: Option<T>` lets us *consume* the tx on commit/rollback without awkward moves.
pub struct Transaction<T: GraphTx + Send> {
    inner: Option<T>,
}

impl<T: GraphTx + Send> Transaction<T> {
    fn backend_mut(&mut self) -> Result<&mut T> {
        self.inner.as_mut().ok_or(GrmError::TransactionClosed)
    }

    pub fn tx_mut(&mut self) -> Result<&mut T> {
        self.backend_mut()
    }

    fn take_inner(&mut self) -> Result<T> {
        self.inner.take().ok_or(GrmError::TransactionClosed)
    }

    pub(crate) fn from_inner(inner: T) -> Self {
        Self { inner: Some(inner) }
    }

    fn inner_mut(&mut self) -> crate::Result<&mut T> {
        self.inner.as_mut().ok_or(GrmError::TransactionClosed)
    }

    pub async fn execute<R: NodeModel>(&mut self, q: Query<R>) -> Result<QueryExecution> {
        let gq = q.compile_to_graph();
        let qr = self.inner_mut()?.execute_graph(&gq).await?;
        Ok(QueryExecution { gq, qr })
    }

    // Typed decode wrapper (thin)
    pub async fn query<R, M>(&mut self, q: Query<R>) -> Result<Vec<M>>
    where
        R: NodeModel,
        M: DecodeFromRow,
    {
        let exec = self.execute(q).await?;
        exec.qr
            .rows
            .iter()
            .map(|row| <M as crate::decode::DecodeFromRow>::decode(&exec.gq, row))
            .collect()
    }

    pub async fn query_rel<RRoot, RRel>(&mut self, q: Query<RRoot>) -> Result<Vec<RRel>>
    where
        RRoot: NodeModel,
        RRel: RelModel,
    {
        let exec = self.execute(q).await?;

        // Optional safety: ensure the query is actually returning a rel.
        match exec.gq.ret {
            Return::Rel(_) => {}
            _ => {
                return Err(crate::GrmError::Mapping(
                "query_rel called but query return is not Return::Rel; did you forget .return_rel()?".into()
            ));
            }
        }

        exec.qr
            .rows
            .iter()
            .map(|row| decode_rel_from_row::<RRel>(&exec.gq, row))
            .collect()
    }

    // Commit consumes the tx (enforced by `GraphTx`).
    pub async fn commit(mut self) -> Result<()> {
        let tx = self.take_inner()?;
        tx.commit().await
    }

    // Rollback consumes the tx (enforced by `GraphTx`).
    pub async fn rollback(mut self) -> Result<()> {
        let tx = self.take_inner()?;
        tx.rollback().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct ClosedMockTx;

    #[async_trait]
    impl GraphTx for ClosedMockTx {
        async fn execute_graph(&mut self, _q: &GraphQuery) -> Result<QueryResult> {
            unreachable!("closed transaction should not expose its inner tx")
        }

        async fn commit(self) -> Result<()> {
            unreachable!("closed transaction should not expose its inner tx")
        }

        async fn rollback(self) -> Result<()> {
            unreachable!("closed transaction should not expose its inner tx")
        }
    }

    #[test]
    fn closed_transaction_returns_transaction_closed_from_mut_access() {
        let mut tx = Transaction::<ClosedMockTx> { inner: None };

        assert!(matches!(tx.tx_mut(), Err(GrmError::TransactionClosed)));
    }

    #[test]
    fn closed_transaction_returns_transaction_closed_when_consumed_again() {
        let mut tx = Transaction::<ClosedMockTx> { inner: None };

        assert!(matches!(tx.take_inner(), Err(GrmError::TransactionClosed)));
    }
}
