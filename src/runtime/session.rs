use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::backend::{BinaryPersistedGraphStore, GraphStore, PersistedGraphStore};
use crate::dsl::{
    CompareOp, Direction, GraphQuery, HopMatch, MatchClause, NodeMatch, Return, ReturnKind, VarGen,
};
use crate::fsutil::{
    backup_path, log_path, write_file_atomically, write_file_atomically_with_backup,
};
use crate::runtime::{KeyValueArg, QueryTerm, SessionCommand, parse_command_line};
use crate::runtime::{parse_required_flag, validate_field_name, validate_model_name};
use crate::{
    BackendIdentity, GraphClient, GraphTx, InMemoryBackend, Result, RuntimeField, RuntimeNodeModel,
    RuntimeRelModel, RuntimeValueType, SessionModelCatalog, StoredNode, StoredRel,
};

pub struct SessionState {
    client: GraphClient<InMemoryBackend>,
    catalog: SessionModelCatalog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadSource {
    Primary,
    Backup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSession {
    graph: PersistedGraphStore,
    catalog: SessionModelCatalog,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BinaryPersistedSession {
    graph: BinaryPersistedGraphStore,
    catalog: SessionModelCatalog,
}

#[derive(Debug, Clone, Serialize)]
struct InterchangeDocument {
    format: &'static str,
    version: u32,
    kind: &'static str,
    identity: InterchangeIdentity,
    schema: InterchangeSchema,
    data: InterchangeData,
}

#[derive(Debug, Clone, Serialize)]
struct InterchangeIdentity {
    node: &'static str,
    edge: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct InterchangeSchema {
    nodes: Vec<InterchangeNodeModel>,
    edges: Vec<InterchangeEdgeModel>,
}

#[derive(Debug, Clone, Serialize)]
struct InterchangeNodeModel {
    name: String,
    id_field: String,
    id_type: &'static str,
    fields: Vec<InterchangeField>,
}

#[derive(Debug, Clone, Serialize)]
struct InterchangeEdgeModel {
    name: String,
    from: String,
    to: String,
    id_field: String,
    id_type: &'static str,
    fields: Vec<InterchangeField>,
}

#[derive(Debug, Clone, Serialize)]
struct InterchangeField {
    name: String,
    #[serde(rename = "type")]
    value_type: &'static str,
    required: bool,
}

#[derive(Debug, Clone, Serialize)]
struct InterchangeData {
    nodes: Vec<InterchangeNode>,
    edges: Vec<InterchangeEdge>,
}

#[derive(Debug, Clone, Serialize)]
struct InterchangeNode {
    id: i64,
    model: String,
    props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
struct InterchangeEdge {
    id: i64,
    model: String,
    from: i64,
    to: i64,
    props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum SessionLogEntry {
    RegisterNodeModel { model: RuntimeNodeModel },
    RegisterRelModel { model: RuntimeRelModel },
    UpsertNode { node: StoredNode },
    DeleteNode { id: i64 },
    UpsertRel { rel: StoredRel },
    DeleteRel { id: i64 },
}

const AUTOCOMMIT_CHECKPOINT_INTERVAL: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionFileFormat {
    Json,
    Binary,
}

#[derive(Debug, Clone)]
struct AutocommitTarget {
    format: SessionFileFormat,
    path: PathBuf,
    pending_entries: usize,
}

#[derive(Debug, Clone)]
struct SessionPredicate {
    field: String,
    op: CompareOp,
    raw_value: String,
}

#[derive(Debug, Clone)]
struct SessionOrder {
    field: String,
    direction: SortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum OutputFormat {
    #[default]
    Default,
    Jsonl,
    Table,
    Graph,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Default)]
struct NodeFindQuery {
    predicates: Vec<SessionPredicate>,
    end_predicates: Vec<SessionPredicate>,
    edge_predicates: Vec<SessionPredicate>,
    traversals: Vec<SessionTraversalStep>,
    order: Vec<SessionOrder>,
    limit: Option<usize>,
    offset: Option<usize>,
    id_filter: Option<i64>,
    return_mode: SessionTraversalReturn,
    format: OutputFormat,
}

#[derive(Debug, Clone, Default)]
struct EdgeFindQuery {
    predicates: Vec<SessionPredicate>,
    order: Vec<SessionOrder>,
    limit: Option<usize>,
    offset: Option<usize>,
    id_filter: Option<i64>,
    from_filter: Option<i64>,
    to_filter: Option<i64>,
    format: OutputFormat,
}

#[derive(Debug, Clone)]
struct SessionTraversalStep {
    direction: Direction,
    rel_model_name: Option<String>,
    end_model_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SessionTraversalReturn {
    #[default]
    End,
    Root,
    Edge,
}

#[derive(Debug, Clone)]
enum SessionQueryResult {
    Nodes {
        model: RuntimeNodeModel,
        rows: Vec<StoredNode>,
    },
    Edges {
        model: RuntimeRelModel,
        rows: Vec<StoredRel>,
    },
    Graph(SessionGraphResult),
}

#[derive(Debug, Clone)]
struct SessionGraphResult {
    plan: RuntimeTraversalPlan,
    rows: Vec<crate::dsl::QueryRow>,
    return_mode: SessionTraversalReturn,
}

#[derive(Debug, Clone)]
struct GraphRenderPath {
    root: StoredNode,
    steps: Vec<GraphRenderStep>,
}

#[derive(Debug, Clone)]
struct GraphRenderStep {
    rel: StoredRel,
    node: StoredNode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionOutputMode {
    Interactive,
    Script,
}

#[derive(Debug, Clone, Default)]
struct ScriptSummary {
    created_node_types: Vec<String>,
    created_link_types: Vec<String>,
    inserted_nodes: BTreeMap<String, usize>,
    inserted_edges: BTreeMap<String, usize>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            client: GraphClient::new(InMemoryBackend::new()),
            catalog: SessionModelCatalog::new(),
        }
    }

    pub fn client(&self) -> &GraphClient<InMemoryBackend> {
        &self.client
    }

    pub fn catalog(&self) -> &SessionModelCatalog {
        &self.catalog
    }

    fn persisted_session(&self) -> PersistedSession {
        PersistedSession {
            graph: self.client.backend().snapshot_store().to_persisted(),
            catalog: self.catalog.clone(),
        }
    }

    fn interchange_document(&self) -> InterchangeDocument {
        let store = self.client.backend().snapshot_store();
        let node_models = self.catalog.list_node_models();
        let edge_models = self.catalog.list_rel_models();

        let schema = InterchangeSchema {
            nodes: node_models
                .iter()
                .map(|model| InterchangeNodeModel {
                    name: model.name.clone(),
                    id_field: model.id_field_name.clone(),
                    id_type: model.id_type.keyword(),
                    fields: interchange_fields(&model.fields),
                })
                .collect(),
            edges: edge_models
                .iter()
                .map(|model| InterchangeEdgeModel {
                    name: model.name.clone(),
                    from: model.from_model.clone(),
                    to: model.to_model.clone(),
                    id_field: model.id_field_name.clone(),
                    id_type: model.id_type.keyword(),
                    fields: interchange_fields(&model.fields),
                })
                .collect(),
        };

        let data = InterchangeData {
            nodes: store
                .nodes
                .values()
                .map(|node| InterchangeNode {
                    id: node.id,
                    model: self.interchange_node_model_name(node),
                    props: node.props.clone(),
                })
                .collect(),
            edges: store
                .rels
                .values()
                .map(|rel| InterchangeEdge {
                    id: rel.id,
                    model: self.interchange_edge_model_name(rel),
                    from: rel.from,
                    to: rel.to,
                    props: rel.props.clone(),
                })
                .collect(),
        };

        InterchangeDocument {
            format: "grm.interchange",
            version: 1,
            kind: "graph",
            identity: InterchangeIdentity {
                node: self.node_id_type().keyword(),
                edge: self.rel_id_type().keyword(),
            },
            schema,
            data,
        }
    }

    fn interchange_node_model_name(&self, node: &StoredNode) -> String {
        self.catalog
            .list_node_models()
            .into_iter()
            .find(|model| node.labels.iter().any(|label| label == &model.label))
            .map(|model| model.name.clone())
            .or_else(|| node.labels.first().cloned())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    fn interchange_edge_model_name(&self, rel: &StoredRel) -> String {
        self.catalog
            .list_rel_models()
            .into_iter()
            .find(|model| model.rel_type == rel.rel_type)
            .map(|model| model.name.clone())
            .unwrap_or_else(|| rel.rel_type.clone())
    }

    fn apply_persisted_session(&mut self, persisted: PersistedSession) {
        self.client
            .backend()
            .replace_store(GraphStore::from_persisted(persisted.graph));
        self.catalog = persisted.catalog;
    }

    pub fn save_to_json(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let json = serde_json::to_string_pretty(&self.persisted_session()).map_err(|_| {
            crate::error::GrmError::SaveAborted("failed to serialize session as JSON")
        })?;
        write_file_atomically_with_backup(path, json.as_bytes()).map_err(|_| {
            crate::error::GrmError::SaveAborted("failed to write JSON session file")
        })?;
        clear_session_log(path)
            .map_err(|_| crate::error::GrmError::SaveAborted("failed to clear session log file"))?;
        Ok(())
    }

    pub fn export_to_json(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let json = serde_json::to_string_pretty(&self.interchange_document()).map_err(|_| {
            crate::error::GrmError::SaveAborted("failed to serialize graph export as JSON")
        })?;
        write_file_atomically(path, json.as_bytes())
            .map_err(|_| crate::error::GrmError::SaveAborted("failed to write JSON export file"))?;
        Ok(())
    }

    pub fn save_to_binary(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let persisted = BinaryPersistedSession {
            graph: self
                .client
                .backend()
                .snapshot_store()
                .to_binary_persisted()?,
            catalog: self.catalog.clone(),
        };
        let bytes = bincode::serialize(&persisted).map_err(|_| {
            crate::error::GrmError::SaveAborted("failed to serialize session as binary")
        })?;
        write_file_atomically_with_backup(path, &bytes).map_err(|_| {
            crate::error::GrmError::SaveAborted("failed to write binary session file")
        })?;
        clear_session_log(path)
            .map_err(|_| crate::error::GrmError::SaveAborted("failed to clear session log file"))?;
        Ok(())
    }

    pub fn load_from_json(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.load_from_json_with_source(path).map(|_| ())
    }

    fn load_from_json_with_source(&mut self, path: impl AsRef<Path>) -> Result<LoadSource> {
        let path = path.as_ref();
        let json = fs::read_to_string(path)
            .map_err(|_| crate::error::GrmError::LoadAborted("failed to read JSON session file"))?;
        match serde_json::from_str::<PersistedSession>(&json) {
            Ok(persisted) => {
                self.apply_persisted_session(persisted);
                self.apply_session_log(path)?;
                Ok(LoadSource::Primary)
            }
            Err(_) => self.load_json_backup(path),
        }
    }

    pub fn load_from_binary(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.load_from_binary_with_source(path).map(|_| ())
    }

    fn load_from_binary_with_source(&mut self, path: impl AsRef<Path>) -> Result<LoadSource> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|_| {
            crate::error::GrmError::LoadAborted("failed to read binary session file")
        })?;
        match bincode::deserialize::<BinaryPersistedSession>(&bytes) {
            Ok(persisted) => {
                self.client
                    .backend()
                    .replace_store(GraphStore::from_binary_persisted(persisted.graph)?);
                self.catalog = persisted.catalog;
                self.apply_session_log(path)?;
                Ok(LoadSource::Primary)
            }
            Err(_) => self.load_binary_backup(path),
        }
    }

    pub fn register_model(&mut self, model: RuntimeNodeModel) -> Result<()> {
        self.catalog.register_node_model(model)
    }

    pub fn model_list(&self) -> Vec<&RuntimeNodeModel> {
        self.catalog.list_node_models()
    }

    pub fn model(&self, name: &str) -> Option<&RuntimeNodeModel> {
        self.catalog.get_node_model(name)
    }

    pub fn register_rel_model(&mut self, model: RuntimeRelModel) -> Result<()> {
        if self.catalog.get_node_model(&model.from_model).is_none() {
            return Err(crate::GrmError::Constraint(format!(
                "from model '{}' is not defined in this session",
                model.from_model
            )));
        }
        if self.catalog.get_node_model(&model.to_model).is_none() {
            return Err(crate::GrmError::Constraint(format!(
                "to model '{}' is not defined in this session",
                model.to_model
            )));
        }
        self.catalog.register_rel_model(model)
    }

    pub fn rel_model_list(&self) -> Vec<&RuntimeRelModel> {
        self.catalog.list_rel_models()
    }

    pub fn rel_model(&self, name: &str) -> Option<&RuntimeRelModel> {
        self.catalog.get_rel_model(name)
    }

    pub fn node_id_type(&self) -> crate::BackendIdType {
        self.client.backend().node_id_type()
    }

    pub fn rel_id_type(&self) -> crate::BackendIdType {
        self.client.backend().rel_id_type()
    }

    pub async fn create_instance(
        &self,
        model_name: &str,
        raw_values: &BTreeMap<String, String>,
    ) -> Result<StoredNode> {
        let model = self
            .catalog
            .get(model_name)
            .ok_or_else(|| crate::GrmError::NotFound)?;
        let props = model.validate_instance_input(raw_values)?;
        let mut tx = self.client.transaction().await?;
        let created = tx
            .tx_mut()?
            .create_node(vec![model.label.clone()], props)
            .await?;
        tx.commit().await?;
        Ok(created)
    }

    pub async fn create_relationship_instance(
        &self,
        model_name: &str,
        from_id: &str,
        to_id: &str,
        raw_values: &BTreeMap<String, String>,
    ) -> Result<StoredRel> {
        let model = self
            .catalog
            .get_rel_model(model_name)
            .ok_or(crate::GrmError::NotFound)?;
        let props = model.validate_instance_input(raw_values)?;
        let from_raw = self.parse_backend_id(from_id, self.node_id_type(), "from node")?;
        let to_raw = self.parse_backend_id(to_id, self.node_id_type(), "to node")?;

        let mut tx = self.client.transaction().await?;

        let from_node = tx
            .tx_mut()?
            .find_node_by_id(from_raw)
            .await?
            .ok_or_else(|| {
                crate::GrmError::Constraint(format!("from node '{}' was not found", from_raw))
            })?;
        if !from_node
            .labels
            .iter()
            .any(|label| label == &model.from_model)
        {
            return Err(crate::GrmError::Constraint(format!(
                "from node '{}' does not match model '{}'",
                from_raw, model.from_model
            )));
        }

        let to_node = tx.tx_mut()?.find_node_by_id(to_raw).await?.ok_or_else(|| {
            crate::GrmError::Constraint(format!("to node '{}' was not found", to_raw))
        })?;
        if !to_node.labels.iter().any(|label| label == &model.to_model) {
            return Err(crate::GrmError::Constraint(format!(
                "to node '{}' does not match model '{}'",
                to_raw, model.to_model
            )));
        }

        let created = tx
            .tx_mut()?
            .create_relationship(from_raw, to_raw, &model.rel_type, props)
            .await?;
        tx.commit().await?;
        Ok(created)
    }

    pub async fn update_node_instance(
        &self,
        model_name: &str,
        id: &str,
        raw_values: &BTreeMap<String, String>,
    ) -> Result<StoredNode> {
        let model = self
            .catalog
            .get_node_model(model_name)
            .ok_or(crate::GrmError::NotFound)?;
        let raw_id = self.parse_backend_id(id, self.node_id_type(), "node id")?;
        let props = self.parse_model_filters(raw_values, model)?;

        let mut tx = self.client.transaction().await?;
        let existing = tx.tx_mut()?.find_node_by_id(raw_id).await?.ok_or_else(|| {
            crate::GrmError::Constraint(format!("node '{}' was not found", raw_id))
        })?;
        if !existing.labels.iter().any(|label| label == &model.label) {
            return Err(crate::GrmError::Constraint(format!(
                "node '{}' does not match model '{}'",
                raw_id, model.name
            )));
        }

        let updated = tx
            .tx_mut()?
            .update_node(raw_id, props)
            .await?
            .ok_or_else(|| {
                crate::GrmError::Constraint(format!("node '{}' was not found", raw_id))
            })?;
        tx.commit().await?;
        Ok(updated)
    }

    pub async fn delete_node_instance(&self, model_name: &str, id: &str) -> Result<()> {
        let model = self
            .catalog
            .get_node_model(model_name)
            .ok_or(crate::GrmError::NotFound)?;
        let raw_id = self.parse_backend_id(id, self.node_id_type(), "node id")?;

        let mut tx = self.client.transaction().await?;
        let existing = tx.tx_mut()?.find_node_by_id(raw_id).await?.ok_or_else(|| {
            crate::GrmError::Constraint(format!("node '{}' was not found", raw_id))
        })?;
        if !existing.labels.iter().any(|label| label == &model.label) {
            return Err(crate::GrmError::Constraint(format!(
                "node '{}' does not match model '{}'",
                raw_id, model.name
            )));
        }

        tx.tx_mut()?.delete_node(raw_id).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn update_relationship_instance(
        &self,
        model_name: &str,
        id: &str,
        raw_values: &BTreeMap<String, String>,
    ) -> Result<StoredRel> {
        let model = self
            .catalog
            .get_rel_model(model_name)
            .ok_or(crate::GrmError::NotFound)?;
        let raw_id = self.parse_backend_id(id, self.rel_id_type(), "edge id")?;
        let props = self.parse_rel_filters(raw_values, model)?;

        let existing = self
            .find_relationships(
                model_name,
                &BTreeMap::from([(String::from("id"), raw_id.to_string())]),
            )?
            .into_iter()
            .next()
            .ok_or_else(|| {
                crate::GrmError::Constraint(format!("edge '{}' was not found", raw_id))
            })?;

        let mut tx = self.client.transaction().await?;
        let updated = tx
            .tx_mut()?
            .update_relationship(existing.id, props)
            .await?
            .ok_or_else(|| {
                crate::GrmError::Constraint(format!("edge '{}' was not found", raw_id))
            })?;
        tx.commit().await?;
        Ok(updated)
    }

    pub async fn delete_relationship_instance(&self, model_name: &str, id: &str) -> Result<()> {
        let raw_id = self.parse_backend_id(id, self.rel_id_type(), "edge id")?;
        let existing = self
            .find_relationships(
                model_name,
                &BTreeMap::from([(String::from("id"), raw_id.to_string())]),
            )?
            .into_iter()
            .next()
            .ok_or_else(|| {
                crate::GrmError::Constraint(format!("edge '{}' was not found", raw_id))
            })?;

        let mut tx = self.client.transaction().await?;
        tx.tx_mut()?.delete_relationship(existing.id).await?;
        tx.commit().await?;
        Ok(())
    }

    pub fn find_nodes(
        &self,
        model_name: &str,
        filters: &BTreeMap<String, String>,
    ) -> Result<Vec<StoredNode>> {
        let query = self.parse_node_find_query(model_name, filters)?;
        self.find_nodes_with_query(model_name, &query)
    }

    fn find_nodes_with_query(
        &self,
        model_name: &str,
        query: &NodeFindQuery,
    ) -> Result<Vec<StoredNode>> {
        let model = self
            .catalog
            .get_node_model(model_name)
            .ok_or(crate::GrmError::NotFound)?;
        let prop_filters = self.parse_model_predicates(&query.predicates, model)?;

        let mut nodes = self.client.backend().snapshot_nodes();
        nodes.retain(|node| node.labels.iter().any(|label| label == &model.label));

        if let Some(id) = query.id_filter {
            nodes.retain(|node| node.id == id);
        }

        nodes.retain(|node| matches_predicates(&node.props, &prop_filters));
        if !query.order.is_empty() {
            self.sort_nodes(&mut nodes, model, &query.order)?;
        }
        nodes = apply_offset_limit(nodes, query.offset, query.limit);
        Ok(nodes)
    }

    async fn execute_node_query(
        &self,
        model_name: &str,
        query: &NodeFindQuery,
    ) -> Result<SessionQueryResult> {
        if query.traversals.is_empty() {
            let rows = self.find_nodes_with_query(model_name, query)?;
            let model = self
                .catalog
                .get_node_model(model_name)
                .ok_or(crate::GrmError::NotFound)?
                .clone();
            return Ok(SessionQueryResult::Nodes { model, rows });
        }

        self.execute_node_traversal_query(model_name, query).await
    }

    async fn execute_node_traversal_query(
        &self,
        model_name: &str,
        query: &NodeFindQuery,
    ) -> Result<SessionQueryResult> {
        let root_model = self
            .catalog
            .get_node_model(model_name)
            .ok_or(crate::GrmError::NotFound)?
            .clone();
        let root_filters = self.parse_model_predicates(&query.predicates, &root_model)?;

        let plan = self.build_runtime_graph_query(&root_model, query)?;
        let mut tx = self.client.transaction().await?;
        let result = tx.tx_mut()?.execute_graph(&plan.graph_query).await?;
        tx.commit().await?;

        let end_filters = self.parse_model_predicates(&query.end_predicates, &plan.end_model)?;
        let edge_filters =
            self.parse_rel_predicates(&query.edge_predicates, &plan.return_rel_model)?;

        let filtered_rows = result
            .rows
            .into_iter()
            .filter(|row| {
                row.values
                    .get(&plan.root_var)
                    .and_then(|value| value.as_node())
                    .map(|node| matches_predicates(&node.props, &root_filters))
                    .unwrap_or(false)
            })
            .filter(|row| {
                if end_filters.is_empty() {
                    return true;
                }

                row.values
                    .get(&plan.end_var)
                    .and_then(|value| value.as_node())
                    .map(|node| matches_predicates(&node.props, &end_filters))
                    .unwrap_or(false)
            })
            .filter(|row| {
                if edge_filters.is_empty() {
                    return true;
                }

                row.values
                    .get(&plan.return_rel_var)
                    .and_then(|value| match value {
                        crate::dsl::KernelValue::Rel(rel) => Some(rel),
                        _ => None,
                    })
                    .map(|rel| matches_predicates(&rel.props, &edge_filters))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();

        if query.format == OutputFormat::Graph {
            let mut rows = filtered_rows;
            match plan.graph_query.return_kind() {
                ReturnKind::Node => {
                    let model = if query.return_mode == SessionTraversalReturn::Root {
                        &root_model
                    } else {
                        &plan.end_model
                    };
                    if !query.order.is_empty() {
                        sort_query_rows_by_node_return(
                            &mut rows,
                            &plan.graph_query,
                            model,
                            &query.order,
                        )?;
                    }
                }
                ReturnKind::Rel => {
                    if !query.order.is_empty() {
                        sort_query_rows_by_rel_return(
                            &mut rows,
                            &plan.graph_query,
                            &plan.return_rel_model,
                            &query.order,
                        )?;
                    }
                }
            }
            rows = apply_offset_limit(rows, query.offset, query.limit);

            return Ok(SessionQueryResult::Graph(SessionGraphResult {
                plan,
                rows,
                return_mode: query.return_mode,
            }));
        }

        match plan.graph_query.return_kind() {
            ReturnKind::Node => {
                let mut rows = filtered_rows
                    .into_iter()
                    .filter_map(|row| {
                        row.get_returned(&plan.graph_query)
                            .and_then(|value| value.as_node())
                            .map(stored_node_from_kernel)
                    })
                    .collect::<Vec<_>>();

                let model = if query.return_mode == SessionTraversalReturn::Root {
                    root_model
                } else {
                    plan.end_model.clone()
                };

                if !query.order.is_empty() {
                    self.sort_nodes(&mut rows, &model, &query.order)?;
                }
                rows = apply_offset_limit(rows, query.offset, query.limit);

                Ok(SessionQueryResult::Nodes { model, rows })
            }
            ReturnKind::Rel => {
                let mut rows = filtered_rows
                    .into_iter()
                    .filter_map(|row| {
                        row.get_returned(&plan.graph_query)
                            .and_then(|value| match value {
                                crate::dsl::KernelValue::Rel(rel) => {
                                    Some(stored_rel_from_kernel(rel))
                                }
                                _ => None,
                            })
                    })
                    .collect::<Vec<_>>();

                if !query.order.is_empty() {
                    self.sort_relationships(&mut rows, &plan.return_rel_model, &query.order)?;
                }
                rows = apply_offset_limit(rows, query.offset, query.limit);

                Ok(SessionQueryResult::Edges {
                    model: plan.return_rel_model,
                    rows,
                })
            }
        }
    }

    pub fn find_relationships(
        &self,
        model_name: &str,
        filters: &BTreeMap<String, String>,
    ) -> Result<Vec<StoredRel>> {
        let query = self.parse_edge_find_query(model_name, filters)?;
        self.find_relationships_with_query(model_name, &query)
    }

    fn find_relationships_with_query(
        &self,
        model_name: &str,
        query: &EdgeFindQuery,
    ) -> Result<Vec<StoredRel>> {
        let model = self
            .catalog
            .get_rel_model(model_name)
            .ok_or(crate::GrmError::NotFound)?;
        let prop_filters = self.parse_rel_predicates(&query.predicates, model)?;

        let mut rels = self.client.backend().snapshot_relationships();
        rels.retain(|rel| rel.rel_type == model.rel_type);

        if let Some(id) = query.id_filter {
            rels.retain(|rel| rel.id == id);
        }
        if let Some(from) = query.from_filter {
            rels.retain(|rel| rel.from == from);
        }
        if let Some(to) = query.to_filter {
            rels.retain(|rel| rel.to == to);
        }

        rels.retain(|rel| matches_predicates(&rel.props, &prop_filters));
        if !query.order.is_empty() {
            self.sort_relationships(&mut rels, model, &query.order)?;
        }
        rels = apply_offset_limit(rels, query.offset, query.limit);
        Ok(rels)
    }

    fn parse_model_predicates(
        &self,
        predicates: &[SessionPredicate],
        model: &RuntimeNodeModel,
    ) -> Result<Vec<(String, CompareOp, Value)>> {
        let mut parsed = Vec::new();
        for predicate in predicates {
            let Some(field) = model.field(&predicate.field) else {
                return Err(crate::GrmError::Constraint(format!(
                    "unknown field '{}' for model '{}'",
                    predicate.field, model.name
                )));
            };

            parsed.push((
                predicate.field.clone(),
                predicate.op,
                field.value_type.parse_value(&predicate.raw_value)?,
            ));
        }
        Ok(parsed)
    }

    fn parse_model_filters(
        &self,
        filters: &BTreeMap<String, String>,
        model: &RuntimeNodeModel,
    ) -> Result<BTreeMap<String, Value>> {
        let mut parsed = BTreeMap::new();
        for (key, raw) in filters {
            if key == "id" || key == &model.id_field_name {
                continue;
            }

            let Some(field) = model.field(key) else {
                return Err(crate::GrmError::Constraint(format!(
                    "unknown field '{}' for model '{}'",
                    key, model.name
                )));
            };

            parsed.insert(key.clone(), field.value_type.parse_value(raw)?);
        }
        Ok(parsed)
    }

    fn parse_rel_predicates(
        &self,
        predicates: &[SessionPredicate],
        model: &RuntimeRelModel,
    ) -> Result<Vec<(String, CompareOp, Value)>> {
        let mut parsed = Vec::new();
        for predicate in predicates {
            let Some(field) = model.field(&predicate.field) else {
                return Err(crate::GrmError::Constraint(format!(
                    "unknown field '{}' for link '{}'",
                    predicate.field, model.name
                )));
            };

            parsed.push((
                predicate.field.clone(),
                predicate.op,
                field.value_type.parse_value(&predicate.raw_value)?,
            ));
        }
        Ok(parsed)
    }

    fn parse_rel_filters(
        &self,
        filters: &BTreeMap<String, String>,
        model: &RuntimeRelModel,
    ) -> Result<BTreeMap<String, Value>> {
        let mut parsed = BTreeMap::new();
        for (key, raw) in filters {
            if key == "id" || key == &model.id_field_name || key == "from" || key == "to" {
                continue;
            }

            let Some(field) = model.field(key) else {
                return Err(crate::GrmError::Constraint(format!(
                    "unknown field '{}' for link '{}'",
                    key, model.name
                )));
            };

            parsed.insert(key.clone(), field.value_type.parse_value(raw)?);
        }
        Ok(parsed)
    }

    fn parse_node_find_query(
        &self,
        model_name: &str,
        filters: &BTreeMap<String, String>,
    ) -> Result<NodeFindQuery> {
        let model = self
            .catalog
            .get_node_model(model_name)
            .ok_or(crate::GrmError::NotFound)?;
        parse_node_find_query(filters, model, self.node_id_type())
    }

    fn parse_node_find_terms(
        &self,
        model_name: &str,
        terms: &[QueryTerm],
    ) -> Result<NodeFindQuery> {
        let model = self
            .catalog
            .get_node_model(model_name)
            .ok_or(crate::GrmError::NotFound)?;
        parse_node_find_terms(terms, model, self.node_id_type())
    }

    fn parse_edge_find_query(
        &self,
        model_name: &str,
        filters: &BTreeMap<String, String>,
    ) -> Result<EdgeFindQuery> {
        let model = self
            .catalog
            .get_rel_model(model_name)
            .ok_or(crate::GrmError::NotFound)?;
        parse_edge_find_query(filters, model, self.rel_id_type(), self.node_id_type())
    }

    fn parse_backend_id(
        &self,
        raw: &str,
        id_type: crate::BackendIdType,
        subject: &str,
    ) -> Result<i64> {
        match id_type {
            crate::BackendIdType::Int64 => raw
                .trim()
                .parse::<i64>()
                .map_err(|_| crate::GrmError::Constraint(format!("{subject} must be an int id"))),
            crate::BackendIdType::Uuid => Err(crate::GrmError::NotSupported(
                "uuid runtime session ids are not supported by this backend yet",
            )),
        }
    }

    fn sort_nodes(
        &self,
        nodes: &mut [StoredNode],
        model: &RuntimeNodeModel,
        orders: &[SessionOrder],
    ) -> Result<()> {
        validate_node_order_fields(model, orders)?;
        nodes.sort_by(|left, right| compare_node_order_values(left, right, model, orders));
        Ok(())
    }

    fn sort_relationships(
        &self,
        rels: &mut [StoredRel],
        model: &RuntimeRelModel,
        orders: &[SessionOrder],
    ) -> Result<()> {
        validate_rel_order_fields(model, orders)?;
        rels.sort_by(|left, right| compare_rel_order_values(left, right, model, orders));
        Ok(())
    }

    fn build_runtime_graph_query(
        &self,
        root_model: &RuntimeNodeModel,
        query: &NodeFindQuery,
    ) -> Result<RuntimeTraversalPlan> {
        let mut vg = VarGen::default();
        let root_var = vg.fresh();
        let root_labels = leak_labels(&root_model.label);

        let mut matches = vec![MatchClause::Node(NodeMatch {
            var: root_var,
            labels: root_labels,
            id_filter: query.id_filter,
            property_filters: vec![],
        })];

        let mut current_var = root_var;
        let mut current_model = root_model.clone();
        let mut last_rel_var = None;
        let mut last_rel_model = None;

        for step in &query.traversals {
            let start_model = current_model.clone();
            let end_model = self
                .catalog
                .get_node_model(&step.end_model_name)
                .ok_or(crate::GrmError::Constraint(format!(
                    "unknown traversal end model '{}'",
                    step.end_model_name
                )))?
                .clone();

            let rel_model = match &step.rel_model_name {
                Some(name) => Some(
                    self.catalog
                        .get_rel_model(name)
                        .ok_or(crate::GrmError::Constraint(format!(
                            "unknown traversal link '{}'",
                            name
                        )))?
                        .clone(),
                ),
                None => resolve_any_traversal_model(
                    &self.catalog,
                    &start_model,
                    &end_model,
                    step.direction,
                )?,
            };

            if let Some(rel_model) = &rel_model {
                validate_traversal_step_models(
                    &start_model,
                    &end_model,
                    rel_model,
                    step.direction,
                )?;
            }

            let rel_var = vg.fresh();
            let end_var = vg.fresh();
            let rel_type = rel_model
                .as_ref()
                .map(|model| leak_string(model.rel_type.clone()));
            let end_labels = leak_labels(&end_model.label);

            matches.push(MatchClause::Hop(HopMatch {
                start: current_var,
                rel_type,
                rel_var,
                dir: step.direction,
                end: end_var,
                end_labels,
            }));
            matches.push(MatchClause::Node(NodeMatch {
                var: end_var,
                labels: end_labels,
                id_filter: None,
                property_filters: vec![],
            }));

            current_var = end_var;
            current_model = end_model;
            last_rel_var = Some(rel_var);
            last_rel_model = rel_model;
        }

        let return_value = match query.return_mode {
            SessionTraversalReturn::Root => Return::Node(root_var),
            SessionTraversalReturn::End => Return::Node(current_var),
            SessionTraversalReturn::Edge => Return::Rel(last_rel_var.ok_or_else(|| {
                crate::GrmError::Constraint(
                    "return=edge requires at least one traversal hop".into(),
                )
            })?),
        };

        let graph_query = GraphQuery {
            matches,
            where_: vec![],
            ret: return_value,
            limit: None,
            offset: None,
        };
        graph_query.validate()?;

        let return_rel_model = last_rel_model.ok_or_else(|| {
            crate::GrmError::Constraint(
                "traversal query requires at least one traversal hop".into(),
            )
        })?;

        Ok(RuntimeTraversalPlan {
            graph_query,
            root_var,
            end_var: current_var,
            return_rel_var: last_rel_var.unwrap(),
            end_model: current_model,
            return_rel_model,
        })
    }
}

fn interchange_fields(fields: &[RuntimeField]) -> Vec<InterchangeField> {
    fields
        .iter()
        .map(|field| InterchangeField {
            name: field.name.clone(),
            value_type: field.value_type.keyword(),
            required: field.required,
        })
        .collect()
}

#[derive(Debug, Clone)]
struct RuntimeTraversalPlan {
    graph_query: GraphQuery,
    root_var: crate::dsl::VarId,
    end_var: crate::dsl::VarId,
    return_rel_var: crate::dsl::VarId,
    end_model: RuntimeNodeModel,
    return_rel_model: RuntimeRelModel,
}

pub struct CliSession<R: BufRead, W: Write> {
    state: SessionState,
    reader: R,
    writer: W,
    prompt_name: &'static str,
    autocommit: Option<AutocommitTarget>,
    colors: SessionColors,
    output_mode: SessionOutputMode,
    script_summary: ScriptSummary,
}

impl<R: BufRead, W: Write> CliSession<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self::with_colors(SessionState::new(), reader, writer, SessionColors::plain())
    }

    pub fn new_with_color(reader: R, writer: W, enabled: bool) -> Self {
        Self::with_colors(
            SessionState::new(),
            reader,
            writer,
            SessionColors::for_terminal(enabled),
        )
    }

    pub fn with_state(state: SessionState, reader: R, writer: W) -> Self {
        Self::with_colors(state, reader, writer, SessionColors::plain())
    }

    pub fn with_state_and_color(state: SessionState, reader: R, writer: W, enabled: bool) -> Self {
        Self::with_colors(state, reader, writer, SessionColors::for_terminal(enabled))
    }

    fn with_colors(state: SessionState, reader: R, writer: W, colors: SessionColors) -> Self {
        Self {
            state,
            reader,
            writer,
            prompt_name: "session",
            autocommit: None,
            colors,
            output_mode: SessionOutputMode::Interactive,
            script_summary: ScriptSummary::default(),
        }
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn into_parts(self) -> (SessionState, R, W) {
        (self.state, self.reader, self.writer)
    }

    pub async fn run(&mut self) -> Result<()> {
        self.run_interactive_loop(
            "Welcome to GRM-RS CLI.\nFresh in-memory graph session started. Type 'session.help' for commands.",
        )
        .await
    }

    pub async fn continue_interactive(&mut self) -> Result<()> {
        self.run_interactive_loop(
            "Welcome to GRM-RS CLI.\nScript loaded. Entering interactive session. Type 'session.help' for commands.",
        )
        .await
    }

    pub async fn run_script(&mut self) -> Result<()> {
        self.prompt_name = "script";
        self.output_mode = SessionOutputMode::Script;
        self.script_summary = ScriptSummary::default();

        writeln!(self.writer, "Welcome to GRM-RS CLI.")?;
        writeln!(self.writer, "Running setup script...")?;

        loop {
            let Some(line) = self.read_command_line()? else {
                break;
            };

            let line = strip_script_comment(&line);
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let should_exit = self.handle_command(trimmed).await?;
            if should_exit {
                break;
            }
        }

        self.write_script_summary()?;

        self.prompt_name = "session";
        self.output_mode = SessionOutputMode::Interactive;

        Ok(())
    }

    async fn run_interactive_loop(&mut self, banner: &str) -> Result<()> {
        writeln!(self.writer, "{banner}")?;

        loop {
            self.write_prompt()?;
            let Some(line) = self.read_command_line()? else {
                writeln!(self.writer)?;
                break;
            };

            match self.handle_command(&line).await {
                Ok(should_exit) => {
                    if should_exit {
                        break;
                    }
                }
                Err(err) => {
                    writeln!(self.writer, "{err}")?;
                }
            }
        }

        Ok(())
    }

    pub async fn handle_command(&mut self, line: &str) -> Result<bool> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(false);
        }

        match parse_command_line(trimmed)? {
            SessionCommand::Help => self.write_help()?,
            SessionCommand::Exit => return Ok(true),
            SessionCommand::SessionDescribe => self.write_session_summary()?,
            SessionCommand::ModelDefine { args } => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                if args.is_empty() {
                    self.run_model_define_wizard().await?;
                } else {
                    self.handle_model_define_args(args.as_slice())?;
                }
            }
            SessionCommand::ModelList => self.write_model_list()?,
            SessionCommand::ModelShow { name } => self.write_model_show(&name)?,
            SessionCommand::LinkDefine { args } => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                if args.is_empty() {
                    self.run_link_define_wizard().await?;
                } else {
                    self.handle_link_define_args(args.as_slice())?;
                }
            }
            SessionCommand::LinkList => self.write_rel_model_list()?,
            SessionCommand::LinkShow { name } => self.write_rel_model_show(&name)?,
            SessionCommand::NodeCreate {
                model_name,
                assignments,
            } => {
                self.handle_node_create_parsed(&model_name, &assignments)
                    .await?
            }
            SessionCommand::NodeFind { model_name, terms } => {
                self.handle_node_find_parsed(&model_name, &terms).await?
            }
            SessionCommand::NodeUpdate {
                model_name,
                id,
                assignments,
            } => {
                self.handle_node_update_parsed(&model_name, &id, &assignments)
                    .await?
            }
            SessionCommand::NodeDelete { model_name, id } => {
                self.handle_node_delete_parsed(&model_name, &id).await?
            }
            SessionCommand::EdgeCreate {
                model_name,
                assignments,
            } => {
                self.handle_edge_create_parsed(&model_name, &assignments)
                    .await?
            }
            SessionCommand::EdgeFind { model_name, terms } => {
                self.handle_edge_find_parsed(&model_name, &terms)?
            }
            SessionCommand::EdgeUpdate {
                model_name,
                id,
                assignments,
            } => {
                self.handle_edge_update_parsed(&model_name, &id, &assignments)
                    .await?
            }
            SessionCommand::EdgeDelete { model_name, id } => {
                self.handle_edge_delete_parsed(&model_name, &id).await?
            }
            SessionCommand::SessionSave { args } => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                self.handle_session_save(args.as_slice())?
            }
            SessionCommand::SessionLoad { args } => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                self.handle_session_load(args.as_slice())?
            }
            SessionCommand::SessionExport { args } => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                self.handle_session_export(args.as_slice())?
            }
            SessionCommand::SessionAutocommit { args } => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                self.handle_session_autocommit(args.as_slice())?
            }
            SessionCommand::Unknown { .. } => writeln!(self.writer, "Unknown command: {trimmed}")?,
        }

