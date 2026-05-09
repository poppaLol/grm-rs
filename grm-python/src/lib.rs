#![expect(
    clippy::useless_conversion,
    reason = "PyO3's pymethod wrappers currently expand PyResult returns through redundant conversions"
)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use grm_rs::backend::{BackendIdType, BackendIdentity, GraphBackend, GraphTx};
use grm_rs::{
    GraphClient, Neo4jBackend, Neo4jConfig, RuntimeField, RuntimeNodeModel, RuntimeRelModel,
    RuntimeValueType, SessionModelCatalog, SessionState, StoredNode, StoredRel,
};
use pyo3::create_exception;
use pyo3::exceptions::{PyRuntimeError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList, PyModule};
use serde_json::Value;

create_exception!(_grm_rs, PyGrmError, pyo3::exceptions::PyException);

#[pyclass(name = "Session")]
struct PySession {
    state: SessionState,
    autocommit: Option<PyAutocommitTarget>,
}

#[pyclass(name = "Neo4jSession")]
struct PyNeo4jSession {
    client: GraphClient<Neo4jBackend>,
    catalog: SessionModelCatalog,
}

#[derive(Clone)]
struct PyAutocommitTarget {
    format: PySessionFileFormat,
    path: PathBuf,
}

#[derive(Clone, Copy)]
enum PySessionFileFormat {
    Json,
    Binary,
}

#[pymethods]
impl PySession {
    #[new]
    #[pyo3(signature = (*, autocommit=false, autocommit_path=None, autocommit_format="json"))]
    fn new(
        autocommit: bool,
        autocommit_path: Option<String>,
        autocommit_format: &str,
    ) -> PyResult<Self> {
        let autocommit = configure_autocommit(autocommit, autocommit_path, autocommit_format)?;
        Ok(Self {
            state: SessionState::new(),
            autocommit,
        })
    }

    #[getter]
    fn autocommit(&self) -> bool {
        self.autocommit.is_some()
    }

    #[getter]
    fn autocommit_path(&self) -> Option<String> {
        self.autocommit
            .as_ref()
            .map(|target| target.path.display().to_string())
    }

