use crate::{
    DecodeFromRow, GraphBackend, GraphTx, RelModel,
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

// Cheap-to-clone entrypoint (pool/session/config façade).
#[derive(Clone)]
pub struct GraphClient<B: GraphBackend> {
    backend: B,
}

impl<B: GraphBackend> GraphClient<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
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
}

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
    fn inner_mut(&mut self) -> Result<&mut T> {
        self.inner
            .as_mut()
            .ok_or_else(|| GrmError::Backend("transaction already finished".into()))
    }

    fn take_inner(&mut self) -> Result<T> {
        self.inner
            .take()
            .ok_or_else(|| GrmError::Backend("transaction already finished".into()))
    }

    // Lowest-level escape hatch: access the backend tx directly.
    pub fn tx_mut(&mut self) -> Result<&mut T> {
        self.inner_mut()
    }

    pub async fn execute<R: NodeModel>(&mut self, q: Query<R>) -> Result<QueryExecution> {
        let gq = q.compile_to_graph();
        let qr = self.inner_mut()?.execute_graph(&gq).await?;
        Ok(QueryExecution { gq, qr })
    }

    // Typed decode wrapper (thin). Replace `DecodeFromRow` with your real decode trait.
    pub async fn query<R, M: DecodeFromRow>(&mut self, q: Query<R>) -> Result<Vec<M>>
    where
        R: NodeModel,
        M: crate::decode::DecodeFromRow,
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