        Ok(false)
    }

    fn write_prompt(&mut self) -> Result<()> {
        write!(self.writer, "grm({})> ", self.prompt_name)?;
        self.writer.flush()?;
        Ok(())
    }

    fn write_help(&mut self) -> Result<()> {
        writeln!(self.writer, "Available commands:")?;
        writeln!(
            self.writer,
            "  model.define [<Name> <id_field> [field:type:required|optional ...]]"
        )?;
        writeln!(self.writer, "  model.list")?;
        writeln!(self.writer, "  model.show <name>")?;
        writeln!(
            self.writer,
            "  link.define [<Name> <from_model> <to_model> <id_field> [field:type:required|optional ...]]"
        )?;
        writeln!(self.writer, "  link.list")?;
        writeln!(self.writer, "  link.show <name>")?;
        writeln!(self.writer, "  node.create <ModelName> [field=value ...]")?;
        writeln!(
            self.writer,
            "  node.find <ModelName> [field=value|field!=value|field>value|field>=value|field<value|field<=value|field~value ...] [order=<field>:asc|desc[,<field>:asc|desc ...]] [limit=<n>] [offset=<n>] [format=default|jsonl|table|graph]"
        )?;
        writeln!(
            self.writer,
            "  node.update <ModelName> <id> [field=value ...]"
        )?;
        writeln!(self.writer, "  node.delete <ModelName> <id>")?;
        writeln!(
            self.writer,
            "  edge.create <LinkName> from=<id> to=<id> [field=value ...]"
        )?;
        writeln!(
            self.writer,
            "  edge.find <LinkName> [from=<id>] [to=<id>] [field=value|field!=value|field>value|field>=value|field<value|field<=value|field~value ...] [order=<field>:asc|desc[,<field>:asc|desc ...]] [limit=<n>] [offset=<n>] [format=default|jsonl|table|graph]"
        )?;
        writeln!(
            self.writer,
            "  edge.update <LinkName> <id> [field=value ...]"
        )?;
        writeln!(self.writer, "  edge.delete <LinkName> <id>")?;
        writeln!(self.writer, "Examples:")?;
        writeln!(
            self.writer,
            "  node.update User 1 name=\"Alice Johnson\" age=43"
        )?;
        writeln!(self.writer, "  node.find User name=\"Alice Jones\"")?;
        writeln!(
            self.writer,
            "  node.find User name=\"Alice Jones\" via=out:Authored:Post"
        )?;
        writeln!(
            self.writer,
            "  node.find User name=\"Alice Jones\" via=out:Accessed:Post edge.accessedOn=2026-04-20 return=edge"
        )?;
        writeln!(
            self.writer,
            "  node.find User age>=21 order=age:desc,name:asc limit=10"
        )?;
        writeln!(self.writer, "  node.find User age>=21 format=jsonl")?;
        writeln!(self.writer, "  node.find User age>=21 format=table")?;
        writeln!(
            self.writer,
            "  edge.update Authored 1 authoredOn=2026-04-12"
        )?;
        writeln!(
            self.writer,
            "  edge.find Authored from=1 authoredOn>=2026-04-10 order=authoredOn:desc,to:asc"
        )?;
        writeln!(self.writer, "  session.save --json <path>")?;
        writeln!(self.writer, "  session.save --bin <path>")?;
        writeln!(self.writer, "  session.load --json <path>")?;
        writeln!(self.writer, "  session.load --bin <path>")?;
        writeln!(self.writer, "  session.export --json <path>")?;
        writeln!(self.writer, "  session.autocommit --json <path>")?;
        writeln!(self.writer, "  session.autocommit --bin <path>")?;
        writeln!(self.writer, "  session.autocommit status")?;
        writeln!(self.writer, "  session.autocommit off")?;
        writeln!(self.writer, "  session.describe")?;
        writeln!(self.writer, "  session.help")?;
        writeln!(self.writer, "  session.exit")?;
        Ok(())
    }

    fn write_model_list(&mut self) -> Result<()> {
        if self.output_mode == SessionOutputMode::Script {
            return Ok(());
        }
        let models = self.state.model_list();
        if models.is_empty() {
            writeln!(self.writer, "No models defined in this session.")?;
            return Ok(());
        }

        writeln!(self.writer, "Session models:")?;
        for model in models {
            writeln!(
                self.writer,
                "  {} [{} fields, label={}]",
                model.name,
                model.fields.len(),
                model.label
            )?;
        }
        Ok(())
    }

    fn write_rel_model_list(&mut self) -> Result<()> {
        if self.output_mode == SessionOutputMode::Script {
            return Ok(());
        }
        let models = self.state.rel_model_list();
        if models.is_empty() {
            writeln!(self.writer, "No links defined in this session.")?;
            return Ok(());
        }

        writeln!(self.writer, "Session links:")?;
        for model in models {
            writeln!(
                self.writer,
                "  {} [{} fields, {} -> {}, type={}]",
                model.name,
                model.fields.len(),
                model.from_model,
                model.to_model,
                model.rel_type
            )?;
        }
        Ok(())
    }

    fn write_model_show(&mut self, name: &str) -> Result<()> {
        if self.output_mode == SessionOutputMode::Script {
            return Ok(());
        }
        let Some(model) = self.state.model(name) else {
            writeln!(self.writer, "Model '{name}' not found.")?;
            return Ok(());
        };

        writeln!(self.writer, "Model: {}", model.name)?;
        writeln!(self.writer, "Label: {}", model.label)?;
        writeln!(
            self.writer,
            "Id: {} ({})",
            model.id_field_name,
            model.id_type.keyword()
        )?;
        if model.fields.is_empty() {
            writeln!(self.writer, "Fields: none")?;
            return Ok(());
        }

        writeln!(self.writer, "Fields:")?;
        for field in &model.fields {
            let req = if field.required {
                "required"
            } else {
                "optional"
            };
            writeln!(
                self.writer,
                "  {}: {} ({})",
                field.name,
                field.value_type.keyword(),
                req
            )?;
        }

        Ok(())
    }

    fn write_rel_model_show(&mut self, name: &str) -> Result<()> {
        if self.output_mode == SessionOutputMode::Script {
            return Ok(());
        }
        let Some(model) = self.state.rel_model(name) else {
            writeln!(self.writer, "Link '{name}' not found.")?;
            return Ok(());
        };

        writeln!(self.writer, "Link: {}", model.name)?;
        writeln!(self.writer, "Type: {}", model.rel_type)?;
        writeln!(self.writer, "From: {}", model.from_model)?;
        writeln!(self.writer, "To: {}", model.to_model)?;
        writeln!(
            self.writer,
            "Id: {} ({})",
            model.id_field_name,
            model.id_type.keyword()
        )?;
        if model.fields.is_empty() {
            writeln!(self.writer, "Fields: none")?;
            return Ok(());
        }

        writeln!(self.writer, "Fields:")?;
        for field in &model.fields {
            let req = if field.required {
                "required"
            } else {
                "optional"
            };
            writeln!(
                self.writer,
                "  {}: {} ({})",
                field.name,
                field.value_type.keyword(),
                req
            )?;
        }

        Ok(())
    }

    fn handle_model_define_args(&mut self, parts: &[&str]) -> Result<()> {
        if parts.len() < 2 {
            return Err(crate::GrmError::Constraint(
                "usage: model.define <Name> <id_field> [field:type:required|optional ...]".into(),
            ));
        }

        let name = parts[0];
        let id_field_name = parts[1];
        let mut fields = Vec::new();

        for field_spec in &parts[2..] {
            let segments: Vec<&str> = field_spec.split(':').collect();
            if segments.len() != 3 {
                return Err(crate::GrmError::Constraint(format!(
                    "invalid field spec '{}'; expected name:type:required|optional",
                    field_spec
                )));
            }

            let value_type = RuntimeValueType::parse_keyword(segments[1]).ok_or_else(|| {
                crate::GrmError::Constraint(format!(
                    "invalid field type '{}' in '{}'",
                    segments[1], field_spec
                ))
            })?;

            let required = match segments[2] {
                "required" => true,
                "optional" => false,
                _ => {
                    return Err(crate::GrmError::Constraint(format!(
                        "invalid field requirement '{}' in '{}'",
                        segments[2], field_spec
                    )));
                }
            };

            fields.push(RuntimeField {
                name: segments[0].to_string(),
                value_type,
                required,
            });
        }

        let model = RuntimeNodeModel::new(name, id_field_name, self.state.node_id_type(), fields)?;
        self.state.register_model(model.clone())?;
        self.script_summary
            .created_node_types
            .push(model.name.clone());
        self.persist_autocommit_entry(SessionLogEntry::RegisterNodeModel {
            model: model.clone(),
        })?;
        if self.output_mode != SessionOutputMode::Script {
            writeln!(self.writer, "Model '{}' created from script.", model.name)?;
        }
        Ok(())
    }

    fn handle_link_define_args(&mut self, parts: &[&str]) -> Result<()> {
        if parts.len() < 4 {
            return Err(crate::GrmError::Constraint(
                "usage: link.define <Name> <from_model> <to_model> <id_field> [field:type:required|optional ...]".into(),
            ));
        }

        let name = parts[0];
        let from_model = parts[1];
        let to_model = parts[2];
        let id_field_name = parts[3];
        let mut fields = Vec::new();

        for field_spec in &parts[4..] {
            fields.push(self.parse_field_spec(field_spec)?);
        }

        let model = RuntimeRelModel::new(
            name,
            from_model,
            to_model,
            id_field_name,
            self.state.rel_id_type(),
            fields,
        )?;
        self.state.register_rel_model(model.clone())?;
        self.script_summary
            .created_link_types
            .push(model.name.clone());
        self.persist_autocommit_entry(SessionLogEntry::RegisterRelModel {
            model: model.clone(),
        })?;
        if self.output_mode != SessionOutputMode::Script {
            writeln!(self.writer, "Link '{}' created from script.", model.name)?;
        }
        Ok(())
    }

    async fn run_model_define_wizard(&mut self) -> Result<()> {
        writeln!(self.writer, "Creating a runtime node model.")?;
        let name = self.prompt_model_name()?;
        let id_field_name = self.prompt_id_field_name()?;
        let fields = self.prompt_fields(&id_field_name)?;
        let model = RuntimeNodeModel::new(name, id_field_name, self.state.node_id_type(), fields)?;

        self.write_model_summary(&model)?;

        if !self.prompt_yes_no("Confirm model creation? [y/n]: ")? {
            writeln!(self.writer, "Model creation canceled.")?;
            return Ok(());
        }

        self.state.register_model(model.clone())?;
        self.persist_autocommit_entry(SessionLogEntry::RegisterNodeModel {
            model: model.clone(),
        })?;
        writeln!(self.writer, "Model '{}' created.", model.name)?;

        if self.prompt_yes_no("Create the first instance now? [y/n]: ")? {
            self.run_create_instance_wizard(&model.name).await?;
        }

        Ok(())
    }

    async fn run_link_define_wizard(&mut self) -> Result<()> {
        writeln!(self.writer, "Creating a link.")?;
        let name = self.prompt_model_name()?;
        let from_model = self.prompt_existing_node_model("From node model: ")?;
        let to_model = self.prompt_existing_node_model("To node model: ")?;
        let id_field_name = self.prompt_rel_id_field_name()?;
        let fields = self.prompt_fields(&id_field_name)?;
        let model = RuntimeRelModel::new(
            name,
            from_model,
            to_model,
            id_field_name,
            self.state.rel_id_type(),
            fields,
        )?;

        self.write_rel_model_summary(&model)?;

        if !self.prompt_yes_no("Confirm link creation? [y/n]: ")? {
            writeln!(self.writer, "Link creation canceled.")?;
            return Ok(());
        }

        self.state.register_rel_model(model.clone())?;
        self.persist_autocommit_entry(SessionLogEntry::RegisterRelModel {
            model: model.clone(),
        })?;
        writeln!(self.writer, "Link '{}' created.", model.name)?;

        if self.prompt_yes_no("Create the first link now? [y/n]: ")? {
            self.run_create_relationship_wizard(&model.name).await?;
        }

        Ok(())
    }

    async fn handle_node_create_parsed(
        &mut self,
        model_name: &str,
        assignments: &[KeyValueArg],
    ) -> Result<()> {
        let values = collect_assignments(assignments);
        let created = self.state.create_instance(model_name, &values).await?;
        let (model_name, model_id_field_name) = {
            let model = self
                .state
                .model(model_name)
                .ok_or(crate::GrmError::NotFound)?;
            (model.name.clone(), model.id_field_name.clone())
        };
        *self
            .script_summary
            .inserted_nodes
            .entry(model_name.clone())
            .or_insert(0) += 1;
        self.persist_autocommit_entry(SessionLogEntry::UpsertNode {
            node: created.clone(),
        })?;
        if self.output_mode != SessionOutputMode::Script {
            writeln!(
                self.writer,
                "Created node {} with backend id {}. {}={}.",
                model_name, created.id, model_id_field_name, created.id
            )?;
        }
        Ok(())
    }

    async fn handle_node_update_parsed(
        &mut self,
        model_name: &str,
        id: &str,
        assignments: &[KeyValueArg],
    ) -> Result<()> {
        let updated = self
            .state
            .update_node_instance(model_name, id, &collect_assignments(assignments))
            .await?;
        let (model_name, model_id_field_name) = {
            let model = self
                .state
                .model(model_name)
                .ok_or(crate::GrmError::NotFound)?;
            (model.name.clone(), model.id_field_name.clone())
        };
        self.persist_autocommit_entry(SessionLogEntry::UpsertNode {
            node: updated.clone(),
        })?;
        writeln!(
            self.writer,
            "Updated node {} {}={} {}",
            model_name,
            model_id_field_name,
            updated.id,
            format_props(&updated.props, &self.colors)
        )?;
        Ok(())
    }

    async fn handle_node_delete_parsed(&mut self, model_name: &str, id: &str) -> Result<()> {
        self.state.delete_node_instance(model_name, id).await?;
        let raw_id = self
            .state
            .parse_backend_id(id, self.state.node_id_type(), "node id")?;
        self.persist_autocommit_entry(SessionLogEntry::DeleteNode { id: raw_id })?;
        writeln!(self.writer, "Deleted node {} {}.", model_name, id)?;
        Ok(())
    }

    async fn handle_node_find_parsed(
        &mut self,
        model_name: &str,
        terms: &[QueryTerm],
    ) -> Result<()> {
        let query = self.state.parse_node_find_terms(model_name, terms)?;
        let result = self.state.execute_node_query(model_name, &query).await?;
        self.render_query_result(result, query.format)
    }

    async fn handle_edge_create_parsed(
        &mut self,
        model_name: &str,
        assignments: &[KeyValueArg],
    ) -> Result<()> {
        let mut values = collect_assignments(assignments);
        let from_id = values
            .remove("from")
            .ok_or_else(|| crate::GrmError::Constraint("edge.create requires from=<id>".into()))?;
        let to_id = values
            .remove("to")
            .ok_or_else(|| crate::GrmError::Constraint("edge.create requires to=<id>".into()))?;
        let created = self
            .state
            .create_relationship_instance(model_name, &from_id, &to_id, &values)
            .await?;
        let (rel_type, model_name, model_id_field_name) = {
            let model = self
                .state
                .rel_model(model_name)
                .ok_or(crate::GrmError::NotFound)?;
            (
                model.rel_type.clone(),
                model.name.clone(),
                model.id_field_name.clone(),
            )
        };
        *self
            .script_summary
            .inserted_edges
            .entry(model_name.clone())
            .or_insert(0) += 1;
        self.persist_autocommit_entry(SessionLogEntry::UpsertRel {
            rel: created.clone(),
        })?;
        if self.output_mode != SessionOutputMode::Script {
            writeln!(
                self.writer,
                "Created edge {} of type '{}'. {}={}.",
                created.id, rel_type, model_id_field_name, created.id
            )?;
        }
        Ok(())
    }

    async fn handle_edge_update_parsed(
        &mut self,
        model_name: &str,
        id: &str,
        assignments: &[KeyValueArg],
    ) -> Result<()> {
        let updated = self
            .state
            .update_relationship_instance(model_name, id, &collect_assignments(assignments))
            .await?;
        let (model_name, model_id_field_name) = {
            let model = self
                .state
                .rel_model(model_name)
                .ok_or(crate::GrmError::NotFound)?;
            (model.name.clone(), model.id_field_name.clone())
        };
        self.persist_autocommit_entry(SessionLogEntry::UpsertRel {
            rel: updated.clone(),
        })?;
        writeln!(
            self.writer,
            "Updated edge {} {}={} from={} to={} {}",
            model_name,
            model_id_field_name,
            updated.id,
            updated.from,
            updated.to,
            format_props(&updated.props, &self.colors)
        )?;
        Ok(())
    }

    async fn handle_edge_delete_parsed(&mut self, model_name: &str, id: &str) -> Result<()> {
        self.state
            .delete_relationship_instance(model_name, id)
            .await?;
        let raw_id = self
            .state
            .parse_backend_id(id, self.state.rel_id_type(), "edge id")?;
        self.persist_autocommit_entry(SessionLogEntry::DeleteRel { id: raw_id })?;
        writeln!(self.writer, "Deleted edge {} {}.", model_name, id)?;
        Ok(())
    }

    fn handle_edge_find_parsed(&mut self, model_name: &str, terms: &[QueryTerm]) -> Result<()> {
        let filters = collect_query_terms(terms);
        let query = self.state.parse_edge_find_query(model_name, &filters)?;
        let rels = self
            .state
            .find_relationships_with_query(model_name, &query)?;
        let model = self
            .state
            .rel_model(model_name)
            .ok_or(crate::GrmError::NotFound)?
            .clone();
        self.render_query_result(
            SessionQueryResult::Edges { model, rows: rels },
            query.format,
        )
    }

    fn handle_session_save(&mut self, args: &[&str]) -> Result<()> {
        if args.len() != 2 {
            return Err(crate::GrmError::Constraint(
                "usage: session.save --json <path> | session.save --bin <path>".into(),
            ));
        }

        match args[0] {
            "--json" => {
                self.state.save_to_json(args[1])?;
                writeln!(self.writer, "Saved session to JSON file '{}'.", args[1])?;
            }
            "--bin" => {
                self.state.save_to_binary(args[1])?;
                writeln!(self.writer, "Saved session to binary file '{}'.", args[1])?;
            }
            _ => {
                return Err(crate::GrmError::Constraint(
                    "usage: session.save --json <path> | session.save --bin <path>".into(),
                ));
            }
        }
        Ok(())
    }

    fn handle_session_load(&mut self, args: &[&str]) -> Result<()> {
        if args.len() != 2 {
            return Err(crate::GrmError::Constraint(
                "usage: session.load --json <path> | session.load --bin <path>".into(),
            ));
        }

        match args[0] {
            "--json" => {
                let source = self.state.load_from_json_with_source(args[1])?;
                self.checkpoint_autocommit()?;
                match source {
                    LoadSource::Primary => {
                        writeln!(self.writer, "Loaded session from JSON file '{}'.", args[1])?;
                    }
                    LoadSource::Backup => {
                        let backup = backup_path(args[1]);
                        writeln!(
                            self.writer,
                            "Recovered session from backup JSON file '{}'.",
                            backup.display()
                        )?;
                    }
                }
            }
            "--bin" => {
                let source = self.state.load_from_binary_with_source(args[1])?;
                self.checkpoint_autocommit()?;
                match source {
                    LoadSource::Primary => {
                        writeln!(
                            self.writer,
                            "Loaded session from binary file '{}'.",
                            args[1]
                        )?;
                    }
                    LoadSource::Backup => {
                        let backup = backup_path(args[1]);
                        writeln!(
                            self.writer,
                            "Recovered session from backup binary file '{}'.",
                            backup.display()
                        )?;
                    }
                }
            }
            _ => {
                return Err(crate::GrmError::Constraint(
                    "usage: session.load --json <path> | session.load --bin <path>".into(),
                ));
            }
        }
        Ok(())
    }

    fn handle_session_export(&mut self, args: &[&str]) -> Result<()> {
        if args.len() != 2 {
            return Err(crate::GrmError::Constraint(
                "usage: session.export --json <path>".into(),
            ));
        }

        match args[0] {
            "--json" => {
                self.state.export_to_json(args[1])?;
                writeln!(self.writer, "Exported graph to JSON file '{}'.", args[1])?;
            }
            _ => {
                return Err(crate::GrmError::Constraint(
                    "usage: session.export --json <path>".into(),
                ));
            }
        }
        Ok(())
    }

    fn handle_session_autocommit(&mut self, args: &[&str]) -> Result<()> {
        match args {
            ["status"] => {
                if let Some(target) = &self.autocommit {
                    writeln!(
                        self.writer,
                        "Autocommit is enabled: {} {}",
                        target.format.flag(),
                        target.path.display()
                    )?;
                } else {
                    writeln!(self.writer, "Autocommit is disabled.")?;
                }
            }
            ["off"] => {
                self.autocommit = None;
                writeln!(self.writer, "Autocommit disabled.")?;
            }
            [flag, path] => {
                let format = SessionFileFormat::from_flag(flag).ok_or_else(|| {
                    crate::GrmError::Constraint(
                        "usage: session.autocommit --json <path> | session.autocommit --bin <path> | session.autocommit status | session.autocommit off".into(),
                    )
                })?;
                self.autocommit = Some(AutocommitTarget {
                    format,
                    path: PathBuf::from(path),
                    pending_entries: 0,
                });
                self.checkpoint_autocommit()?;
                writeln!(
                    self.writer,
                    "Autocommit enabled: {} {}",
                    format.flag(),
                    path
                )?;
            }
            _ => {
                return Err(crate::GrmError::Constraint(
                    "usage: session.autocommit --json <path> | session.autocommit --bin <path> | session.autocommit status | session.autocommit off".into(),
                ))
            }
        }

        Ok(())
    }

    fn render_query_result(
        &mut self,
        result: SessionQueryResult,
        format: OutputFormat,
    ) -> Result<()> {
        if self.output_mode == SessionOutputMode::Script {
            return Ok(());
        }
        match format {
            OutputFormat::Default => self.render_default_query_result(result),
            OutputFormat::Jsonl => self.render_jsonl_query_result(result),
            OutputFormat::Table => self.render_table_query_result(result),
            OutputFormat::Graph => match result {
                SessionQueryResult::Graph(graph) => self.render_graph_query_result(graph),
                _ => Err(crate::GrmError::NotSupported(
                    "graph format is only supported for graph-shaped query results",
                )),
            },
        }
    }

    fn render_default_query_result(&mut self, result: SessionQueryResult) -> Result<()> {
        match result {
            SessionQueryResult::Nodes { model, rows } => {
                if rows.is_empty() {
                    writeln!(self.writer, "No nodes matched model '{}'.", model.name)?;
                    return Ok(());
                }

                writeln!(
                    self.writer,
                    "{} nodes matched model '{}'.",
                    rows.len(),
                    model.name
                )?;
                for node in rows {
                    writeln!(
                        self.writer,
                        "Node {} {}={} {}",
                        self.colors.type_name(&model.name),
                        self.colors.property_name(&model.id_field_name),
                        node.id,
                        format_props(&node.props, &self.colors)
                    )?;
                }
            }
            SessionQueryResult::Edges { model, rows } => {
                if rows.is_empty() {
                    writeln!(self.writer, "No edges matched link '{}'.", model.name)?;
                    return Ok(());
                }

                writeln!(
                    self.writer,
                    "{} edges matched link '{}'.",
                    rows.len(),
                    model.name
                )?;
                for rel in rows {
                    writeln!(
                        self.writer,
                        "Edge {} {}={} from={} to={} {}",
                        self.colors.type_name(&model.name),
                        self.colors.property_name(&model.id_field_name),
                        rel.id,
                        rel.from,
                        rel.to,
                        format_props(&rel.props, &self.colors)
                    )?;
                }
            }
            SessionQueryResult::Graph(_) => {
                return Err(crate::GrmError::NotSupported(
                    "graph results must use format=graph",
                ));
            }
        }

        Ok(())
    }

    fn render_jsonl_query_result(&mut self, result: SessionQueryResult) -> Result<()> {
        match result {
            SessionQueryResult::Nodes { model, rows } => {
                for node in rows {
                    writeln!(
                        self.writer,
                        "{}",
                        json!({
                            "kind": "node",
                            "model": model.name,
                            "id": node.id,
                            "labels": node.labels,
                            "props": node.props,
                        })
                    )?;
                }
            }
            SessionQueryResult::Edges { model, rows } => {
                for rel in rows {
                    writeln!(
                        self.writer,
                        "{}",
                        json!({
                            "kind": "edge",
                            "model": model.name,
                            "id": rel.id,
                            "from": rel.from,
                            "to": rel.to,
                            "type": rel.rel_type,
                            "props": rel.props,
                        })
                    )?;
                }
            }
            SessionQueryResult::Graph(_) => {
                return Err(crate::GrmError::NotSupported(
                    "graph results do not support jsonl output",
                ));
            }
        }

        Ok(())
    }

    fn render_table_query_result(&mut self, result: SessionQueryResult) -> Result<()> {
        match result {
            SessionQueryResult::Nodes { model, rows } => {
                let mut headers = vec![model.id_field_name.clone()];
                headers.extend(model.fields.iter().map(|field| field.name.clone()));
                let header_kinds = std::iter::once(TableHeaderKind::Property)
                    .chain(model.fields.iter().map(|_| TableHeaderKind::Property))
                    .collect::<Vec<_>>();

                let mut matrix = Vec::new();
                for node in rows {
                    let mut row = vec![node.id.to_string()];
                    for field in &model.fields {
                        row.push(format_table_value(
                            node.props.get(&field.name),
                            &self.colors,
                        ));
                    }
                    matrix.push(row);
                }

                write_table(
                    &mut self.writer,
                    &headers,
                    &header_kinds,
                    &matrix,
                    &self.colors,
                )?;
            }
            SessionQueryResult::Edges { model, rows } => {
                let mut headers = vec![
                    model.id_field_name.clone(),
                    "from".into(),
                    "to".into(),
                    "type".into(),
                ];
                headers.extend(model.fields.iter().map(|field| field.name.clone()));
                let header_kinds = vec![
                    TableHeaderKind::Property,
                    TableHeaderKind::Plain,
                    TableHeaderKind::Plain,
                    TableHeaderKind::Type,
                ]
                .into_iter()
                .chain(model.fields.iter().map(|_| TableHeaderKind::Property))
                .collect::<Vec<_>>();

                let mut matrix = Vec::new();
                for rel in rows {
                    let mut row = vec![
                        rel.id.to_string(),
                        rel.from.to_string(),
                        rel.to.to_string(),
                        self.colors.type_name(&rel.rel_type),
                    ];
                    for field in &model.fields {
                        row.push(format_table_value(rel.props.get(&field.name), &self.colors));
                    }
                    matrix.push(row);
                }

                write_table(
                    &mut self.writer,
                    &headers,
                    &header_kinds,
                    &matrix,
                    &self.colors,
                )?;
            }
            SessionQueryResult::Graph(_) => {
                return Err(crate::GrmError::NotSupported(
                    "graph results must use format=graph",
                ));
            }
        }

        Ok(())
    }

    fn render_graph_query_result(&mut self, graph: SessionGraphResult) -> Result<()> {
        let paths = build_graph_render_paths(&graph)?;
        let (node_count, rel_count) = count_graph_entries(&paths);
        writeln!(self.writer, "graph: {node_count} nodes, {rel_count} links")?;

        if paths.is_empty() {
            return Ok(());
        }

        let mut grouped = BTreeMap::<i64, Vec<GraphRenderPath>>::new();
        let mut roots = BTreeMap::<i64, StoredNode>::new();
        for path in paths {
            roots
                .entry(path.root.id)
                .or_insert_with(|| path.root.clone());
            grouped.entry(path.root.id).or_default().push(path);
        }

        let mut first_group = true;
        for (root_id, mut root_paths) in grouped {
            if !first_group {
                writeln!(self.writer)?;
            }
            first_group = false;

            root_paths.sort_by(|left, right| compare_graph_paths(left, right));
            let root = roots
                .get(&root_id)
                .expect("root path grouping must preserve root node");
            writeln!(self.writer, "* {}", self.format_graph_node(root, false))?;

            if root_paths.iter().all(|path| path.steps.is_empty()) {
                continue;
            }

            let mut seen_nodes = BTreeSet::new();
            seen_nodes.insert(root.id);

            if root_paths.len() == 1 {
                self.render_linear_graph_path(&root_paths[0], &mut seen_nodes)?;
            } else {
                writeln!(self.writer, "|\\")?;
                for path in &root_paths {
                    self.render_branch_graph_path(path, &mut seen_nodes)?;
                }
            }
        }

        match graph.return_mode {
            SessionTraversalReturn::Root
            | SessionTraversalReturn::End
            | SessionTraversalReturn::Edge => Ok(()),
        }
    }

    fn render_linear_graph_path(
        &mut self,
        path: &GraphRenderPath,
        seen_nodes: &mut BTreeSet<i64>,
    ) -> Result<()> {
        for step in &path.steps {
            writeln!(self.writer, "|")?;
            let already_seen = !seen_nodes.insert(step.node.id);
            writeln!(
                self.writer,
                "* {}",
                self.format_graph_step(step, already_seen)
            )?;
            if already_seen {
                break;
            }
        }
        Ok(())
    }

    fn render_branch_graph_path(
        &mut self,
        path: &GraphRenderPath,
        seen_nodes: &mut BTreeSet<i64>,
    ) -> Result<()> {
        for (index, step) in path.steps.iter().enumerate() {
            let prefix = if index == 0 { "| * " } else { "|   * " };
            let already_seen = !seen_nodes.insert(step.node.id);
            writeln!(
                self.writer,
                "{prefix}{}",
                self.format_graph_step(step, already_seen)
            )?;
            if already_seen {
                break;
            }
        }
        Ok(())
    }

    fn format_graph_step(&self, step: &GraphRenderStep, already_seen: bool) -> String {
        let rel = self.format_graph_rel(&step.rel);
        let node = self.format_graph_node(&step.node, already_seen);
        format!("{rel} -> {node}")
    }

    fn format_graph_node(&self, node: &StoredNode, already_seen: bool) -> String {
        let label = node
            .labels
            .first()
            .cloned()
            .unwrap_or_else(|| "Node".to_string());
        let head = format!("({}#{})", self.colors.type_name(&label), node.id);
        if already_seen {
            return format!("{head} [seen]");
        }

        let summary = format_graph_props(&node.props, 2, &self.colors);
        if summary.is_empty() {
            head
        } else {
            format!("{head} {summary}")
        }
    }

    fn format_graph_rel(&self, rel: &StoredRel) -> String {
        let head = format!("[{}#{}]", self.colors.type_name(&rel.rel_type), rel.id);
        let summary = format_graph_props(&rel.props, 2, &self.colors);
        if summary.is_empty() {
            head
        } else {
            format!("{head} {summary}")
        }
    }

    fn prompt_model_name(&mut self) -> Result<String> {
        loop {
            let name = self.prompt("Model name (PascalCase): ")?;
            match validate_model_name(&name) {
                Ok(()) => return Ok(name),
                Err(err) => writeln!(self.writer, "{err}")?,
            }
        }
    }

    fn write_script_summary(&mut self) -> Result<()> {
        writeln!(self.writer)?;
        writeln!(self.writer, "Script Summary")?;

        writeln!(self.writer, "Types created:")?;
        if self.script_summary.created_node_types.is_empty()
            && self.script_summary.created_link_types.is_empty()
        {
            writeln!(self.writer, "  none")?;
        } else {
            if !self.script_summary.created_node_types.is_empty() {
                let nodes = self
                    .script_summary
                    .created_node_types
                    .iter()
                    .map(|name| self.colors.type_name(name))
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(self.writer, "  nodes: {nodes}")?;
            }
            if !self.script_summary.created_link_types.is_empty() {
                let links = self
                    .script_summary
                    .created_link_types
                    .iter()
                    .map(|name| self.colors.type_name(name))
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(self.writer, "  links: {links}")?;
            }
        }

        writeln!(self.writer, "Inserted rows:")?;
        let headers = vec!["kind".into(), "type".into(), "inserted".into()];
        let header_kinds = vec![
            TableHeaderKind::Plain,
            TableHeaderKind::Type,
            TableHeaderKind::Property,
        ];
        let mut rows = Vec::new();
        for (name, count) in &self.script_summary.inserted_nodes {
            rows.push(vec![
                "node".into(),
                self.colors.type_name(name),
                count.to_string(),
            ]);
        }
        for (name, count) in &self.script_summary.inserted_edges {
            rows.push(vec![
                "edge".into(),
                self.colors.type_name(name),
                count.to_string(),
            ]);
        }

        if rows.is_empty() {
            writeln!(self.writer, "  none")?;
        } else {
            write_table(
                &mut self.writer,
                &headers,
                &header_kinds,
                &rows,
                &self.colors,
            )?;
        }

        Ok(())
    }

    fn write_session_summary(&mut self) -> Result<()> {
        writeln!(self.writer, "Session Summary")?;

        let node_models = self.state.model_list();
        let rel_models = self.state.rel_model_list();
        writeln!(self.writer, "Types defined:")?;
        if node_models.is_empty() && rel_models.is_empty() {
            writeln!(self.writer, "  none")?;
        } else {
            if !node_models.is_empty() {
                let nodes = node_models
                    .iter()
                    .map(|model| self.colors.type_name(&model.name))
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(self.writer, "  nodes: {nodes}")?;
            }
            if !rel_models.is_empty() {
                let links = rel_models
                    .iter()
                    .map(|model| self.colors.type_name(&model.name))
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(self.writer, "  links: {links}")?;
            }
        }

        let snapshot = self.state.client().backend().snapshot_store();
        writeln!(
            self.writer,
            "Stored rows: {} nodes, {} edges",
            snapshot.nodes.len(),
            snapshot.rels.len()
        )?;

        let headers = vec!["kind".into(), "type".into(), "count".into()];
        let header_kinds = vec![
            TableHeaderKind::Plain,
            TableHeaderKind::Type,
            TableHeaderKind::Property,
        ];
        let mut rows = Vec::new();

        let mut node_counts = BTreeMap::<String, usize>::new();
        for node in snapshot.nodes.values() {
            let label = node
                .labels
                .first()
                .cloned()
                .unwrap_or_else(|| "Node".to_string());
            *node_counts.entry(label).or_insert(0) += 1;
        }
        for (name, count) in node_counts {
            rows.push(vec![
                "node".into(),
                self.colors.type_name(&name),
                count.to_string(),
            ]);
        }

        let mut rel_counts = BTreeMap::<String, usize>::new();
        for rel in snapshot.rels.values() {
            *rel_counts.entry(rel.rel_type.clone()).or_insert(0) += 1;
        }
        for (name, count) in rel_counts {
            rows.push(vec![
                "edge".into(),
                self.colors.type_name(&name),
                count.to_string(),
            ]);
        }

        writeln!(self.writer, "By type:")?;
        if rows.is_empty() {
            writeln!(self.writer, "  none")?;
        } else {
            write_table(
                &mut self.writer,
                &headers,
                &header_kinds,
                &rows,
                &self.colors,
            )?;
        }

        if let Some(target) = &self.autocommit {
            writeln!(
                self.writer,
                "Autocommit: {} {}",
                target.format.flag(),
                target.path.display()
            )?;
        } else {
            writeln!(self.writer, "Autocommit: off")?;
        }

        Ok(())
    }

    fn prompt_id_field_name(&mut self) -> Result<String> {
        let id_type = self.state.node_id_type();
        loop {
            let name = self.prompt(&format!(
                "Id field name (backend type: {}): ",
                id_type.keyword()
            ))?;
            match validate_field_name(&name) {
                Ok(()) => return Ok(name),
                Err(err) => writeln!(self.writer, "{err}")?,
            }
        }
    }

    fn prompt_rel_id_field_name(&mut self) -> Result<String> {
        let id_type = self.state.rel_id_type();
        loop {
            let name = self.prompt(&format!(
                "Relationship id field name (backend type: {}): ",
                id_type.keyword()
            ))?;
            match validate_field_name(&name) {
                Ok(()) => return Ok(name),
                Err(err) => writeln!(self.writer, "{err}")?,
            }
        }
    }

    fn prompt_existing_node_model(&mut self, prompt: &str) -> Result<String> {
        loop {
            let name = self.prompt(prompt)?;
            match self.state.model(&name) {
                Some(_) => return Ok(name),
                None => writeln!(self.writer, "Node model '{}' is not defined.", name)?,
            }
        }
    }

    fn prompt_fields(&mut self, id_field_name: &str) -> Result<Vec<RuntimeField>> {
        let mut fields = Vec::new();

        loop {
            let prompt = if fields.is_empty() {
                "Field name (or 'done' to finish): "
            } else {
                "Next field name (or 'done' to finish): "
            };
            let field_name = self.prompt(prompt)?;
            if field_name.eq_ignore_ascii_case("done") {
                break;
            }

            if fields
                .iter()
                .any(|field: &RuntimeField| field.name == field_name)
            {
                writeln!(self.writer, "field '{}' is already defined", field_name)?;
                continue;
            }

            if field_name == id_field_name {
                writeln!(
                    self.writer,
                    "field '{}' is already reserved as the backend-assigned id field",
                    field_name
                )?;
                continue;
            }

            if let Err(err) = validate_field_name(&field_name) {
                writeln!(self.writer, "{err}")?;
                continue;
            }

            let value_type = self.prompt_value_type()?;
            let required = self.prompt_required_flag()?;
            fields.push(RuntimeField {
                name: field_name,
                value_type,
                required,
            });
        }

        Ok(fields)
    }

    fn prompt_value_type(&mut self) -> Result<RuntimeValueType> {
        loop {
            let raw = self.prompt("Field type [string|int|float|bool]: ")?;
            if let Some(value_type) = RuntimeValueType::parse_keyword(&raw) {
                return Ok(value_type);
            }
            writeln!(self.writer, "Invalid field type '{raw}'.")?;
        }
    }

    fn parse_field_spec(&self, field_spec: &str) -> Result<RuntimeField> {
        let segments: Vec<&str> = field_spec.split(':').collect();
        if segments.len() != 3 {
            return Err(crate::GrmError::Constraint(format!(
                "invalid field spec '{}'; expected name:type:required|optional",
                field_spec
            )));
        }

        let value_type = RuntimeValueType::parse_keyword(segments[1]).ok_or_else(|| {
            crate::GrmError::Constraint(format!(
                "invalid field type '{}' in '{}'",
                segments[1], field_spec
            ))
        })?;

        let required = match segments[2] {
            "required" => true,
            "optional" => false,
            _ => {
                return Err(crate::GrmError::Constraint(format!(
                    "invalid field requirement '{}' in '{}'",
                    segments[2], field_spec
                )));
            }
        };

        Ok(RuntimeField {
            name: segments[0].to_string(),
            value_type,
            required,
        })
    }

    fn checkpoint_autocommit(&mut self) -> Result<()> {
        let Some(target) = &mut self.autocommit else {
            return Ok(());
        };

        let path = target.path.clone();
        let format = target.format;
        match format {
            SessionFileFormat::Json => self.state.save_to_json(&target.path),
            SessionFileFormat::Binary => self.state.save_to_binary(&target.path),
        }
        .map_err(|err| {
            crate::GrmError::Backend(format!(
                "autocommit failed for '{}': {}",
                path.display(),
                err
            ))
        })?;
        target.pending_entries = 0;
        Ok(())
    }

    fn persist_autocommit_entry(&mut self, entry: SessionLogEntry) -> Result<()> {
        let Some(target) = &mut self.autocommit else {
            return Ok(());
        };

        append_session_log(&target.path, &entry).map_err(|err| {
            crate::GrmError::Backend(format!(
                "autocommit failed for '{}': {}",
                target.path.display(),
                err
            ))
        })?;
        target.pending_entries += 1;

        if target.pending_entries >= AUTOCOMMIT_CHECKPOINT_INTERVAL {
            self.checkpoint_autocommit()?;
        }

        Ok(())
    }

    fn prompt_required_flag(&mut self) -> Result<bool> {
        loop {
            let raw = self.prompt("Required? [y/n]: ")?;
            if let Some(required) = parse_required_flag(&raw) {
                return Ok(required);
            }
            writeln!(self.writer, "Please answer y/n.")?;
        }
    }

    fn write_model_summary(&mut self, model: &RuntimeNodeModel) -> Result<()> {
        writeln!(self.writer, "Model summary:")?;
        writeln!(self.writer, "  Name: {}", model.name)?;
        writeln!(self.writer, "  Label: {}", model.label)?;
        writeln!(
            self.writer,
            "  Id: {} ({}, backend-assigned)",
            model.id_field_name,
            model.id_type.keyword()
        )?;
        if model.fields.is_empty() {
            writeln!(self.writer, "  Fields: none")?;
        } else {
            writeln!(self.writer, "  Fields:")?;
            for field in &model.fields {
                let req = if field.required {
                    "required"
                } else {
                    "optional"
                };
                writeln!(
                    self.writer,
                    "    {}: {} ({})",
                    field.name,
                    field.value_type.keyword(),
                    req
                )?;
            }
        }
        Ok(())
    }

    fn write_rel_model_summary(&mut self, model: &RuntimeRelModel) -> Result<()> {
        writeln!(self.writer, "Link summary:")?;
        writeln!(self.writer, "  Name: {}", model.name)?;
        writeln!(self.writer, "  Type: {}", model.rel_type)?;
        writeln!(self.writer, "  From: {}", model.from_model)?;
        writeln!(self.writer, "  To: {}", model.to_model)?;
        writeln!(
            self.writer,
            "  Id: {} ({}, backend-assigned)",
            model.id_field_name,
            model.id_type.keyword()
        )?;
        if model.fields.is_empty() {
            writeln!(self.writer, "  Fields: none")?;
        } else {
            writeln!(self.writer, "  Fields:")?;
            for field in &model.fields {
                let req = if field.required {
                    "required"
                } else {
                    "optional"
                };
                writeln!(
                    self.writer,
                    "    {}: {} ({})",
                    field.name,
                    field.value_type.keyword(),
                    req
                )?;
            }
        }
        Ok(())
    }

    async fn run_create_instance_wizard(&mut self, model_name: &str) -> Result<()> {
        let Some(model) = self.state.model(model_name).cloned() else {
            writeln!(self.writer, "Model '{model_name}' not found.")?;
            return Ok(());
        };

        writeln!(self.writer, "Creating instance of '{}'.", model.name)?;
        let mut values = BTreeMap::new();
        for field in &model.fields {
            let prompt = if field.required {
                format!(
                    "Value for {} ({}, required): ",
                    field.name,
                    field.value_type.keyword()
                )
            } else {
                format!(
                    "Value for {} ({}, optional, blank to skip): ",
                    field.name,
                    field.value_type.keyword()
                )
            };

            loop {
                let raw = self.prompt(&prompt)?;
                if raw.is_empty() && !field.required {
                    break;
                }

                match field.value_type.parse_value(&raw) {
                    Ok(_) => {
                        values.insert(field.name.clone(), raw);
                        break;
                    }
                    Err(err) => writeln!(self.writer, "{err}")?,
                }
            }
        }

        let created = self.state.create_instance(&model.name, &values).await?;
        writeln!(
            self.writer,
            "Created node {} with label '{}'. {}={}.",
            created.id, model.label, model.id_field_name, created.id
        )?;
        Ok(())
    }

    async fn run_create_relationship_wizard(&mut self, model_name: &str) -> Result<()> {
        let Some(model) = self.state.rel_model(model_name).cloned() else {
            writeln!(self.writer, "Link '{model_name}' not found.")?;
            return Ok(());
        };

        writeln!(self.writer, "Creating link '{}'.", model.name)?;
        let from_id = self.prompt(&format!(
            "From node id for model {} ({}): ",
            model.from_model,
            self.state.node_id_type().keyword()
        ))?;
        let to_id = self.prompt(&format!(
            "To node id for model {} ({}): ",
            model.to_model,
            self.state.node_id_type().keyword()
        ))?;

        let mut values = BTreeMap::new();
        for field in &model.fields {
            let prompt = if field.required {
                format!(
                    "Value for {} ({}, required): ",
                    field.name,
                    field.value_type.keyword()
                )
            } else {
                format!(
                    "Value for {} ({}, optional, blank to skip): ",
                    field.name,
                    field.value_type.keyword()
                )
            };

            loop {
                let raw = self.prompt(&prompt)?;
                if raw.is_empty() && !field.required {
                    break;
                }

                match field.value_type.parse_value(&raw) {
                    Ok(_) => {
                        values.insert(field.name.clone(), raw);
                        break;
                    }
                    Err(err) => writeln!(self.writer, "{err}")?,
                }
            }
        }

        let created = self
            .state
            .create_relationship_instance(&model.name, &from_id, &to_id, &values)
            .await?;
        writeln!(
            self.writer,
            "Created relationship {} of type '{}'. {}={}.",
            created.id, model.rel_type, model.id_field_name, created.id
        )?;
        Ok(())
    }

    fn prompt_yes_no(&mut self, prompt: &str) -> Result<bool> {
        loop {
            let raw = self.prompt(prompt)?;
            if let Some(answer) = parse_required_flag(&raw) {
                return Ok(answer);
            }
            writeln!(self.writer, "Please answer y/n.")?;
        }
    }

    fn prompt(&mut self, prompt: &str) -> Result<String> {
        write!(self.writer, "{prompt}")?;
        self.writer.flush()?;
        let Some(line) = self.read_line()? else {
            return Err(crate::GrmError::Backend(
                "interactive session ended unexpectedly".into(),
            ));
        };
        Ok(line.trim().to_string())
    }

    fn read_command_line(&mut self) -> Result<Option<String>> {
        let Some(first_line) = self.read_line()? else {
            return Ok(None);
        };

        let mut combined = String::new();
        let mut current = first_line;

        loop {
            let physical = current.trim_end_matches(&['\r', '\n'][..]);
            let trimmed_end = physical.trim_end();

            if let Some(content) = trimmed_end.strip_suffix('\\') {
                combined.push_str(content);
                combined.push('\n');
                current = self.read_line()?.ok_or_else(|| {
                    crate::GrmError::Constraint("line continuation ended unexpectedly".into())
                })?;
                continue;
            }

            combined.push_str(physical);
            return Ok(Some(combined));
        }
    }

    fn read_line(&mut self) -> Result<Option<String>> {
        let mut line = String::new();
        let read = self.reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        Ok(Some(line))
    }
}