    #[getter]
    fn autocommit_format(&self) -> Option<&'static str> {
        self.autocommit
            .as_ref()
            .map(|target| target.format.keyword())
    }

    fn node_id_type(&self) -> &'static str {
        backend_id_type_name(self.state.node_id_type())
    }

    fn rel_id_type(&self) -> &'static str {
        backend_id_type_name(self.state.rel_id_type())
    }

    fn model_create(
        &mut self,
        name: &str,
        id_field: &str,
        fields: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let model = RuntimeNodeModel::new(
            name,
            id_field,
            self.state.node_id_type(),
            parse_fields(fields)?,
        )
        .map_err(grm_err)?;
        self.state.register_model(model).map_err(grm_err)?;
        self.persist_autocommit().map_err(grm_err)
    }

    fn link_create(
        &mut self,
        name: &str,
        from_model: &str,
        to_model: &str,
        id_field: &str,
        fields: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let model = RuntimeRelModel::new(
            name,
            from_model,
            to_model,
            id_field,
            self.state.rel_id_type(),
            parse_fields(fields)?,
        )
        .map_err(grm_err)?;
        self.state.register_rel_model(model).map_err(grm_err)?;
        self.persist_autocommit().map_err(grm_err)
    }

    fn model_show(&self, py: Python<'_>, name: &str) -> PyResult<Option<PyObject>> {
        self.state
            .model(name)
            .map(|model| runtime_node_model_to_py(py, model))
            .transpose()
    }

    fn model_list(&self, py: Python<'_>) -> PyResult<PyObject> {
        let items = PyList::empty_bound(py);
        for model in self.state.model_list() {
            items.append(runtime_node_model_to_py(py, model)?)?;
        }
        Ok(items.into())
    }

    fn link_show(&self, py: Python<'_>, name: &str) -> PyResult<Option<PyObject>> {
        self.state
            .rel_model(name)
            .map(|model| runtime_rel_model_to_py(py, model))
            .transpose()
    }

    fn link_list(&self, py: Python<'_>) -> PyResult<PyObject> {
        let items = PyList::empty_bound(py);
        for model in self.state.rel_model_list() {
            items.append(runtime_rel_model_to_py(py, model)?)?;
        }
        Ok(items.into())
    }

    #[pyo3(signature = (model_name, values=None))]
    fn node_create(
        &mut self,
        py: Python<'_>,
        model_name: &str,
        values: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let raw_values = extract_string_map(values)?;
        let node = block_on(py, self.state.create_instance(model_name, &raw_values))?;
        self.persist_autocommit().map_err(grm_err)?;
        stored_node_to_py(py, &node)
    }

    #[pyo3(signature = (model_name, node_id, values=None))]
    fn node_update(
        &mut self,
        py: Python<'_>,
        model_name: &str,
        node_id: &Bound<'_, PyAny>,
        values: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let id = python_value_to_string(node_id)?;
        let raw_values = extract_string_map(values)?;
        let node = block_on(
            py,
            self.state
                .update_node_instance(model_name, &id, &raw_values),
        )?;
        self.persist_autocommit().map_err(grm_err)?;
        stored_node_to_py(py, &node)
    }

    fn node_delete(
        &mut self,
        py: Python<'_>,
        model_name: &str,
        node_id: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let id = python_value_to_string(node_id)?;
        block_on(py, self.state.delete_node_instance(model_name, &id))?;
        self.persist_autocommit().map_err(grm_err)
    }

    #[pyo3(signature = (model_name, filters=None))]
    fn node_find(
        &self,
        py: Python<'_>,
        model_name: &str,
        filters: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let raw_filters = extract_string_map(filters)?;
        let nodes = self
            .state
            .find_nodes(model_name, &raw_filters)
            .map_err(grm_err)?;
        let items = PyList::empty_bound(py);
        for node in nodes {
            items.append(stored_node_to_py(py, &node)?)?;
        }
        Ok(items.into())
    }

    #[pyo3(signature = (model_name, from_id, to_id, values=None))]
    fn edge_create(
        &mut self,
        py: Python<'_>,
        model_name: &str,
        from_id: &Bound<'_, PyAny>,
        to_id: &Bound<'_, PyAny>,
        values: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let from_id = python_value_to_string(from_id)?;
        let to_id = python_value_to_string(to_id)?;
        let raw_values = extract_string_map(values)?;
        let rel = block_on(
            py,
            self.state
                .create_relationship_instance(model_name, &from_id, &to_id, &raw_values),
        )?;
        self.persist_autocommit().map_err(grm_err)?;
        stored_rel_to_py(py, &rel)
    }

    #[pyo3(signature = (model_name, edge_id, values=None))]
    fn edge_update(
        &mut self,
        py: Python<'_>,
        model_name: &str,
        edge_id: &Bound<'_, PyAny>,
        values: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let id = python_value_to_string(edge_id)?;
        let raw_values = extract_string_map(values)?;
        let rel = block_on(
            py,
            self.state
                .update_relationship_instance(model_name, &id, &raw_values),
        )?;
        self.persist_autocommit().map_err(grm_err)?;
        stored_rel_to_py(py, &rel)
    }

    fn edge_delete(
        &mut self,
        py: Python<'_>,
        model_name: &str,
        edge_id: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let id = python_value_to_string(edge_id)?;
        block_on(py, self.state.delete_relationship_instance(model_name, &id))?;
        self.persist_autocommit().map_err(grm_err)
    }

    #[pyo3(signature = (model_name, filters=None))]
    fn edge_find(
        &self,
        py: Python<'_>,
        model_name: &str,
        filters: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let raw_filters = extract_string_map(filters)?;
        let rels = self
            .state
            .find_relationships(model_name, &raw_filters)
            .map_err(grm_err)?;
        let items = PyList::empty_bound(py);
        for rel in rels {
            items.append(stored_rel_to_py(py, &rel)?)?;
        }
        Ok(items.into())
    }

    fn save_json(&self, path: &str) -> PyResult<()> {
        self.state.save_to_json(path).map_err(grm_err)
    }

    fn save_binary(&self, path: &str) -> PyResult<()> {
        self.state.save_to_binary(path).map_err(grm_err)
    }

    fn load_json(&mut self, path: &str) -> PyResult<()> {
        self.state.load_from_json(path).map_err(grm_err)?;
        self.persist_autocommit().map_err(grm_err)
    }

    fn load_binary(&mut self, path: &str) -> PyResult<()> {
        self.state.load_from_binary(path).map_err(grm_err)?;
        self.persist_autocommit().map_err(grm_err)
    }
}

