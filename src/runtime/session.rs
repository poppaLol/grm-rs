use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    BackendIdentity, GraphClient, GraphTx, InMemoryBackend, Result, RuntimeField, RuntimeNodeModel,
    RuntimeRelModel, RuntimeValueType, SessionModelCatalog, StoredNode, StoredRel,
};
use crate::runtime::{parse_required_flag, validate_field_name, validate_model_name};
use crate::backend::{BinaryPersistedGraphStore, GraphStore, PersistedGraphStore};

pub struct SessionState {
    client: GraphClient<InMemoryBackend>,
    catalog: SessionModelCatalog,
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

    fn apply_persisted_session(&mut self, persisted: PersistedSession) {
        self.client
            .backend()
            .replace_store(GraphStore::from_persisted(persisted.graph));
        self.catalog = persisted.catalog;
    }

    pub fn save_to_json(&self, path: impl AsRef<Path>) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.persisted_session())
            .map_err(|_| crate::error::GrmError::SaveAborted("failed to serialize session as JSON"))?;
        fs::write(path, json)
            .map_err(|_| crate::error::GrmError::SaveAborted("failed to write JSON session file"))?;
        Ok(())
    }

    pub fn save_to_binary(&self, path: impl AsRef<Path>) -> Result<()> {
        let persisted = BinaryPersistedSession {
            graph: self.client.backend().snapshot_store().to_binary_persisted()?,
            catalog: self.catalog.clone(),
        };
        let bytes = bincode::serialize(&persisted)
            .map_err(|_| crate::error::GrmError::SaveAborted("failed to serialize session as binary"))?;
        fs::write(path, bytes)
            .map_err(|_| crate::error::GrmError::SaveAborted("failed to write binary session file"))?;
        Ok(())
    }

    pub fn load_from_json(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let json = fs::read_to_string(path)
            .map_err(|_| crate::error::GrmError::LoadAborted("failed to read JSON session file"))?;
        let persisted: PersistedSession = serde_json::from_str(&json)
            .map_err(|_| crate::error::GrmError::LoadAborted("failed to deserialize JSON session file"))?;
        self.apply_persisted_session(persisted);
        Ok(())
    }

    pub fn load_from_binary(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let bytes = fs::read(path)
            .map_err(|_| crate::error::GrmError::LoadAborted("failed to read binary session file"))?;
        let persisted: BinaryPersistedSession = bincode::deserialize(&bytes)
            .map_err(|_| crate::error::GrmError::LoadAborted("failed to deserialize binary session file"))?;
        self.client
            .backend()
            .replace_store(GraphStore::from_binary_persisted(persisted.graph)?);
        self.catalog = persisted.catalog;
        Ok(())
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
            .ok_or_else(|| crate::GrmError::Constraint(format!("from node '{}' was not found", from_raw)))?;
        if !from_node.labels.iter().any(|label| label == &model.from_model) {
            return Err(crate::GrmError::Constraint(format!(
                "from node '{}' does not match model '{}'",
                from_raw, model.from_model
            )));
        }

        let to_node = tx
            .tx_mut()?
            .find_node_by_id(to_raw)
            .await?
            .ok_or_else(|| crate::GrmError::Constraint(format!("to node '{}' was not found", to_raw)))?;
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
        let existing = tx
            .tx_mut()?
            .find_node_by_id(raw_id)
            .await?
            .ok_or_else(|| crate::GrmError::Constraint(format!("node '{}' was not found", raw_id)))?;
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
            .ok_or_else(|| crate::GrmError::Constraint(format!("node '{}' was not found", raw_id)))?;
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
        let existing = tx
            .tx_mut()?
            .find_node_by_id(raw_id)
            .await?
            .ok_or_else(|| crate::GrmError::Constraint(format!("node '{}' was not found", raw_id)))?;
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
            .find_relationships(model_name, &BTreeMap::from([(String::from("id"), raw_id.to_string())]))?
            .into_iter()
            .next()
            .ok_or_else(|| crate::GrmError::Constraint(format!("edge '{}' was not found", raw_id)))?;

        let mut tx = self.client.transaction().await?;
        let updated = tx
            .tx_mut()?
            .update_relationship(existing.id, props)
            .await?
            .ok_or_else(|| crate::GrmError::Constraint(format!("edge '{}' was not found", raw_id)))?;
        tx.commit().await?;
        Ok(updated)
    }

    pub async fn delete_relationship_instance(&self, model_name: &str, id: &str) -> Result<()> {
        let raw_id = self.parse_backend_id(id, self.rel_id_type(), "edge id")?;
        let existing = self
            .find_relationships(model_name, &BTreeMap::from([(String::from("id"), raw_id.to_string())]))?
            .into_iter()
            .next()
            .ok_or_else(|| crate::GrmError::Constraint(format!("edge '{}' was not found", raw_id)))?;

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
        let model = self
            .catalog
            .get_node_model(model_name)
            .ok_or(crate::GrmError::NotFound)?;
        let id_filter =
            self.extract_id_filter(filters, &model.id_field_name, self.node_id_type())?;
        let prop_filters = self.parse_model_filters(filters, model)?;

        let mut nodes = self.client.backend().snapshot_nodes();
        nodes.retain(|node| node.labels.iter().any(|label| label == &model.label));

        if let Some(id) = id_filter {
            nodes.retain(|node| node.id == id);
        }

        nodes.retain(|node| matches_props(&node.props, &prop_filters));
        Ok(nodes)
    }

    pub fn find_relationships(
        &self,
        model_name: &str,
        filters: &BTreeMap<String, String>,
    ) -> Result<Vec<StoredRel>> {
        let model = self
            .catalog
            .get_rel_model(model_name)
            .ok_or(crate::GrmError::NotFound)?;
        let id_filter =
            self.extract_id_filter(filters, &model.id_field_name, self.rel_id_type())?;
        let from_filter = filters
            .get("from")
            .map(|raw| self.parse_backend_id(raw, self.node_id_type(), "from"))
            .transpose()?;
        let to_filter = filters
            .get("to")
            .map(|raw| self.parse_backend_id(raw, self.node_id_type(), "to"))
            .transpose()?;
        let prop_filters = self.parse_rel_filters(filters, model)?;

        let mut rels = self.client.backend().snapshot_relationships();
        rels.retain(|rel| rel.rel_type == model.rel_type);

        if let Some(id) = id_filter {
            rels.retain(|rel| rel.id == id);
        }
        if let Some(from) = from_filter {
            rels.retain(|rel| rel.from == from);
        }
        if let Some(to) = to_filter {
            rels.retain(|rel| rel.to == to);
        }

        rels.retain(|rel| matches_props(&rel.props, &prop_filters));
        Ok(rels)
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

    fn extract_id_filter(
        &self,
        filters: &BTreeMap<String, String>,
        id_field_name: &str,
        id_type: crate::BackendIdType,
    ) -> Result<Option<i64>> {
        match (filters.get("id"), filters.get(id_field_name)) {
            (Some(_), Some(_)) => Err(crate::GrmError::Constraint(format!(
                "use either 'id' or '{}' when filtering by backend id",
                id_field_name
            ))),
            (Some(raw), None) => self.parse_backend_id(raw, id_type, "id").map(Some),
            (None, Some(raw)) => self.parse_backend_id(raw, id_type, id_field_name).map(Some),
            (None, None) => Ok(None),
        }
    }

    fn parse_backend_id(
        &self,
        raw: &str,
        id_type: crate::BackendIdType,
        subject: &str,
    ) -> Result<i64> {
        match id_type {
            crate::BackendIdType::Int64 => raw.trim().parse::<i64>().map_err(|_| {
                crate::GrmError::Constraint(format!("{subject} must be an int id"))
            }),
            crate::BackendIdType::Uuid => Err(crate::GrmError::NotSupported(
                "uuid runtime session ids are not supported by this backend yet",
            )),
        }
    }
}