impl SessionState {
    fn apply_session_log(&mut self, path: &Path) -> Result<()> {
        let entries = read_session_log(path)?;
        if entries.is_empty() {
            return Ok(());
        }

        let mut store = self.client.backend().snapshot_store();
        let mut catalog = self.catalog.clone();
        for entry in &entries {
            apply_session_log_entry(&mut store, &mut catalog, entry).map_err(|_| {
                crate::error::GrmError::LoadAborted("failed to apply session log file")
            })?;
        }

        self.client.backend().replace_store(store);
        self.catalog = catalog;
        Ok(())
    }

    fn load_json_backup(&mut self, path: &Path) -> Result<LoadSource> {
        let backup = backup_path(path);
        let json = fs::read_to_string(&backup).map_err(|_| {
            crate::error::GrmError::LoadAborted("failed to deserialize JSON session file")
        })?;
        let persisted: PersistedSession = serde_json::from_str(&json).map_err(|_| {
            crate::error::GrmError::LoadAborted("failed to deserialize JSON session file")
        })?;
        self.apply_persisted_session(persisted);
        self.apply_session_log(path)?;
        Ok(LoadSource::Backup)
    }

    fn load_binary_backup(&mut self, path: &Path) -> Result<LoadSource> {
        let backup = backup_path(path);
        let bytes = fs::read(&backup).map_err(|_| {
            crate::error::GrmError::LoadAborted("failed to deserialize binary session file")
        })?;
        let persisted: BinaryPersistedSession = bincode::deserialize(&bytes).map_err(|_| {
            crate::error::GrmError::LoadAborted("failed to deserialize binary session file")
        })?;
        self.client
            .backend()
            .replace_store(GraphStore::from_binary_persisted(persisted.graph)?);
        self.catalog = persisted.catalog;
        self.apply_session_log(path)?;
        Ok(LoadSource::Backup)
    }
}