#[pymethods]
impl PyNeo4jSession {
    #[new]
    #[pyo3(signature = (*, uri, user, password))]
    fn new(py: Python<'_>, uri: &str, user: &str, password: &str) -> PyResult<Self> {
        let backend = block_on(
            py,
            grm_rs::connect_neo4j_backend(Neo4jConfig {
                uri: uri.to_string(),
                user: user.to_string(),
                password: password.to_string(),
            }),
        )?;
        Ok(Self {
            client: GraphClient::new(backend),
            catalog: SessionModelCatalog::new(),
        })
    }

    fn node_id_type(&self) -> &'static str {
        backend_id_type_name(self.client.backend().node_id_type())
    }

    fn rel_id_type(&self) -> &'static str {
        backend_id_type_name(self.client.backend().rel_id_type())
    }

    #[pyo3(signature = (query_text, params=None))]
    fn execute_query(
        &self,
        py: Python<'_>,
        query_text: &str,
        params: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<usize> {
        let params = py_dict_to_json_object(params)?;
        let result = block_on(py, self.client.backend().execute_query(query_text, params))?;
        Ok(result.rows.len())
    }

    fn model_create(
        &mut self,
        name: &str,
        id_field: &str,
        fields: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let model = RuntimeNodeModel::new(
            name,
            id_field,
            self.client.backend().node_id_type(),
            parse_fields(fields)?,
        )
        .map_err(grm_err)?;
        self.catalog.register_node_model(model).map_err(grm_err)
    }

    fn link_create(
        &mut self,
        name: &str,
        from_model: &str,
        to_model: &str,
        id_field: &str,
        fields: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        if self.catalog.get_node_model(from_model).is_none() {
            return Err(grm_err(grm_rs::GrmError::NotFound));
        }
        if self.catalog.get_node_model(to_model).is_none() {
            return Err(grm_err(grm_rs::GrmError::NotFound));
        }
        let model = RuntimeRelModel::new(
            name,
            from_model,
            to_model,
            id_field,
            self.client.backend().rel_id_type(),
            parse_fields(fields)?,
        )
        .map_err(grm_err)?;
        self.catalog.register_rel_model(model).map_err(grm_err)
    }

    #[pyo3(signature = (model_name, values=None))]
    fn node_create(
        &self,
        py: Python<'_>,
        model_name: &str,
        values: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let model = self
            .catalog
            .get_node_model(model_name)
            .ok_or_else(|| grm_err(grm_rs::GrmError::NotFound))?
            .clone();
        let raw_values = extract_string_map(values)?;
        let props = model.validate_instance_input(&raw_values).map_err(grm_err)?;
        let node = block_on(py, async {
            let mut tx = self.client.transaction().await?;
            let node = tx
                .tx_mut()?
                .create_node(vec![model.label.clone()], props)
                .await?;
            tx.commit().await?;
            Ok(node)
        })?;
        stored_node_to_py(py, &node)
    }

    #[pyo3(signature = (model_name, filters=None))]
    fn node_find(
        &self,
        py: Python<'_>,
        model_name: &str,
        filters: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let model = self
            .catalog
            .get_node_model(model_name)
            .ok_or_else(|| grm_err(grm_rs::GrmError::NotFound))?
            .clone();
        let raw_filters = extract_string_map(filters)?;
        if raw_filters.len() != 1 {
            return Err(PyTypeError::new_err(
                "Neo4jSession.node_find currently expects exactly one property filter",
            ));
        }
        let (field_name, raw_value) = raw_filters
            .iter()
            .next()
            .expect("single filter checked above");
        let field = model.field(field_name).ok_or_else(|| {
            grm_err(grm_rs::GrmError::Constraint(format!(
                "unknown field '{}' for model '{}'",
                field_name, model.name
            )))
        })?;
        let value = field.value_type.parse_value(raw_value).map_err(grm_err)?;

        let nodes = block_on(py, async {
            let mut tx = self.client.transaction().await?;
            let nodes = tx.tx_mut()?.find_nodes_by_property(field_name, &value).await?;
            tx.rollback().await?;
            Ok(nodes)
        })?;
        let items = PyList::empty_bound(py);
        for node in nodes
            .into_iter()
            .filter(|node| node.labels.iter().any(|label| label == &model.label))
        {
            items.append(stored_node_to_py(py, &node)?)?;
        }
        Ok(items.into())
    }

    #[pyo3(signature = (model_name, from_id, to_id, values=None))]
    fn edge_create(
        &self,
        py: Python<'_>,
        model_name: &str,
        from_id: &Bound<'_, PyAny>,
        to_id: &Bound<'_, PyAny>,
        values: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let model = self
            .catalog
            .get_rel_model(model_name)
            .ok_or_else(|| grm_err(grm_rs::GrmError::NotFound))?
            .clone();
        let from_id = python_value_to_string(from_id)?
            .parse::<i64>()
            .map_err(|_| PyTypeError::new_err("from_id must be an int for Neo4jSession"))?;
        let to_id = python_value_to_string(to_id)?
            .parse::<i64>()
            .map_err(|_| PyTypeError::new_err("to_id must be an int for Neo4jSession"))?;
        let raw_values = extract_string_map(values)?;
        let props = model.validate_instance_input(&raw_values).map_err(grm_err)?;
        let rel = block_on(py, async {
            let mut tx = self.client.transaction().await?;
            let rel = tx
                .tx_mut()?
                .create_relationship(from_id, to_id, &model.rel_type, props)
                .await?;
            tx.commit().await?;
            Ok(rel)
        })?;
        stored_rel_to_py(py, &rel)
    }
}

