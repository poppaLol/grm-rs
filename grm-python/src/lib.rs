#![expect(
    clippy::useless_conversion,
    reason = "PyO3's pymethod wrappers currently expand PyResult returns through redundant conversions"
)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use grm_rs::backend::{BackendIdType, BackendIdentity, GraphBackend, GraphTx};
use grm_rs::{
    apply_session_batch, DefineEdgeRequest, DefineNodeRequest, DurabilityFormat, DurableOperation,
    EdgeCreateRequest, EdgeDeleteRequest, EdgeFindRequest, EdgeResponse, EdgeUpdateRequest,
    ExplainRequest, FieldSpec, FieldValueType, GraphClient, Neo4jBackend, Neo4jConfig,
    NodeCreateRequest, NodeDeleteRequest, NodeFindRequest, NodeResponse, NodeUpdateRequest,
    OrderDirection, OrderSpec, PredicateOp, ProfileRequest, PropertyPredicate, QueryRequest,
    QueryTerm, RuntimeField, RuntimeNodeModel, RuntimeRelModel, RuntimeRequest, RuntimeResponse,
    RuntimeValueType, SessionBatchParams, SessionFindResult, SessionModelCatalog, SessionState,
    StoredNode, StoredRel, TraversalDirection, TraversalReturn, TraversalStepRequest,
};
use grm_service_api::{GrpcWorkspaceClient, GrpcWorkspaceMode};
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

#[pyclass(name = "ServiceSession")]
struct PyServiceSession {
    runtime: tokio::runtime::Runtime,
    client: GrpcWorkspaceClient,
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

#[derive(Clone, Copy)]
enum PyServiceWorkspaceFormat {
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
        let mut state = SessionState::new();
        if let Some(target) = &autocommit {
            if target.path.exists() {
                state
                    .recover_durable(target.format.durability_format(), &target.path)
                    .map_err(grm_err)?;
            } else {
                state
                    .checkpoint_durable(target.format.durability_format(), &target.path)
                    .map_err(grm_err)?;
            }
        }
        Ok(Self { state, autocommit })
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
        let outcome = self
            .state
            .apply_define_node(DefineNodeRequest {
                name: name.to_string(),
                id_field: id_field.to_string(),
                fields: parse_field_specs(fields)?,
            })
            .map_err(grm_err)?;
        self.append_autocommit(outcome.durable_op).map_err(grm_err)
    }

    fn link_create(
        &mut self,
        name: &str,
        from_model: &str,
        to_model: &str,
        id_field: &str,
        fields: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let outcome = self
            .state
            .apply_define_edge(DefineEdgeRequest {
                name: name.to_string(),
                from_model: from_model.to_string(),
                to_model: to_model.to_string(),
                id_field: id_field.to_string(),
                fields: parse_field_specs(fields)?,
            })
            .map_err(grm_err)?;
        self.append_autocommit(outcome.durable_op).map_err(grm_err)
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
        let outcome = block_on(
            py,
            self.state.apply_node_create(NodeCreateRequest {
                model: model_name.to_string(),
                props: extract_json_map(values)?,
            }),
        )?;
        self.append_autocommit(outcome.durable_op)
            .map_err(grm_err)?;
        stored_node_to_py(py, &outcome.value)
    }

    #[pyo3(signature = (ops, *, atomic=true, response="summary", allow_deletes=false))]
    fn batch(
        &mut self,
        py: Python<'_>,
        ops: &Bound<'_, PyAny>,
        atomic: bool,
        response: &str,
        allow_deletes: bool,
    ) -> PyResult<PyObject> {
        let params_value = Value::Object(serde_json::Map::from_iter([
            ("ops".to_string(), py_any_to_json_value(ops)?),
            ("atomic".to_string(), Value::Bool(atomic)),
            ("response".to_string(), Value::String(response.to_string())),
            ("allow_deletes".to_string(), Value::Bool(allow_deletes)),
        ]));
        let params: SessionBatchParams = serde_json::from_value(params_value)
            .map_err(|err| PyTypeError::new_err(format!("invalid batch parameters: {err}")))?;
        let outcome = block_on(py, apply_session_batch(&mut self.state, params))?;
        if outcome.should_persist {
            self.append_autocommit_many(&outcome.durable_ops)
                .map_err(grm_err)?;
        }
        json_value_to_py(py, &outcome.value)
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
        let raw_id = self
            .state
            .parse_backend_id(&id, self.state.node_id_type(), "node id")
            .map_err(grm_err)?;
        let outcome = block_on(
            py,
            self.state.apply_node_update(NodeUpdateRequest {
                model: model_name.to_string(),
                id: raw_id,
                props: extract_json_map(values)?,
            }),
        )?;
        self.append_autocommit(outcome.durable_op)
            .map_err(grm_err)?;
        stored_node_to_py(py, &outcome.value)
    }

