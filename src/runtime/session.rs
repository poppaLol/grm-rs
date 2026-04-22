use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};

use crate::{
    BackendIdentity, GraphClient, GraphTx, InMemoryBackend, Result, RuntimeField,
    RuntimeNodeModel, RuntimeValueType, SessionModelCatalog, StoredNode,
};
use crate::runtime::{parse_required_flag, validate_field_name, validate_model_name};

pub struct SessionState {
    client: GraphClient<InMemoryBackend>,
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

    pub fn register_model(&mut self, model: RuntimeNodeModel) -> Result<()> {
        self.catalog.register(model)
    }

    pub fn model_list(&self) -> Vec<&RuntimeNodeModel> {
        self.catalog.list()
    }

    pub fn model(&self, name: &str) -> Option<&RuntimeNodeModel> {
        self.catalog.get(name)
    }

    pub fn node_id_type(&self) -> crate::BackendIdType {
        self.client.backend().node_id_type()
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
}

pub struct CliSession<R: BufRead, W: Write> {
    state: SessionState,
    reader: R,
    writer: W,
}

impl<R: BufRead, W: Write> CliSession<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            state: SessionState::new(),
            reader,
            writer,
        }
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn into_parts(self) -> (SessionState, R, W) {
        (self.state, self.reader, self.writer)
    }

    pub async fn run(&mut self) -> Result<()> {
        writeln!(
            self.writer,
            "Fresh in-memory graph session started. Type 'help' for commands."
        )?;

        loop {
            self.write_prompt()?;
            let Some(line) = self.read_line()? else {
                writeln!(self.writer)?;
                break;
            };

            let should_exit = self.handle_command(&line).await?;
            if should_exit {
                break;
            }
        }

        Ok(())
    }

    pub async fn run_script(&mut self) -> Result<()> {
        loop {
            let Some(line) = self.read_line()? else {
                break;
            };

            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let should_exit = self.handle_command(trimmed).await?;
            if should_exit {
                break;
            }
        }

        Ok(())
    }

    pub async fn handle_command(&mut self, line: &str) -> Result<bool> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(false);
        }

        match trimmed {
            "help" => self.write_help()?,
            "exit" | "quit" => return Ok(true),
            "model create" => self.run_model_create_wizard().await?,
            "model list" => self.write_model_list()?,
            _ if trimmed.starts_with("model define ") => {
                let spec = &trimmed["model define ".len()..];
                self.handle_model_define(spec)?;
            }
            _ if trimmed.starts_with("model show ") => {
                let name = trimmed["model show ".len()..].trim();
                self.write_model_show(name)?;
            }
            _ => {
                writeln!(self.writer, "Unknown command: {trimmed}")?;
            }
        }

        Ok(false)
    }

    fn write_prompt(&mut self) -> Result<()> {
        write!(self.writer, "grm(session)> ")?;
        self.writer.flush()?;
        Ok(())
    }

    fn write_help(&mut self) -> Result<()> {
        writeln!(self.writer, "Available commands:")?;
        writeln!(self.writer, "  model create")?;
        writeln!(
            self.writer,
            "  model define <Name> <id_field> [field:type:required|optional ...]"
        )?;
        writeln!(self.writer, "  model list")?;
        writeln!(self.writer, "  model show <name>")?;
        writeln!(self.writer, "  help")?;
        writeln!(self.writer, "  exit")?;
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

    fn handle_model_define(&mut self, spec: &str) -> Result<()> {
        let parts: Vec<&str> = spec.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(crate::GrmError::Constraint(
                "usage: model define <Name> <id_field> [field:type:required|optional ...]".into(),
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

    async fn run_model_create_wizard(&mut self) -> Result<()> {
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