impl PySession {
    fn persist_autocommit(&self) -> grm_rs::Result<()> {
        let Some(target) = &self.autocommit else {
            return Ok(());
        };

        match target.format {
            PySessionFileFormat::Json => self.state.save_to_json(&target.path),
            PySessionFileFormat::Binary => self.state.save_to_binary(&target.path),
        }
        .map_err(|err| {
            grm_rs::GrmError::Backend(format!(
                "autocommit failed for '{}': {}",
                target.path.display(),
                err
            ))
        })
    }
}

fn block_on<F, T>(_py: Python<'_>, work: F) -> PyResult<T>
where
    F: std::future::Future<Output = grm_rs::Result<T>>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
    runtime.block_on(work).map_err(grm_err)
}

fn parse_fields(fields: &Bound<'_, PyAny>) -> PyResult<Vec<RuntimeField>> {
    let mut parsed = Vec::new();
    for item in fields.iter()? {
        let item = item?;
        let field = item.downcast::<PyDict>().map_err(|_| {
            PyTypeError::new_err(
                "field definitions must be dicts with 'name', 'type', and 'required' keys",
            )
        })?;
        let name = required_string(field, "name")?;
        let field_type = required_string(field, "type")?;
        let value_type = RuntimeValueType::parse_keyword(&field_type).ok_or_else(|| {
            PyTypeError::new_err(format!(
                "unsupported field type '{field_type}', expected one of: string, int, float, bool"
            ))
        })?;
        let required = required_bool(field, "required")?;
        parsed.push(RuntimeField {
            name,
            value_type,
            required,
        });
    }
    Ok(parsed)
}