    fn node_delete(
        &mut self,
        py: Python<'_>,
        model_name: &str,
        node_id: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let id = python_value_to_string(node_id)?;
        let raw_id = self
            .state
            .parse_backend_id(&id, self.state.node_id_type(), "node id")
            .map_err(grm_err)?;
        let outcome = block_on(
            py,
            self.state.apply_node_delete(NodeDeleteRequest {
                model: model_name.to_string(),
                id: raw_id,
            }),
        )?;
        self.append_autocommit(outcome.durable_op).map_err(grm_err)
    }

    #[pyo3(signature = (model_name, filters=None, *, via=None, end_filters=None, edge_filters=None, return_=None, order=None, limit=None, offset=None))]
    #[expect(
        clippy::too_many_arguments,
        reason = "PyO3 method signature mirrors the Python API keyword arguments"
    )]
    fn node_find(
        &mut self,
        py: Python<'_>,
        model_name: &str,
        filters: Option<&Bound<'_, PyDict>>,
        via: Option<&Bound<'_, PyAny>>,
        end_filters: Option<&Bound<'_, PyDict>>,
        edge_filters: Option<&Bound<'_, PyDict>>,
        return_: Option<&str>,
        order: Option<&str>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> PyResult<PyObject> {
        let has_structured_query = via.is_some()
            || end_filters.is_some()
            || edge_filters.is_some()
            || return_.is_some()
            || order.is_some()
            || limit.is_some()
            || offset.is_some();

        if !has_structured_query {
            let request = NodeFindRequest::from_adapter_filter_values(
                model_name.to_string(),
                extract_json_map(filters)?,
            )
            .map_err(grm_err)?;
            let response = match block_on(
                py,
                self.state
                    .execute_runtime(RuntimeRequest::Query(QueryRequest::NodeFind(request))),
            )?
            .response
            {
                RuntimeResponse::Node(NodeResponse::Find(response)) => response,
                _ => {
                    return Err(grm_err(grm_rs::GrmError::NotSupported(
                        "runtime dispatcher returned unexpected node find response",
                    )));
                }
            };
            let items = PyList::empty_bound(py);
            for node in response.nodes {
                items.append(stored_node_to_py(py, &node)?)?;
            }
            return Ok(items.into());
        }

        let request = build_node_find_request(
            model_name,
            filters,
            via,
            end_filters,
            edge_filters,
            return_,
            order,
            limit,
            offset,
        )?;
        let result = block_on(py, self.state.node_find(request))?;
        let items = PyList::empty_bound(py);
        match result {
            SessionFindResult::Nodes(nodes) => {
                for node in nodes {
                    items.append(stored_node_to_py(py, &node)?)?;
                }
            }
            SessionFindResult::Edges(rels) => {
                for rel in rels {
                    items.append(stored_rel_to_py(py, &rel)?)?;
                }
            }
        }
        Ok(items.into())
    }

    #[pyo3(signature = (model_name, filters=None, *, via=None, end_filters=None, edge_filters=None, return_=None, order=None, limit=None, offset=None))]
    #[expect(
        clippy::too_many_arguments,
        reason = "PyO3 method signature mirrors the Python API keyword arguments"
    )]
    fn explain_node_find(
        &self,
        py: Python<'_>,
        model_name: &str,
        filters: Option<&Bound<'_, PyDict>>,
        via: Option<&Bound<'_, PyAny>>,
        end_filters: Option<&Bound<'_, PyDict>>,
        edge_filters: Option<&Bound<'_, PyDict>>,
        return_: Option<&str>,
        order: Option<&str>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> PyResult<PyObject> {
        let request = build_node_find_request(
            model_name,
            filters,
            via,
            end_filters,
            edge_filters,
            return_,
            order,
            limit,
            offset,
        )?;
        let value = self
            .state
            .explain(ExplainRequest {
                query: QueryRequest::NodeFind(request),
            })
            .map_err(grm_err)?;
        json_value_to_py(py, &value)
    }

    #[pyo3(signature = (model_name, filters=None, *, via=None, end_filters=None, edge_filters=None, return_=None, order=None, limit=None, offset=None))]
    #[expect(
        clippy::too_many_arguments,
        reason = "PyO3 method signature mirrors the Python API keyword arguments"
    )]
    fn profile_node_find(
        &self,
        py: Python<'_>,
        model_name: &str,
        filters: Option<&Bound<'_, PyDict>>,
        via: Option<&Bound<'_, PyAny>>,
        end_filters: Option<&Bound<'_, PyDict>>,
        edge_filters: Option<&Bound<'_, PyDict>>,
        return_: Option<&str>,
        order: Option<&str>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> PyResult<PyObject> {
        let request = build_node_find_request(
            model_name,
            filters,
            via,
            end_filters,
            edge_filters,
            return_,
            order,
            limit,
            offset,
        )?;
        let value = block_on(
            py,
            self.state.profile(ProfileRequest {
                query: QueryRequest::NodeFind(request),
            }),
        )?;
        json_value_to_py(py, &value)
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
        let from_id = self
            .state
            .parse_backend_id(
                &python_value_to_string(from_id)?,
                self.state.node_id_type(),
                "from node",
            )
            .map_err(grm_err)?;
        let to_id = self
            .state
            .parse_backend_id(
                &python_value_to_string(to_id)?,
                self.state.node_id_type(),
                "to node",
            )
            .map_err(grm_err)?;
        let outcome = block_on(
            py,
            self.state.apply_edge_create(EdgeCreateRequest {
                model: model_name.to_string(),
                from: from_id,
                to: to_id,
                props: extract_json_map(values)?,
            }),
        )?;
        self.append_autocommit(outcome.durable_op)
            .map_err(grm_err)?;
        stored_rel_to_py(py, &outcome.value)
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
        let raw_id = self
            .state
            .parse_backend_id(&id, self.state.rel_id_type(), "edge id")
            .map_err(grm_err)?;
        let outcome = block_on(
            py,
            self.state.apply_edge_update(EdgeUpdateRequest {
                model: model_name.to_string(),
                id: raw_id,
                props: extract_json_map(values)?,
            }),
        )?;
        self.append_autocommit(outcome.durable_op)
            .map_err(grm_err)?;
        stored_rel_to_py(py, &outcome.value)
    }

    fn edge_delete(
        &mut self,
        py: Python<'_>,
        model_name: &str,
        edge_id: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let id = python_value_to_string(edge_id)?;
        let raw_id = self
            .state
            .parse_backend_id(&id, self.state.rel_id_type(), "edge id")
            .map_err(grm_err)?;
        let outcome = block_on(
            py,
            self.state.apply_edge_delete(EdgeDeleteRequest {
                model: model_name.to_string(),
                id: raw_id,
            }),
        )?;
        self.append_autocommit(outcome.durable_op).map_err(grm_err)
    }

    #[pyo3(signature = (model_name, filters=None))]
    fn edge_find(
        &mut self,
        py: Python<'_>,
        model_name: &str,
        filters: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let request = EdgeFindRequest::from_adapter_filter_values(
            model_name.to_string(),
            extract_json_map(filters)?,
        )
        .map_err(grm_err)?;
        let response = match block_on(
            py,
            self.state
                .execute_runtime(RuntimeRequest::Query(QueryRequest::EdgeFind(request))),
        )?
        .response
        {
            RuntimeResponse::Edge(EdgeResponse::Find(response)) => response,
            _ => {
                return Err(grm_err(grm_rs::GrmError::NotSupported(
                    "runtime dispatcher returned unexpected edge find response",
                )));
            }
        };
        let items = PyList::empty_bound(py);
        for rel in response.edges {
            items.append(stored_rel_to_py(py, &rel)?)?;
        }
        Ok(items.into())
    }

    #[pyo3(signature = (model_name, filters=None))]
    fn explain_edge_find(
        &self,
        py: Python<'_>,
        model_name: &str,
        filters: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let terms = build_terms_from_filters(filters)?;
        let value = self
            .state
            .explain_edge_find_terms(model_name, &terms)
            .map_err(grm_err)?;
        json_value_to_py(py, &value)
    }

    #[pyo3(signature = (model_name, filters=None))]
    fn profile_edge_find(
        &self,
        py: Python<'_>,
        model_name: &str,
        filters: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let terms = build_terms_from_filters(filters)?;
        let value = self
            .state
            .profile_edge_find_terms(model_name, &terms)
            .map_err(grm_err)?;
        json_value_to_py(py, &value)
    }

    fn indexes(&self, py: Python<'_>) -> PyResult<PyObject> {
        let value = self.state.index_catalog_value();
        json_value_to_py(py, &value)
    }

    fn save_json(&self, path: &str) -> PyResult<()> {
        self.state.save_to_json(path).map_err(grm_err)
    }

    fn save_binary(&self, path: &str) -> PyResult<()> {
        self.state.save_to_binary(path).map_err(grm_err)
    }

    fn export_json(&self, path: &str) -> PyResult<()> {
        self.state.export_to_json(path).map_err(grm_err)
    }

    fn export_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let value = self.state.export_value().map_err(grm_err)?;
        json_value_to_py(py, &value)
    }

    fn load_json(&mut self, path: &str) -> PyResult<()> {
        self.state.load_from_json(path).map_err(grm_err)?;
        self.checkpoint_autocommit().map_err(grm_err)
    }

    fn load_binary(&mut self, path: &str) -> PyResult<()> {
        self.state.load_from_binary(path).map_err(grm_err)?;
        self.checkpoint_autocommit().map_err(grm_err)
    }

    fn import_json(&mut self, path: &str) -> PyResult<()> {
        self.state.import_from_json(path).map_err(grm_err)?;
        self.checkpoint_autocommit().map_err(grm_err)
    }
}