fn append_session_log(path: &Path, entry: &SessionLogEntry) -> io::Result<()> {
    let log_path = log_path(path);
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let line = serde_json::to_vec(entry).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "failed to serialize session log",
        )
    })?;
    file.write_all(&line)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn clear_session_log(path: &Path) -> io::Result<()> {
    let log_path = log_path(path);
    match fs::remove_file(log_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn read_session_log(path: &Path) -> Result<Vec<SessionLogEntry>> {
    let log_path = log_path(path);
    let contents = match fs::read_to_string(&log_path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => {
            return Err(crate::error::GrmError::LoadAborted(
                "failed to read session log file",
            ));
        }
    };

    let mut entries = Vec::new();
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        let entry = serde_json::from_str(line).map_err(|_| {
            crate::error::GrmError::LoadAborted("failed to deserialize session log file")
        })?;
        entries.push(entry);
    }
    Ok(entries)
}

fn apply_session_log_entry(
    store: &mut GraphStore,
    catalog: &mut SessionModelCatalog,
    entry: &SessionLogEntry,
) -> Result<()> {
    match entry {
        SessionLogEntry::RegisterNodeModel { model } => {
            if catalog.get_node_model(&model.name).is_none() {
                catalog.register_node_model(model.clone())?;
            }
        }
        SessionLogEntry::RegisterRelModel { model } => {
            if catalog.get_rel_model(&model.name).is_none() {
                catalog.register_rel_model(model.clone())?;
            }
        }
        SessionLogEntry::UpsertNode { node } => {
            store.next_node_id = store.next_node_id.max(node.id + 1);
            store.nodes.insert(node.id, node.clone());
        }
        SessionLogEntry::DeleteNode { id } => {
            store.nodes.remove(id);
            store.rels.retain(|_, rel| rel.from != *id && rel.to != *id);
        }
        SessionLogEntry::UpsertRel { rel } => {
            store.next_rel_id = store.next_rel_id.max(rel.id + 1);
            store.rels.insert(rel.id, rel.clone());
        }
        SessionLogEntry::DeleteRel { id } => {
            store.rels.remove(id);
        }
    }

    Ok(())
}

fn strip_script_comment(line: &str) -> String {
    let mut quote: Option<char> = None;
    let mut chars = line.char_indices().peekable();

    while let Some((index, ch)) = chars.next() {
        match quote {
            Some(q) => match ch {
                '\\' => {
                    chars.next();
                }
                _ if ch == q => quote = None,
                _ => {}
            },
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '#' => return line[..index].trim_end().to_string(),
                _ => {}
            },
        }
    }

    line.to_string()
}