fn configure_autocommit(
    autocommit: bool,
    autocommit_path: Option<String>,
    autocommit_format: &str,
) -> PyResult<Option<PyAutocommitTarget>> {
    if !autocommit {
        return Ok(None);
    }

    let path = autocommit_path.ok_or_else(|| {
        PyTypeError::new_err("autocommit_path is required when autocommit is enabled")
    })?;
    let format = PySessionFileFormat::parse(autocommit_format)?;

    Ok(Some(PyAutocommitTarget {
        format,
        path: PathBuf::from(path),
    }))
}

impl PySessionFileFormat {
    fn parse(value: &str) -> PyResult<Self> {
        match value {
            "json" => Ok(Self::Json),
            "binary" | "bin" => Ok(Self::Binary),
            _ => Err(PyTypeError::new_err(
                "autocommit_format must be 'json' or 'binary'",
            )),
        }
    }

    fn keyword(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Binary => "binary",
        }
    }
}

fn required_string(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<String> {
    let value = dict
        .get_item(key)?
        .ok_or_else(|| PyTypeError::new_err(format!("missing required key '{key}'")))?;
    value
        .extract::<String>()
        .map_err(|_| PyTypeError::new_err(format!("field '{key}' must be a string")))
}

fn required_bool(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<bool> {
    let value = dict
        .get_item(key)?
        .ok_or_else(|| PyTypeError::new_err(format!("missing required key '{key}'")))?;
    value
        .extract::<bool>()
        .map_err(|_| PyTypeError::new_err(format!("field '{key}' must be a bool")))
}

fn extract_string_map(input: Option<&Bound<'_, PyDict>>) -> PyResult<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    let Some(input) = input else {
        return Ok(values);
    };

    for (key, value) in input {
        let key = key
            .extract::<String>()
            .map_err(|_| PyTypeError::new_err("mapping keys must be strings"))?;
        values.insert(key, python_value_to_string(&value)?);
    }

    Ok(values)
}

fn py_dict_to_json_object(input: Option<&Bound<'_, PyDict>>) -> PyResult<Value> {
    let mut values = serde_json::Map::new();
    let Some(input) = input else {
        return Ok(Value::Object(values));
    };

    for (key, value) in input {
        let key = key
            .extract::<String>()
            .map_err(|_| PyTypeError::new_err("query parameter keys must be strings"))?;
        values.insert(key, py_any_to_json_value(&value)?);
    }

    Ok(Value::Object(values))
}

fn py_any_to_json_value(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    if value.is_none() {
        return Ok(Value::Null);
    }
    if let Ok(value) = value.extract::<bool>() {
        return Ok(Value::Bool(value));
    }
    if let Ok(value) = value.extract::<i64>() {
        return Ok(Value::from(value));
    }
    if let Ok(value) = value.extract::<f64>() {
        return Ok(Value::from(value));
    }
    if let Ok(value) = value.extract::<String>() {
        return Ok(Value::String(value));
    }
    Err(PyTypeError::new_err(
        "query parameters must be None, bool, int, float, or string",
    ))
}

fn python_value_to_string(value: &Bound<'_, PyAny>) -> PyResult<String> {
    if value.is_none() {
        return Err(PyTypeError::new_err(
            "None is not a supported graph value; omit the field instead",
        ));
    }
    if let Ok(value) = value.extract::<bool>() {
        return Ok(value.to_string());
    }
    if let Ok(value) = value.extract::<i64>() {
        return Ok(value.to_string());
    }
    if let Ok(value) = value.extract::<f64>() {
        return Ok(value.to_string());
    }
    if let Ok(value) = value.extract::<String>() {
        return Ok(value);
    }
    Err(PyTypeError::new_err(
        "graph values must be str, int, float, or bool",
    ))
}

fn runtime_node_model_to_py(py: Python<'_>, model: &RuntimeNodeModel) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("name", &model.name)?;
    dict.set_item("label", &model.label)?;
    dict.set_item("id_field", &model.id_field_name)?;
    dict.set_item("id_type", backend_id_type_name(model.id_type))?;
    let fields = PyList::empty_bound(py);
    for field in &model.fields {
        fields.append(runtime_field_to_py(py, field)?)?;
    }
    dict.set_item("fields", fields)?;
    Ok(dict.into())
}

fn runtime_rel_model_to_py(py: Python<'_>, model: &RuntimeRelModel) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("name", &model.name)?;
    dict.set_item("type", &model.rel_type)?;
    dict.set_item("from_model", &model.from_model)?;
    dict.set_item("to_model", &model.to_model)?;
    dict.set_item("id_field", &model.id_field_name)?;
    dict.set_item("id_type", backend_id_type_name(model.id_type))?;
    let fields = PyList::empty_bound(py);
    for field in &model.fields {
        fields.append(runtime_field_to_py(py, field)?)?;
    }
    dict.set_item("fields", fields)?;
    Ok(dict.into())
}

fn runtime_field_to_py(py: Python<'_>, field: &RuntimeField) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("name", &field.name)?;
    dict.set_item("type", field.value_type.keyword())?;
    dict.set_item("required", field.required)?;
    Ok(dict.into())
}

fn stored_node_to_py(py: Python<'_>, node: &StoredNode) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("id", node.id)?;
    dict.set_item("labels", node.labels.clone())?;
    dict.set_item("props", json_object_to_py(py, &node.props)?)?;
    Ok(dict.into())
}

fn stored_rel_to_py(py: Python<'_>, rel: &StoredRel) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("id", rel.id)?;
    dict.set_item("type", &rel.rel_type)?;
    dict.set_item("from", rel.from)?;
    dict.set_item("to", rel.to)?;
    dict.set_item("props", json_object_to_py(py, &rel.props)?)?;
    Ok(dict.into())
}

fn json_object_to_py(py: Python<'_>, object: &BTreeMap<String, Value>) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    for (key, value) in object {
        dict.set_item(key, json_value_to_py(py, value)?)?;
    }
    Ok(dict.into())
}