pub struct CliSession<R: BufRead, W: Write> {
    state: SessionState,
    reader: R,
    writer: W,
    prompt_name: &'static str,
}

impl<R: BufRead, W: Write> CliSession<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            state: SessionState::new(),
            reader,
            writer,
            prompt_name: "session",
        }
    }

    pub fn with_state(state: SessionState, reader: R, writer: W) -> Self {
        Self {
            state,
            reader,
            writer,
            prompt_name: "session",
        }
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn into_parts(self) -> (SessionState, R, W) {
        (self.state, self.reader, self.writer)
    }

    pub async fn run(&mut self) -> Result<()> {
        self.run_interactive_loop("Fresh in-memory graph session started. Type 'session.help' for commands.")
            .await
    }

    pub async fn continue_interactive(&mut self) -> Result<()> {
        self.run_interactive_loop("Script loaded. Entering interactive session. Type 'session.help' for commands.")
            .await
    }

    pub async fn run_script(&mut self) -> Result<()> {
        self.prompt_name = "script";
        loop {
            let Some(line) = self.read_line()? else {
                break;
            };

            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            writeln!(self.writer, "grm(script)> {trimmed}")?;
            let should_exit = self.handle_command(trimmed).await?;
            if should_exit {
                break;
            }
        }

        self.prompt_name = "session";

        Ok(())
    }

    async fn run_interactive_loop(&mut self, banner: &str) -> Result<()> {
        writeln!(self.writer, "{banner}")?;

        loop {
            self.write_prompt()?;
            let Some(line) = self.read_line()? else {
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

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let command = parts[0];
        let args = &parts[1..];

        match command {
            "?" | "help" => self.write_help()?,
            "session.help" => self.write_help()?,
            "exit" | "session.exit" => return Ok(true),
            "model.define" => {
                if args.is_empty() {
                    self.run_model_define_wizard().await?;
                } else {
                    self.handle_model_define_args(args)?;
                }
            }
            "model.list" => self.write_model_list()?,
            "model.show" => self.write_model_show(expect_single_arg(command, args)?)?,
            "link.define" => {
                if args.is_empty() {
                    self.run_link_define_wizard().await?;
                } else {
                    self.handle_link_define_args(args)?;
                }
            }
            "link.list" => self.write_rel_model_list()?,
            "link.show" => self.write_rel_model_show(expect_single_arg(command, args)?)?,
            "node.create" => self.handle_node_create(args).await?,
            "node.find" => self.handle_node_find(args)?,
            "node.update" | "node.edit" => self.handle_node_update(args).await?,
            "node.delete" => self.handle_node_delete(args).await?,
            "edge.create" => self.handle_edge_create(args).await?,
            "edge.find" => self.handle_edge_find(args)?,
            "edge.update" | "edge.edit" => self.handle_edge_update(args).await?,
            "edge.delete" => self.handle_edge_delete(args).await?,
            "session.save" => self.handle_session_save(args)?,
            "session.load" => self.handle_session_load(args)?,
            _ => writeln!(self.writer, "Unknown command: {trimmed}")?,
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
        writeln!(self.writer, "  model.define [<Name> <id_field> [field:type:required|optional ...]]")?;
        writeln!(self.writer, "  model.list")?;
        writeln!(self.writer, "  model.show <name>")?;
        writeln!(self.writer, "  link.define [<Name> <from_model> <to_model> <id_field> [field:type:required|optional ...]]")?;
        writeln!(self.writer, "  link.list")?;
        writeln!(self.writer, "  link.show <name>")?;
        writeln!(self.writer, "  node.create <ModelName> [field=value ...]")?;
        writeln!(self.writer, "  node.find <ModelName> [field=value ...]")?;
        writeln!(self.writer, "  node.update <ModelName> <id> [field=value ...]")?;
        writeln!(self.writer, "  node.delete <ModelName> <id>")?;
        writeln!(self.writer, "  edge.create <LinkName> from=<id> to=<id> [field=value ...]")?;
        writeln!(self.writer, "  edge.find <LinkName> [from=<id>] [to=<id>] [field=value ...]")?;
        writeln!(self.writer, "  edge.update <LinkName> <id> [field=value ...]")?;
        writeln!(self.writer, "  edge.delete <LinkName> <id>")?;
        writeln!(self.writer, "  session.save --json <path>")?;
        writeln!(self.writer, "  session.save --bin <path>")?;
        writeln!(self.writer, "  session.load --json <path>")?;
        writeln!(self.writer, "  session.load --bin <path>")?;
        writeln!(self.writer, "  session.help")?;
        writeln!(self.writer, "  session.exit")?;
        Ok(())
    }

    fn write_model_list(&mut self) -> Result<()> {
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
            let req = if field.required { "required" } else { "optional" };
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
            let req = if field.required { "required" } else { "optional" };
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

        let model =
            RuntimeNodeModel::new(name, id_field_name, self.state.node_id_type(), fields)?;
        self.state.register_model(model.clone())?;
        writeln!(self.writer, "Model '{}' created from script.", model.name)?;
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
        writeln!(self.writer, "Link '{}' created from script.", model.name)?;
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
        writeln!(self.writer, "Link '{}' created.", model.name)?;

        if self.prompt_yes_no("Create the first link now? [y/n]: ")? {
            self.run_create_relationship_wizard(&model.name).await?;
        }

        Ok(())
    }

    async fn handle_node_create(&mut self, args: &[&str]) -> Result<()> {
        if args.is_empty() {
            return Err(crate::GrmError::Constraint(
                "usage: node.create <ModelName> [field=value ...]".into(),
            ));
        }

        let model_name = args[0];
        let values = parse_key_value_args(&args[1..])?;
        let created = self.state.create_instance(model_name, &values).await?;
        let model = self.state.model(model_name).ok_or(crate::GrmError::NotFound)?;
        writeln!(
            self.writer,
            "Created node {} with label '{}'. {}={}.",
            created.id, model.label, model.id_field_name, created.id
        )?;
        Ok(())
    }

    async fn handle_node_update(&mut self, args: &[&str]) -> Result<()> {
        if args.len() < 2 {
            return Err(crate::GrmError::Constraint(
                "usage: node.update <ModelName> <id> [field=value ...]".into(),
            ));
        }
        let updated = self
            .state
            .update_node_instance(args[0], args[1], &parse_key_value_args(&args[2..])?)
            .await?;
        let model = self.state.model(args[0]).ok_or(crate::GrmError::NotFound)?;
        writeln!(
            self.writer,
            "Updated node {} {}={} {}",
            model.name,
            model.id_field_name,
            updated.id,
            format_props(&updated.props)
        )?;
        Ok(())
    }

    async fn handle_node_delete(&mut self, args: &[&str]) -> Result<()> {
        if args.len() != 2 {
            return Err(crate::GrmError::Constraint(
                "usage: node.delete <ModelName> <id>".into(),
            ));
        }
        self.state.delete_node_instance(args[0], args[1]).await?;
        writeln!(self.writer, "Deleted node {} {}.", args[0], args[1])?;
        Ok(())
    }

    fn handle_node_find(&mut self, args: &[&str]) -> Result<()> {
        if args.is_empty() {
            return Err(crate::GrmError::Constraint(
                "usage: node.find <ModelName> [field=value ...]".into(),
            ));
        }

        let model_name = args[0];
        let filters = parse_key_value_args(&args[1..])?;
        let model = self
            .state
            .model(model_name)
            .ok_or(crate::GrmError::NotFound)?
            .clone();
        let nodes = self.state.find_nodes(model_name, &filters)?;
        if nodes.is_empty() {
            writeln!(self.writer, "No nodes matched model '{}'.", model_name)?;
            return Ok(());
        }

        for node in nodes {
            writeln!(
                self.writer,
                "Node {} {}={} {}",
                model.name,
                model.id_field_name,
                node.id,
                format_props(&node.props)
            )?;
        }
        Ok(())
    }

    async fn handle_edge_create(&mut self, args: &[&str]) -> Result<()> {
        if args.is_empty() {
            return Err(crate::GrmError::Constraint(
                "usage: edge.create <LinkName> from=<id> to=<id> [field=value ...]".into(),
            ));
        }

        let model_name = args[0];
        let mut values = parse_key_value_args(&args[1..])?;
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
        let model = self
            .state
            .rel_model(model_name)
            .ok_or(crate::GrmError::NotFound)?;
        writeln!(
            self.writer,
            "Created edge {} of type '{}'. {}={}.",
            created.id, model.rel_type, model.id_field_name, created.id
        )?;
        Ok(())
    }

    async fn handle_edge_update(&mut self, args: &[&str]) -> Result<()> {
        if args.len() < 2 {
            return Err(crate::GrmError::Constraint(
                "usage: edge.update <LinkName> <id> [field=value ...]".into(),
            ));
        }
        let updated = self
            .state
            .update_relationship_instance(args[0], args[1], &parse_key_value_args(&args[2..])?)
            .await?;
        let model = self.state.rel_model(args[0]).ok_or(crate::GrmError::NotFound)?;
        writeln!(
            self.writer,
            "Updated edge {} {}={} from={} to={} {}",
            model.name,
            model.id_field_name,
            updated.id,
            updated.from,
            updated.to,
            format_props(&updated.props)
        )?;
        Ok(())
    }

    async fn handle_edge_delete(&mut self, args: &[&str]) -> Result<()> {
        if args.len() != 2 {
            return Err(crate::GrmError::Constraint(
                "usage: edge.delete <LinkName> <id>".into(),
            ));
        }
        self.state.delete_relationship_instance(args[0], args[1]).await?;
        writeln!(self.writer, "Deleted edge {} {}.", args[0], args[1])?;
        Ok(())
    }

    fn handle_edge_find(&mut self, args: &[&str]) -> Result<()> {
        if args.is_empty() {
            return Err(crate::GrmError::Constraint(
                "usage: edge.find <LinkName> [from=<id>] [to=<id>] [field=value ...]".into(),
            ));
        }

        let model_name = args[0];
        let filters = parse_key_value_args(&args[1..])?;
        let model = self
            .state
            .rel_model(model_name)
            .ok_or(crate::GrmError::NotFound)?
            .clone();
        let rels = self.state.find_relationships(model_name, &filters)?;
        if rels.is_empty() {
            writeln!(self.writer, "No edges matched link '{}'.", model_name)?;
            return Ok(());
        }

        for rel in rels {
            writeln!(
                self.writer,
                "Edge {} {}={} from={} to={} {}",
                model.name,
                model.id_field_name,
                rel.id,
                rel.from,
                rel.to,
                format_props(&rel.props)
            )?;
        }
        Ok(())
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
                ))
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
                self.state.load_from_json(args[1])?;
                writeln!(self.writer, "Loaded session from JSON file '{}'.", args[1])?;
            }
            "--bin" => {
                self.state.load_from_binary(args[1])?;
                writeln!(self.writer, "Loaded session from binary file '{}'.", args[1])?;
            }
            _ => {
                return Err(crate::GrmError::Constraint(
                    "usage: session.load --json <path> | session.load --bin <path>".into(),
                ))
            }
        }
        Ok(())
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

            if fields.iter().any(|field: &RuntimeField| field.name == field_name) {
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
                let req = if field.required { "required" } else { "optional" };
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
                let req = if field.required { "required" } else { "optional" };
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
            created.id,
            model.label,
            model.id_field_name,
            created.id
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
            created.id,
            model.rel_type,
            model.id_field_name,
            created.id
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

    fn read_line(&mut self) -> Result<Option<String>> {
        let mut line = String::new();
        let read = self.reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        Ok(Some(line))
    }
}

impl From<io::Error> for crate::GrmError {
    fn from(err: io::Error) -> Self {
        crate::GrmError::Backend(err.to_string())
    }
}

fn expect_single_arg<'a>(command: &str, args: &'a [&str]) -> Result<&'a str> {
    if args.len() != 1 {
        return Err(crate::GrmError::Constraint(format!(
            "usage: {command} <name>"
        )));
    }
    Ok(args[0])
}

fn parse_key_value_args(args: &[&str]) -> Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    for arg in args {
        let Some((key, value)) = arg.split_once('=') else {
            return Err(crate::GrmError::Constraint(format!(
                "expected key=value argument, got '{}'",
                arg
            )));
        };
        values.insert(key.to_string(), value.to_string());
    }
    Ok(values)
}

fn matches_props(props: &BTreeMap<String, Value>, filters: &BTreeMap<String, Value>) -> bool {
    filters
        .iter()
        .all(|(key, value)| props.get(key).map(|stored| stored == value).unwrap_or(false))
}

fn format_props(props: &BTreeMap<String, Value>) -> String {
    if props.is_empty() {
        return "{}".into();
    }

    let mut parts = Vec::new();
    for (key, value) in props {
        parts.push(format!("{key}={}", format_value(value)));
    }
    format!("{{{}}}", parts.join(" "))
}

fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}
