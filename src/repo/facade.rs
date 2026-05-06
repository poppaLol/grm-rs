use crate::{
    DecodeFromRow, GraphTx, NodeModel, Query, RelModel,
    client::{QueryExecution, Transaction},
    repo::{NodeRepositoryTx, RelRepositoryTx},
};

pub struct Repo<'a, T: GraphTx + Send> {
    tx: &'a mut Transaction<T>,
}

impl<T: GraphTx + Send> Transaction<T> {
    pub fn repo(&mut self) -> Repo<'_, T> {
        Repo { tx: self }
    }
}

impl<'a, T: GraphTx + Send> Repo<'a, T> {
    pub async fn execute<R>(&mut self, q: Query<R>) -> crate::Result<QueryExecution>
    where
        R: NodeModel,
    {
        self.tx.execute(q).await
    }

    pub async fn query<R, M>(&mut self, q: Query<R>) -> crate::Result<Vec<M>>
    where
        R: NodeModel,
        M: DecodeFromRow, // whatever you already use
    {
        self.tx.query::<R, M>(q).await
    }

    pub async fn query_rel<R, Rel>(&mut self, q: Query<R>) -> crate::Result<Vec<Rel>>
    where
        R: NodeModel,
        Rel: RelModel,
    {
        self.tx.query_rel::<R, Rel>(q).await
    }

    pub fn nodes<M: NodeModel>(&mut self) -> NodeRepositoryTx<'_, T, M> {
        NodeRepositoryTx::new(self.tx)
    }

    pub fn rels<R: RelModel>(&mut self) -> RelRepositoryTx<'_, T, R> {
        RelRepositoryTx::new(self.tx)
    }
}