fn json_value_to_py(py: Python<'_>, value: &Value) -> PyResult<PyObject> {
    match value {
        Value::Null => Ok(py.None()),
        Value::Bool(value) => Ok(value.into_py(py)),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(value.into_py(py))
            } else if let Some(value) = value.as_u64() {
                Ok(value.into_py(py))
            } else if let Some(value) = value.as_f64() {
                Ok(value.into_py(py))
            } else {
                Err(PyRuntimeError::new_err("unsupported numeric JSON value"))
            }
        }
        Value::String(value) => Ok(value.into_py(py)),
        Value::Array(values) => {
            let items = PyList::empty_bound(py);
            for value in values {
                items.append(json_value_to_py(py, value)?)?;
            }
            Ok(items.into())
        }
        Value::Object(values) => {
            let dict = PyDict::new_bound(py);
            for (key, value) in values {
                dict.set_item(key, json_value_to_py(py, value)?)?;
            }
            Ok(dict.into())
        }
    }
}

fn backend_id_type_name(id_type: BackendIdType) -> &'static str {
    match id_type {
        BackendIdType::Int64 => "int",
        BackendIdType::Uuid => "uuid",
    }
}

fn grm_err(err: grm_rs::GrmError) -> PyErr {
    PyGrmError::new_err(err.to_string())
}

#[pymodule]
fn _grm_rs(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("GrmError", py.get_type_bound::<PyGrmError>())?;
    module.add_class::<PySession>()?;
    module.add_class::<PyNeo4jSession>()?;
    Ok(())
}