#[pymethods]
impl PyServiceSession {
    #[new]
    #[pyo3(signature = (*, endpoint, workspace_ref, mode="open", workspace_format="binary"))]
    fn new(
        _py: Python<'_>,
        endpoint: &str,
        workspace_ref: &str,
        mode: &str,
        workspace_format: &str,
    ) -> PyResult<Self> {
        let mode = parse_service_mode(mode)?;
        let format = PyServiceWorkspaceFormat::parse(workspace_format)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
        let client = runtime
            .block_on(GrpcWorkspaceClient::connect_with_format(
                endpoint.to_string(),
                workspace_ref.to_string(),
                mode,
                format.into(),
            ))
            .map_err(service_err)?;
        Ok(Self { runtime, client })
    }

    fn endpoint(&self) -> &str {
        self.client.endpoint()
    }

    fn workspace_ref(&self) -> &str {
        &self.client.workspace_ref().id
    }

    fn node_id_type(&self) -> &'static str {
        "int"
    }

    fn rel_id_type(&self) -> &'static str {
        "int"
    }

    fn model_create(
        &mut self,
        _py: Python<'_>,
        name: &str,
        id_field: &str,
        fields: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        self.runtime
            .block_on(self.client.define_node(DefineNodeRequest {
                name: name.to_string(),
                id_field: id_field.to_string(),
                fields: parse_field_specs(fields)?,
            }))
            .map_err(service_err)
            .map(|_| ())
    }

    fn link_create(
        &mut self,
        _py: Python<'_>,
        name: &str,
        from_model: &str,
        to_model: &str,
        id_field: &str,
        fields: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        self.runtime
            .block_on(self.client.define_edge(DefineEdgeRequest {
                name: name.to_string(),
                from_model: from_model.to_string(),
                to_model: to_model.to_string(),
                id_field: id_field.to_string(),
                fields: parse_field_specs(fields)?,
            }))
            .map_err(service_err)
            .map(|_| ())
    }

    fn model_list(&mut self, py: Python<'_>) -> PyResult<PyObject> {
        let schema = self
            .runtime
            .block_on(self.client.schema_list())
            .map_err(service_err)?;
        let items = PyList::empty_bound(py);
        for model in schema.node_models {
            items.append(runtime_node_model_to_py(py, &model)?)?;
        }
        Ok(items.into())
    }

    fn link_list(&mut self, py: Python<'_>) -> PyResult<PyObject> {
        let schema = self
            .runtime
            .block_on(self.client.schema_list())
            .map_err(service_err)?;
        let items = PyList::empty_bound(py);
        for model in schema.edge_models {
            items.append(runtime_rel_model_to_py(py, &model)?)?;
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
        let node = self
            .runtime
            .block_on(self.client.create_node(NodeCreateRequest {
                model: model_name.to_string(),
                props: extract_json_map(values)?,
            }))
            .map_err(service_err)?;
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
        let node = self
            .runtime
            .block_on(self.client.update_node(NodeUpdateRequest {
                model: model_name.to_string(),
                id: parse_python_i64(node_id, "node id")?,
                props: extract_json_map(values)?,
            }))
            .map_err(service_err)?;
        stored_node_to_py(py, &node)
    }

    fn node_delete(
        &mut self,
        _py: Python<'_>,
        model_name: &str,
        node_id: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        self.runtime
            .block_on(self.client.delete_node(NodeDeleteRequest {
                model: model_name.to_string(),
                id: parse_python_i64(node_id, "node id")?,
            }))
            .map_err(service_err)
            .map(|_| ())
    }

    #[pyo3(signature = (model_name, filters=None))]
    fn node_find(
        &mut self,
        py: Python<'_>,
        model_name: &str,
        filters: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let request =
            NodeFindRequest::from_adapter_filter_values(model_name, extract_json_map(filters)?)
                .map_err(grm_err)?;
        let response = self
            .runtime
            .block_on(self.client.find_nodes(request))
            .map_err(service_err)?;
        let items = PyList::empty_bound(py);
        for node in response.nodes {
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
        let edge = self
            .runtime
            .block_on(self.client.create_edge(EdgeCreateRequest {
                model: model_name.to_string(),
                from: parse_python_i64(from_id, "from node")?,
                to: parse_python_i64(to_id, "to node")?,
                props: extract_json_map(values)?,
            }))
            .map_err(service_err)?;
        stored_rel_to_py(py, &edge)
    }

    #[pyo3(signature = (model_name, edge_id, values=None))]
    fn edge_update(
        &mut self,
        py: Python<'_>,
        model_name: &str,
        edge_id: &Bound<'_, PyAny>,
        values: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let edge = self
            .runtime
            .block_on(self.client.update_edge(EdgeUpdateRequest {
                model: model_name.to_string(),
                id: parse_python_i64(edge_id, "edge id")?,
                props: extract_json_map(values)?,
            }))
            .map_err(service_err)?;
        stored_rel_to_py(py, &edge)
    }

    fn edge_delete(
        &mut self,
        _py: Python<'_>,
        model_name: &str,
        edge_id: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        self.runtime
            .block_on(self.client.delete_edge(EdgeDeleteRequest {
                model: model_name.to_string(),
                id: parse_python_i64(edge_id, "edge id")?,
            }))
            .map_err(service_err)
            .map(|_| ())
    }

    #[pyo3(signature = (model_name, filters=None))]
    fn edge_find(
        &mut self,
        py: Python<'_>,
        model_name: &str,
        filters: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let request =
            EdgeFindRequest::from_adapter_filter_values(model_name, extract_json_map(filters)?)
                .map_err(grm_err)?;
        let response = self
            .runtime
            .block_on(self.client.find_edges(request))
            .map_err(service_err)?;
        let items = PyList::empty_bound(py);
        for edge in response.edges {
            items.append(stored_rel_to_py(py, &edge)?)?;
        }
        Ok(items.into())
    }

    #[pyo3(signature = (ops, *, atomic=true, response="summary", allow_deletes=false))]
    fn batch(
        &mut self,
        py: Python<'_>,
        ops: &Bound<'_, PyAny>,
        atomic: bool,
        response: &str,
        allow_deletes: bool,
    ) -> PyResult<PyObject> {
        let params_value = Value::Object(serde_json::Map::from_iter([
            ("ops".to_string(), py_any_to_json_value(ops)?),
            ("atomic".to_string(), Value::Bool(atomic)),
            ("response".to_string(), Value::String(response.to_string())),
            ("allow_deletes".to_string(), Value::Bool(allow_deletes)),
        ]));
        let params: SessionBatchParams = serde_json::from_value(params_value)
            .map_err(|err| PyTypeError::new_err(format!("invalid batch parameters: {err}")))?;
        let outcome = self
            .runtime
            .block_on(self.client.apply_batch(params))
            .map_err(service_err)?;
        json_value_to_py(py, &outcome.value)
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
        let props = model
            .validate_instance_input(&raw_values)
            .map_err(grm_err)?;
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
            let nodes = tx
                .tx_mut()?
                .find_nodes_by_property(field_name, &value)
                .await?;
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
        let props = model
            .validate_instance_input(&raw_values)
            .map_err(grm_err)?;
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
    fn append_autocommit(&self, op: DurableOperation) -> grm_rs::Result<()> {
        let Some(target) = &self.autocommit else {
            return Ok(());
        };

        self.state
            .append_durable_operation(&target.path, &op)
            .map_err(|err| {
                grm_rs::GrmError::Backend(format!(
                    "autocommit failed for '{}': {}",
                    target.path.display(),
                    err
                ))
            })
    }

    fn append_autocommit_many(&self, ops: &[DurableOperation]) -> grm_rs::Result<()> {
        for op in ops {
            self.append_autocommit(op.clone())?;
        }
        Ok(())
    }

    fn checkpoint_autocommit(&self) -> grm_rs::Result<()> {
        let Some(target) = &self.autocommit else {
            return Ok(());
        };

        self.state
            .checkpoint_durable(target.format.durability_format(), &target.path)
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

fn service_err(err: grm_service_api::GrpcWorkspaceClientError) -> PyErr {
    PyGrmError::new_err(err.to_string())
}

impl PyServiceWorkspaceFormat {
    fn parse(raw: &str) -> PyResult<Self> {
        match raw {
            "json" => Ok(Self::Json),
            "bin" | "binary" => Ok(Self::Binary),
            other => Err(PyTypeError::new_err(format!(
                "unsupported workspace_format '{other}', expected 'binary' or 'json'"
            ))),
        }
    }
}

impl From<PyServiceWorkspaceFormat> for grm_service_api::DurabilityFormat {
    fn from(format: PyServiceWorkspaceFormat) -> Self {
        match format {
            PyServiceWorkspaceFormat::Json => Self::Json,
            PyServiceWorkspaceFormat::Binary => Self::Binary,
        }
    }
}

fn parse_service_mode(raw: &str) -> PyResult<GrpcWorkspaceMode> {
    match raw {
        "create" => Ok(GrpcWorkspaceMode::Create),
        "open" => Ok(GrpcWorkspaceMode::Open),
        other => Err(PyTypeError::new_err(format!(
            "unsupported mode '{other}', expected 'create' or 'open'"
        ))),
    }
}

fn parse_python_i64(value: &Bound<'_, PyAny>, name: &str) -> PyResult<i64> {
    python_value_to_string(value)?
        .parse::<i64>()
        .map_err(|_| PyTypeError::new_err(format!("{name} must be an integer")))
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

fn parse_field_specs(fields: &Bound<'_, PyAny>) -> PyResult<Vec<FieldSpec>> {
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
        let value_type = parse_field_value_type(&field_type).ok_or_else(|| {
            PyTypeError::new_err(format!(
                "unsupported field type '{field_type}', expected one of: string, int, float, bool"
            ))
        })?;
        let required = required_bool(field, "required")?;
        parsed.push(FieldSpec {
            name,
            value_type,
            required,
        });
    }
    Ok(parsed)
}

fn parse_field_value_type(raw: &str) -> Option<FieldValueType> {
    match raw {
        "string" => Some(FieldValueType::String),
        "int" => Some(FieldValueType::Int),
        "float" => Some(FieldValueType::Float),
        "bool" => Some(FieldValueType::Bool),
        _ => None,
    }
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

    fn durability_format(self) -> DurabilityFormat {
        match self {
            Self::Json => DurabilityFormat::Json,
            Self::Binary => DurabilityFormat::Binary,
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

fn extract_json_map(input: Option<&Bound<'_, PyDict>>) -> PyResult<BTreeMap<String, Value>> {
    let mut values = BTreeMap::new();
    let Some(input) = input else {
        return Ok(values);
    };

    for (key, value) in input {
        let key = key
            .extract::<String>()
            .map_err(|_| PyTypeError::new_err("mapping keys must be strings"))?;
        values.insert(key, py_any_to_json_value(&value)?);
    }

    Ok(values)
}

#[expect(
    clippy::too_many_arguments,
    reason = "keeps Python node_find keyword handling explicit and close to the PyO3 signature"
)]
fn build_node_find_request(
    model_name: &str,
    filters: Option<&Bound<'_, PyDict>>,
    via: Option<&Bound<'_, PyAny>>,
    end_filters: Option<&Bound<'_, PyDict>>,
    edge_filters: Option<&Bound<'_, PyDict>>,
    return_: Option<&str>,
    order: Option<&str>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> PyResult<NodeFindRequest> {
    Ok(NodeFindRequest {
        model: model_name.to_string(),
        predicates: collect_filter_predicates(filters)?,
        end_predicates: collect_filter_predicates(end_filters)?,
        edge_predicates: collect_filter_predicates(edge_filters)?,
        traversals: collect_via_steps(via)?,
        order: parse_python_order(order)?,
        limit,
        offset,
        id: None,
        return_mode: return_.map(parse_python_return).transpose()?,
    })
}

fn collect_filter_predicates(
    filters: Option<&Bound<'_, PyDict>>,
) -> PyResult<Vec<PropertyPredicate>> {
    let mut predicates = Vec::new();
    let Some(filters) = filters else {
        return Ok(predicates);
    };

    for (key, value) in filters {
        let raw_key = key
            .extract::<String>()
            .map_err(|_| PyTypeError::new_err("mapping keys must be strings"))?;
        let (field, op) = split_python_predicate_key(&raw_key)?;
        predicates.push(PropertyPredicate {
            field: field.to_string(),
            op,
            value: py_any_to_json_value(&value)?,
        });
    }
    Ok(predicates)
}

fn build_terms_from_filters(filters: Option<&Bound<'_, PyDict>>) -> PyResult<Vec<QueryTerm>> {
    let mut terms = Vec::new();
    for (key, value) in extract_string_map(filters)? {
        terms.push(QueryTerm { key, value });
    }
    Ok(terms)
}

fn collect_via_steps(via: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<TraversalStepRequest>> {
    let mut steps = Vec::new();
    let Some(via) = via else {
        return Ok(steps);
    };

    for item in via.iter().map_err(|_| {
        PyTypeError::new_err("via must be a list of dicts with 'dir', 'link', and 'model' keys")
    })? {
        let item = item?;
        let step = item.downcast::<PyDict>().map_err(|_| {
            PyTypeError::new_err("via entries must be dicts with 'dir', 'link', and 'model' keys")
        })?;
        let direction = required_traversal_string(step, "dir")?;
        let link = required_traversal_string(step, "link")?;
        let model = required_traversal_string(step, "model")?;
        steps.push(TraversalStepRequest {
            direction: parse_python_direction(&direction)?,
            edge_model: if link == "*" { None } else { Some(link) },
            end_model: model,
        });
    }

    Ok(steps)
}

fn split_python_predicate_key(raw_key: &str) -> PyResult<(&str, PredicateOp)> {
    for (suffix, op) in [
        ("!", PredicateOp::Ne),
        (">=", PredicateOp::Ge),
        ("<=", PredicateOp::Le),
        (">", PredicateOp::Gt),
        ("<", PredicateOp::Lt),
        ("~", PredicateOp::Contains),
    ] {
        if let Some(field) = raw_key.strip_suffix(suffix) {
            if !field.is_empty() {
                return Ok((field, op));
            }
        }
    }
    Ok((raw_key, PredicateOp::Eq))
}

fn parse_python_direction(raw: &str) -> PyResult<TraversalDirection> {
    match raw {
        "out" | "outgoing" => Ok(TraversalDirection::Out),
        "in" | "incoming" => Ok(TraversalDirection::In),
        "both" => Ok(TraversalDirection::Both),
        _ => Err(PyTypeError::new_err(
            "via direction must be one of: out, in, both",
        )),
    }
}

fn parse_python_return(raw: &str) -> PyResult<TraversalReturn> {
    match raw {
        "end" => Ok(TraversalReturn::End),
        "root" => Ok(TraversalReturn::Root),
        "edge" | "rel" => Ok(TraversalReturn::Edge),
        _ => Err(PyTypeError::new_err(
            "return must be one of: root, end, edge",
        )),
    }
}

fn parse_python_order(raw: Option<&str>) -> PyResult<Vec<OrderSpec>> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let mut order = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for segment in raw.split(',') {
        let Some((field, direction)) = segment.split_once(':') else {
            return Err(PyTypeError::new_err(
                "order must use order=<field>:asc|desc[,<field>:asc|desc ...]",
            ));
        };
        if !seen.insert(field.to_string()) {
            return Err(PyTypeError::new_err(format!(
                "duplicate order field '{field}'"
            )));
        }
        let direction = match direction {
            "asc" => OrderDirection::Asc,
            "desc" => OrderDirection::Desc,
            _ => return Err(PyTypeError::new_err("order direction must be asc or desc")),
        };
        order.push(OrderSpec {
            field: field.to_string(),
            direction,
        });
    }
    Ok(order)
}

fn required_traversal_string(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<String> {
    let value = dict
        .get_item(key)?
        .ok_or_else(|| PyTypeError::new_err(format!("via entries require key '{key}'")))?;
    value
        .extract::<String>()
        .map_err(|_| PyTypeError::new_err(format!("via entry '{key}' must be a string")))
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
    if let Ok(dict) = value.downcast::<PyDict>() {
        let mut values = serde_json::Map::new();
        for (key, value) in dict {
            let key = key
                .extract::<String>()
                .map_err(|_| PyTypeError::new_err("JSON object keys must be strings"))?;
            values.insert(key, py_any_to_json_value(&value)?);
        }
        return Ok(Value::Object(values));
    }
    if let Ok(list) = value.downcast::<PyList>() {
        let mut values = Vec::with_capacity(list.len());
        for item in list {
            values.push(py_any_to_json_value(&item)?);
        }
        return Ok(Value::Array(values));
    }
    Err(PyTypeError::new_err(
        "JSON values must be None, bool, int, float, string, dict, or list",
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
    module.add_class::<PyServiceSession>()?;
    module.add_class::<PyNeo4jSession>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::Server;

    #[tokio::test]
    async fn service_session_routes_supported_python_surface_through_grpc() {
        pyo3::prepare_freethreaded_python();
        let temp = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let service = grm_service_api::GrpcWorkspaceService::with_local_workspace_root(temp.path())
            .into_server();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(service)
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = shutdown_rx.await;
                })
                .await
        });

        Python::with_gil(|py| {
            let mut session = PyServiceSession::new(
                py,
                &format!("http://{addr}"),
                "python-service-smoke",
                "create",
                "binary",
            )
            .unwrap();
            let field = PyDict::new_bound(py);
            field.set_item("name", "name").unwrap();
            field.set_item("type", "string").unwrap();
            field.set_item("required", true).unwrap();
            let fields = PyList::empty_bound(py);
            fields.append(field).unwrap();
            session
                .model_create(py, "User", "userId", fields.as_any())
                .unwrap();
            let values = PyDict::new_bound(py);
            values.set_item("name", "Ada").unwrap();
            let created = session
                .node_create(py, "User", Some(&values))
                .unwrap()
                .bind(py)
                .downcast::<PyDict>()
                .unwrap()
                .get_item("id")
                .unwrap()
                .unwrap()
                .extract::<i64>()
                .unwrap();
            let filters = PyDict::new_bound(py);
            filters.set_item("id", created).unwrap();
            let found = session.node_find(py, "User", Some(&filters)).unwrap();
            assert_eq!(found.bind(py).downcast::<PyList>().unwrap().len(), 1);
        });

        assert!(temp.path().join("python-service-smoke.bin").exists());
        shutdown_tx.send(()).unwrap();
        server.await.unwrap().unwrap();
    }
}