impl From<io::Error> for crate::GrmError {
    fn from(err: io::Error) -> Self {
        crate::GrmError::Backend(err.to_string())
    }
}

fn matches_predicates(
    props: &BTreeMap<String, Value>,
    filters: &[(String, CompareOp, Value)],
) -> bool {
    filters.iter().all(|(key, op, value)| {
        props
            .get(key)
            .map(|stored| compare_values(stored, *op, value))
            .unwrap_or(false)
    })
}

fn format_props(props: &BTreeMap<String, Value>, colors: &SessionColors) -> String {
    if props.is_empty() {
        return "{}".into();
    }

    let mut parts = Vec::new();
    for (key, value) in props {
        parts.push(format!(
            "{}={}",
            colors.property_name(key),
            format_value(value, colors)
        ));
    }
    format!("{{{}}}", parts.join(" "))
}

fn format_graph_props(
    props: &BTreeMap<String, Value>,
    limit: usize,
    colors: &SessionColors,
) -> String {
    props
        .iter()
        .take(limit)
        .map(|(key, value)| {
            format!(
                "{}={}",
                colors.property_name(key),
                format_value(value, colors)
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_value(value: &Value, colors: &SessionColors) -> String {
    match value {
        Value::String(s) => {
            if s.contains(char::is_whitespace) {
                colors.string_value(&format!("\"{s}\""))
            } else {
                colors.string_value(s)
            }
        }
        _ => value.to_string(),
    }
}

fn format_table_value(value: Option<&Value>, colors: &SessionColors) -> String {
    match value {
        Some(value) => format_value(value, colors),
        None => String::new(),
    }
}

fn write_table<W: Write>(
    writer: &mut W,
    headers: &[String],
    header_kinds: &[TableHeaderKind],
    rows: &[Vec<String>],
    colors: &SessionColors,
) -> Result<()> {
    let mut widths: Vec<usize> = headers.iter().map(|header| header.len()).collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(visible_width(cell));
        }
    }

    let border = format_table_border(&widths);
    writeln!(writer, "{border}")?;
    writeln!(
        writer,
        "{}",
        format_table_header_row(headers, header_kinds, &widths, colors)
    )?;
    writeln!(writer, "{border}")?;
    for row in rows {
        writeln!(writer, "{}", format_table_row(row, &widths))?;
    }
    writeln!(writer, "{border}")?;
    Ok(())
}

fn format_table_border(widths: &[usize]) -> String {
    let mut line = String::new();
    line.push('+');
    for width in widths {
        let _ = write!(line, "{}+", "-".repeat(*width + 2));
    }
    line
}

fn format_table_row(cells: &[String], widths: &[usize]) -> String {
    let mut line = String::new();
    line.push('|');
    for (cell, width) in cells.iter().zip(widths.iter()) {
        let padding = width.saturating_sub(visible_width(cell));
        let _ = write!(line, " {}{} |", cell, " ".repeat(padding));
    }
    line
}

fn format_table_header_row(
    headers: &[String],
    header_kinds: &[TableHeaderKind],
    widths: &[usize],
    colors: &SessionColors,
) -> String {
    let styled = headers
        .iter()
        .zip(header_kinds.iter())
        .map(|(header, kind)| match kind {
            TableHeaderKind::Plain => header.clone(),
            TableHeaderKind::Property => colors.property_name(header),
            TableHeaderKind::Type => colors.type_name(header),
        })
        .collect::<Vec<_>>();
    format_table_row(&styled, widths)
}

#[derive(Debug, Clone, Copy)]
enum TableHeaderKind {
    Plain,
    Property,
    Type,
}

#[derive(Debug, Clone, Copy)]
struct SessionColors {
    enabled: bool,
}

impl SessionColors {
    const GREEN: &'static str = "\x1b[32m";
    const BLUE: &'static str = "\x1b[34m";
    const ORANGE: &'static str = "\x1b[38;5;208m";
    const RESET: &'static str = "\x1b[0m";

    fn plain() -> Self {
        Self { enabled: false }
    }

    fn for_terminal(enabled: bool) -> Self {
        Self { enabled }
    }

    fn type_name(&self, text: &str) -> String {
        self.wrap(text, Self::GREEN)
    }

    fn property_name(&self, text: &str) -> String {
        self.wrap(text, Self::BLUE)
    }

    fn string_value(&self, text: &str) -> String {
        self.wrap(text, Self::ORANGE)
    }

    fn wrap(&self, text: &str, color: &str) -> String {
        if self.enabled {
            format!("{color}{text}{}", Self::RESET)
        } else {
            text.to_string()
        }
    }
}

fn visible_width(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut width = 0;

    while index < bytes.len() {
        if bytes[index] == 0x1b {
            index += 1;
            if index < bytes.len() && bytes[index] == b'[' {
                index += 1;
                while index < bytes.len() && bytes[index] != b'm' {
                    index += 1;
                }
                if index < bytes.len() {
                    index += 1;
                }
                continue;
            }
        }

        if let Some(ch) = text[index..].chars().next() {
            width += 1;
            index += ch.len_utf8();
        } else {
            break;
        }
    }

    width
}

fn build_graph_render_paths(graph: &SessionGraphResult) -> Result<Vec<GraphRenderPath>> {
    let mut paths = Vec::new();

    for row in &graph.rows {
        let root = row
            .values
            .get(&graph.plan.root_var)
            .and_then(|value| value.as_node())
            .map(stored_node_from_kernel)
            .ok_or_else(|| crate::GrmError::Backend("graph result missing root node".into()))?;

        let mut steps = Vec::new();
        for clause in &graph.plan.graph_query.matches {
            if let MatchClause::Hop(hop) = clause {
                let rel = row
                    .values
                    .get(&hop.rel_var)
                    .and_then(|value| match value {
                        crate::dsl::KernelValue::Rel(rel) => Some(stored_rel_from_kernel(rel)),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        crate::GrmError::Backend("graph result missing relationship".into())
                    })?;
                let node = row
                    .values
                    .get(&hop.end)
                    .and_then(|value| value.as_node())
                    .map(stored_node_from_kernel)
                    .ok_or_else(|| {
                        crate::GrmError::Backend("graph result missing end node".into())
                    })?;
                steps.push(GraphRenderStep { rel, node });
            }
        }

        paths.push(GraphRenderPath { root, steps });
    }

    Ok(paths)
}

fn count_graph_entries(paths: &[GraphRenderPath]) -> (usize, usize) {
    let mut nodes = BTreeSet::new();
    let mut rels = BTreeSet::new();
    for path in paths {
        nodes.insert(path.root.id);
        for step in &path.steps {
            rels.insert(step.rel.id);
            nodes.insert(step.node.id);
        }
    }
    (nodes.len(), rels.len())
}

fn compare_graph_paths(left: &GraphRenderPath, right: &GraphRenderPath) -> std::cmp::Ordering {
    for (left_step, right_step) in left.steps.iter().zip(right.steps.iter()) {
        let rel_order = left_step.rel.id.cmp(&right_step.rel.id);
        if rel_order != std::cmp::Ordering::Equal {
            return rel_order;
        }

        let node_order = left_step.node.id.cmp(&right_step.node.id);
        if node_order != std::cmp::Ordering::Equal {
            return node_order;
        }
    }

    left.steps.len().cmp(&right.steps.len())
}

fn sort_query_rows_by_node_return(
    rows: &mut [crate::dsl::QueryRow],
    graph_query: &GraphQuery,
    model: &RuntimeNodeModel,
    orders: &[SessionOrder],
) -> Result<()> {
    validate_node_order_fields(model, orders)?;
    rows.sort_by(|left, right| {
        let left_node = left
            .get_returned(graph_query)
            .and_then(|value| value.as_node())
            .map(stored_node_from_kernel);
        let right_node = right
            .get_returned(graph_query)
            .and_then(|value| value.as_node())
            .map(stored_node_from_kernel);
        compare_optional_nodes(left_node.as_ref(), right_node.as_ref(), model, orders)
    });
    Ok(())
}

fn sort_query_rows_by_rel_return(
    rows: &mut [crate::dsl::QueryRow],
    graph_query: &GraphQuery,
    model: &RuntimeRelModel,
    orders: &[SessionOrder],
) -> Result<()> {
    validate_rel_order_fields(model, orders)?;
    rows.sort_by(|left, right| {
        let left_rel = left
            .get_returned(graph_query)
            .and_then(|value| match value {
                crate::dsl::KernelValue::Rel(rel) => Some(stored_rel_from_kernel(rel)),
                _ => None,
            });
        let right_rel = right
            .get_returned(graph_query)
            .and_then(|value| match value {
                crate::dsl::KernelValue::Rel(rel) => Some(stored_rel_from_kernel(rel)),
                _ => None,
            });
        compare_optional_rels(left_rel.as_ref(), right_rel.as_ref(), model, orders)
    });
    Ok(())
}

fn compare_optional_nodes(
    left: Option<&StoredNode>,
    right: Option<&StoredNode>,
    model: &RuntimeNodeModel,
    orders: &[SessionOrder],
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => compare_node_order_values(left, right, model, orders),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn compare_optional_rels(
    left: Option<&StoredRel>,
    right: Option<&StoredRel>,
    model: &RuntimeRelModel,
    orders: &[SessionOrder],
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => compare_rel_order_values(left, right, model, orders),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn parse_node_find_query(
    filters: &BTreeMap<String, String>,
    model: &RuntimeNodeModel,
    id_type: crate::BackendIdType,
) -> Result<NodeFindQuery> {
    let terms = filters
        .iter()
        .map(|(key, value)| QueryTerm {
            key: key.clone(),
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    parse_node_find_terms(&terms, model, id_type)
}

fn parse_node_find_terms(
    terms: &[QueryTerm],
    model: &RuntimeNodeModel,
    id_type: crate::BackendIdType,
) -> Result<NodeFindQuery> {
    let mut query = NodeFindQuery::default();
    for term in terms {
        let raw_key = term.key.as_str();
        let raw_value = term.value.as_str();
        match raw_key {
            "format" => query.format = OutputFormat::parse(raw_value)?,
            "limit" => query.limit = Some(parse_usize_term(raw_value, "limit")?),
            "offset" => query.offset = Some(parse_usize_term(raw_value, "offset")?),
            "order" => query.order = parse_order_term(raw_value)?,
            "via" => query.traversals.push(parse_traversal_step(raw_value)?),
            "return" => query.return_mode = SessionTraversalReturn::parse(raw_value)?,
            key if key == "id" || key == model.id_field_name => {
                query.id_filter = Some(parse_backend_id(raw_value, id_type, key)?);
            }
            _ if raw_key.starts_with("end.") => {
                let inner = raw_key.trim_start_matches("end.");
                let (field, op) = split_predicate_key(inner)?;
                query.end_predicates.push(SessionPredicate {
                    field: field.to_string(),
                    op,
                    raw_value: raw_value.to_string(),
                });
            }
            _ if raw_key.starts_with("edge.") || raw_key.starts_with("rel.") => {
                let inner = raw_key
                    .strip_prefix("edge.")
                    .or_else(|| raw_key.strip_prefix("rel."))
                    .unwrap_or(raw_key);
                let (field, op) = split_predicate_key(inner)?;
                query.edge_predicates.push(SessionPredicate {
                    field: field.to_string(),
                    op,
                    raw_value: raw_value.to_string(),
                });
            }
            _ => {
                let (field, op) = split_predicate_key(raw_key)?;
                if field == "id" || field == model.id_field_name {
                    return Err(crate::GrmError::Constraint(format!(
                        "backend id filter '{}' only supports '='",
                        field
                    )));
                }
                query.predicates.push(SessionPredicate {
                    field: field.to_string(),
                    op,
                    raw_value: raw_value.to_string(),
                });
            }
        }
    }
    if query.traversals.is_empty() {
        if !query.end_predicates.is_empty() || !query.edge_predicates.is_empty() {
            return Err(crate::GrmError::Constraint(
                "traversal filters require at least one via= traversal".into(),
            ));
        }
        if query.return_mode != SessionTraversalReturn::End {
            return Err(crate::GrmError::Constraint(
                "return=root|end|edge is only supported with via= traversal".into(),
            ));
        }
    }
    Ok(query)
}

fn parse_edge_find_query(
    filters: &BTreeMap<String, String>,
    model: &RuntimeRelModel,
    rel_id_type: crate::BackendIdType,
    node_id_type: crate::BackendIdType,
) -> Result<EdgeFindQuery> {
    let mut query = EdgeFindQuery::default();
    for (raw_key, raw_value) in filters {
        match raw_key.as_str() {
            "format" => query.format = OutputFormat::parse(raw_value)?,
            "limit" => query.limit = Some(parse_usize_term(raw_value, "limit")?),
            "offset" => query.offset = Some(parse_usize_term(raw_value, "offset")?),
            "order" => query.order = parse_order_term(raw_value)?,
            "from" => query.from_filter = Some(parse_backend_id(raw_value, node_id_type, "from")?),
            "to" => query.to_filter = Some(parse_backend_id(raw_value, node_id_type, "to")?),
            key if key == "id" || key == model.id_field_name => {
                query.id_filter = Some(parse_backend_id(raw_value, rel_id_type, key)?);
            }
            _ => {
                let (field, op) = split_predicate_key(raw_key)?;
                if field == "id" || field == model.id_field_name || field == "from" || field == "to"
                {
                    return Err(crate::GrmError::Constraint(format!(
                        "special filter '{}' only supports '='",
                        field
                    )));
                }
                query.predicates.push(SessionPredicate {
                    field: field.to_string(),
                    op,
                    raw_value: raw_value.clone(),
                });
            }
        }
    }
    Ok(query)
}

fn split_predicate_key(raw_key: &str) -> Result<(&str, CompareOp)> {
    for (suffix, op) in [
        ("!", "Ne"),
        (">=", "Ge"),
        ("<=", "Le"),
        (">", "Gt"),
        ("<", "Lt"),
        ("~", "Contains"),
    ] {
        if let Some(field) = raw_key.strip_suffix(suffix) {
            if field.is_empty() {
                break;
            }
            let op = match op {
                "Ne" => CompareOp::Ne,
                "Ge" => CompareOp::Ge,
                "Le" => CompareOp::Le,
                "Gt" => CompareOp::Gt,
                "Lt" => CompareOp::Lt,
                _ => CompareOp::Contains,
            };
            return Ok((field, op));
        }
    }

    Ok((raw_key, CompareOp::Eq))
}

fn parse_order_term(raw: &str) -> Result<Vec<SessionOrder>> {
    let mut orders = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for segment in raw.split(',') {
        let Some((field, direction)) = segment.split_once(':') else {
            return Err(crate::GrmError::Constraint(
                "order must use order=<field>:asc|desc[,<field>:asc|desc ...]".into(),
            ));
        };

        let direction = match direction {
            "asc" => SortDirection::Asc,
            "desc" => SortDirection::Desc,
            _ => {
                return Err(crate::GrmError::Constraint(
                    "order direction must be asc or desc".into(),
                ));
            }
        };

        if !seen.insert(field.to_string()) {
            return Err(crate::GrmError::Constraint(format!(
                "duplicate order field '{}'",
                field
            )));
        }

        orders.push(SessionOrder {
            field: field.to_string(),
            direction,
        });
    }

    Ok(orders)
}

fn parse_usize_term(raw: &str, subject: &str) -> Result<usize> {
    raw.parse::<usize>().map_err(|_| {
        crate::GrmError::Constraint(format!("{subject} must be a non-negative integer"))
    })
}

fn parse_backend_id(raw: &str, id_type: crate::BackendIdType, subject: &str) -> Result<i64> {
    match id_type {
        crate::BackendIdType::Int64 => raw
            .trim()
            .parse::<i64>()
            .map_err(|_| crate::GrmError::Constraint(format!("{subject} must be an int id"))),
        crate::BackendIdType::Uuid => Err(crate::GrmError::NotSupported(
            "uuid runtime session ids are not supported by this backend yet",
        )),
    }
}

fn compare_values(left: &Value, op: CompareOp, right: &Value) -> bool {
    match op {
        CompareOp::Eq => left == right,
        CompareOp::Ne => left != right,
        CompareOp::Gt => numeric_cmp(left, right, |a, b| a > b),
        CompareOp::Ge => numeric_cmp(left, right, |a, b| a >= b),
        CompareOp::Lt => numeric_cmp(left, right, |a, b| a < b),
        CompareOp::Le => numeric_cmp(left, right, |a, b| a <= b),
        CompareOp::Contains => match (left.as_str(), right.as_str()) {
            (Some(lhs), Some(rhs)) => lhs.contains(rhs),
            _ => false,
        },
    }
}

impl OutputFormat {
    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "default" => Ok(Self::Default),
            "jsonl" => Ok(Self::Jsonl),
            "table" => Ok(Self::Table),
            "graph" => Ok(Self::Graph),
            _ => Err(crate::GrmError::Constraint(
                "format must be one of: default, jsonl, table, graph".into(),
            )),
        }
    }
}

impl SessionTraversalReturn {
    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "end" => Ok(Self::End),
            "root" => Ok(Self::Root),
            "edge" | "rel" => Ok(Self::Edge),
            _ => Err(crate::GrmError::Constraint(
                "return must be one of: root, end, edge".into(),
            )),
        }
    }
}

fn parse_traversal_step(raw: &str) -> Result<SessionTraversalStep> {
    let mut parts = raw.split(':');
    let direction = match parts.next() {
        Some("out") | Some("outgoing") => Direction::Out,
        Some("in") | Some("incoming") => Direction::In,
        Some("both") => Direction::Both,
        _ => {
            return Err(crate::GrmError::Constraint(
                "via must use via=<out|in|both>:<LinkName|*>:<EndModel>".into(),
            ));
        }
    };

    let rel_model_name = match parts.next() {
        Some("*") => None,
        Some(name) if !name.is_empty() => Some(name.to_string()),
        _ => {
            return Err(crate::GrmError::Constraint(
                "via must use via=<out|in|both>:<LinkName|*>:<EndModel>".into(),
            ));
        }
    };

    let end_model_name = match parts.next() {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => {
            return Err(crate::GrmError::Constraint(
                "via must use via=<out|in|both>:<LinkName|*>:<EndModel>".into(),
            ));
        }
    };

    if parts.next().is_some() {
        return Err(crate::GrmError::Constraint(
            "via must use via=<out|in|both>:<LinkName|*>:<EndModel>".into(),
        ));
    }

    Ok(SessionTraversalStep {
        direction,
        rel_model_name,
        end_model_name,
    })
}

fn leak_string(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn leak_labels(label: &str) -> &'static [&'static str] {
    let leaked_label = leak_string(label.to_string());
    Box::leak(vec![leaked_label].into_boxed_slice())
}

fn validate_traversal_step_models(
    start_model: &RuntimeNodeModel,
    end_model: &RuntimeNodeModel,
    rel_model: &RuntimeRelModel,
    direction: Direction,
) -> Result<()> {
    if traversal_step_matches_models(start_model, end_model, rel_model, direction) {
        return Ok(());
    }

    Err(crate::GrmError::Constraint(format!(
        "link '{}' does not connect {} to {} in {:?} direction",
        rel_model.name, start_model.name, end_model.name, direction
    )))
}

fn traversal_step_matches_models(
    start_model: &RuntimeNodeModel,
    end_model: &RuntimeNodeModel,
    rel_model: &RuntimeRelModel,
    direction: Direction,
) -> bool {
    match direction {
        Direction::Out => {
            rel_model.from_model == start_model.name && rel_model.to_model == end_model.name
        }
        Direction::In => {
            rel_model.to_model == start_model.name && rel_model.from_model == end_model.name
        }
        Direction::Both => {
            (rel_model.from_model == start_model.name && rel_model.to_model == end_model.name)
                || (rel_model.to_model == start_model.name
                    && rel_model.from_model == end_model.name)
        }
    }
}

fn resolve_any_traversal_model(
    catalog: &SessionModelCatalog,
    start_model: &RuntimeNodeModel,
    end_model: &RuntimeNodeModel,
    direction: Direction,
) -> Result<Option<RuntimeRelModel>> {
    let matches = catalog
        .list_rel_models()
        .into_iter()
        .filter(|model| traversal_step_matches_models(start_model, end_model, model, direction))
        .cloned()
        .collect::<Vec<_>>();

    match matches.len() {
        0 => Err(crate::GrmError::Constraint(format!(
            "no link connects {} to {} in the requested direction",
            start_model.name, end_model.name
        ))),
        1 => Ok(matches.into_iter().next()),
        _ => Err(crate::GrmError::Constraint(format!(
            "multiple links connect {} to {}; use an explicit link name instead of '*'",
            start_model.name, end_model.name
        ))),
    }
}

fn stored_node_from_kernel(node: &crate::dsl::NodeValue) -> StoredNode {
    StoredNode {
        id: node.id,
        labels: node.labels.clone(),
        props: node.props.clone(),
    }
}

fn stored_rel_from_kernel(rel: &crate::dsl::RelValue) -> StoredRel {
    StoredRel {
        id: rel.id,
        rel_type: rel.ty.clone(),
        from: rel.from,
        to: rel.to,
        props: rel.props.clone(),
    }
}

fn numeric_cmp<F>(a: &Value, b: &Value, cmp: F) -> bool
where
    F: Fn(f64, f64) -> bool,
{
    match (a.as_f64(), b.as_f64()) {
        (Some(la), Some(rb)) => cmp(la, rb),
        _ => false,
    }
}

fn validate_node_order_fields(model: &RuntimeNodeModel, orders: &[SessionOrder]) -> Result<()> {
    for order in orders {
        if order.field == "id" || order.field == model.id_field_name {
            continue;
        }
        if model.field(&order.field).is_none() {
            return Err(crate::GrmError::Constraint(format!(
                "unknown order field '{}' for model '{}'",
                order.field, model.name
            )));
        }
    }
    Ok(())
}

fn validate_rel_order_fields(model: &RuntimeRelModel, orders: &[SessionOrder]) -> Result<()> {
    for order in orders {
        if order.field == "id"
            || order.field == model.id_field_name
            || order.field == "from"
            || order.field == "to"
        {
            continue;
        }
        if model.field(&order.field).is_none() {
            return Err(crate::GrmError::Constraint(format!(
                "unknown order field '{}' for link '{}'",
                order.field, model.name
            )));
        }
    }
    Ok(())
}

fn compare_node_order_values(
    left: &StoredNode,
    right: &StoredNode,
    model: &RuntimeNodeModel,
    orders: &[SessionOrder],
) -> std::cmp::Ordering {
    for order in orders {
        let ordering = if order.field == "id" || order.field == model.id_field_name {
            left.id.cmp(&right.id)
        } else {
            compare_optional_values(left.props.get(&order.field), right.props.get(&order.field))
        };

        let ordering = match order.direction {
            SortDirection::Asc => ordering,
            SortDirection::Desc => ordering.reverse(),
        };

        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }

    std::cmp::Ordering::Equal
}

fn compare_rel_order_values(
    left: &StoredRel,
    right: &StoredRel,
    model: &RuntimeRelModel,
    orders: &[SessionOrder],
) -> std::cmp::Ordering {
    for order in orders {
        let ordering = match order.field.as_str() {
            "id" => left.id.cmp(&right.id),
            "from" => left.from.cmp(&right.from),
            "to" => left.to.cmp(&right.to),
            field if field == model.id_field_name => left.id.cmp(&right.id),
            _ => {
                compare_optional_values(left.props.get(&order.field), right.props.get(&order.field))
            }
        };

        let ordering = match order.direction {
            SortDirection::Asc => ordering,
            SortDirection::Desc => ordering.reverse(),
        };

        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }

    std::cmp::Ordering::Equal
}

fn compare_optional_values(left: Option<&Value>, right: Option<&Value>) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => compare_orderable_values(left, right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn compare_orderable_values(left: &Value, right: &Value) -> std::cmp::Ordering {
    if let (Some(lhs), Some(rhs)) = (left.as_f64(), right.as_f64()) {
        return lhs.partial_cmp(&rhs).unwrap_or(std::cmp::Ordering::Equal);
    }
    if let (Some(lhs), Some(rhs)) = (left.as_str(), right.as_str()) {
        return lhs.cmp(rhs);
    }
    if let (Some(lhs), Some(rhs)) = (left.as_bool(), right.as_bool()) {
        return lhs.cmp(&rhs);
    }
    format_value(left, &SessionColors::plain()).cmp(&format_value(right, &SessionColors::plain()))
}

fn apply_offset_limit<T>(items: Vec<T>, offset: Option<usize>, limit: Option<usize>) -> Vec<T> {
    let start = offset.unwrap_or(0);
    if start >= items.len() {
        return Vec::new();
    }

    let end = if let Some(limit) = limit {
        start.saturating_add(limit).min(items.len())
    } else {
        items.len()
    };

    items.into_iter().skip(start).take(end - start).collect()
}

fn collect_assignments(assignments: &[KeyValueArg]) -> BTreeMap<String, String> {
    assignments
        .iter()
        .map(|arg| (arg.key.clone(), arg.value.clone()))
        .collect()
}

fn collect_query_terms(terms: &[QueryTerm]) -> BTreeMap<String, String> {
    terms
        .iter()
        .map(|term| (term.key.clone(), term.value.clone()))
        .collect()
}

impl SessionFileFormat {
    fn from_flag(flag: &str) -> Option<Self> {
        match flag {
            "--json" => Some(Self::Json),
            "--bin" => Some(Self::Binary),
            _ => None,
        }
    }

    fn flag(self) -> &'static str {
        match self {
            Self::Json => "--json",
            Self::Binary => "--bin",
        }
    }
}
