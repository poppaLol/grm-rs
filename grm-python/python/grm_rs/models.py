"""Typed-object helpers for GRM Python adapter conveniences."""

from dataclasses import dataclass
from dataclasses import MISSING, fields as dataclass_fields, is_dataclass
from inspect import Parameter, signature
from typing import (
    Any,
    ClassVar,
    Callable,
    Dict,
    List,
    Optional,
    Sequence,
    Type,
    get_args,
    get_origin,
    cast,
)

from .typing import Edge, FieldDefinition, FieldType, GraphId, GraphValue, Node, PropertyMap


@dataclass(frozen=True)
class _ValueAdapter:
    annotations: tuple[object, ...]
    field_type: FieldType
    value_matches: Callable[[object], bool]
    serialize: Callable[[object], GraphValue]
    display: str


def _as_graph_value(value: object) -> GraphValue:
    return cast(GraphValue, value)


_VALUE_ADAPTERS: tuple[_ValueAdapter, ...] = (
    _ValueAdapter(
        (str,),
        "string",
        lambda value: type(value) is str,
        _as_graph_value,
        "str",
    ),
    _ValueAdapter(
        (int,),
        "int",
        lambda value: type(value) is int,
        _as_graph_value,
        "int",
    ),
    _ValueAdapter(
        (float,),
        "float",
        lambda value: type(value) is float,
        _as_graph_value,
        "float",
    ),
    _ValueAdapter(
        (bool,),
        "bool",
        lambda value: type(value) is bool,
        _as_graph_value,
        "bool",
    ),
)
_ANNOTATION_ADAPTERS: Dict[object, _ValueAdapter] = {
    annotation: adapter
    for adapter in _VALUE_ADAPTERS
    for annotation in adapter.annotations
}
_SUPPORTED_VALUE_DISPLAY = ", ".join(adapter.display for adapter in _VALUE_ADAPTERS)
_METADATA_NAMES = {
    "__grm_id_field__",
    "__grm_link_name__",
    "__grm_from_model__",
    "__grm_to_model__",
    "__grm_from_id_field__",
    "__grm_to_id_field__",
}


class GrmNode:
    """Optional thin mixin for typed node objects."""

    __grm_id_field__: str

    def save(self, graph: Any) -> Node:
        return graph.node_create(self)  # type: ignore[no-any-return]


class GrmEdge:
    """Optional thin mixin for typed edge objects."""

    __grm_link_name__: str
    __grm_from_model__: str
    __grm_to_model__: str
    __grm_id_field__: str
    __grm_from_id_field__: str
    __grm_to_id_field__: str

    def save(self, graph: Any) -> Edge:
        return graph.edge_create(self)  # type: ignore[no-any-return]


def node_model_args(
    model: Type[Any], id_field: Optional[str]
) -> tuple[str, str, List[FieldDefinition]]:
    resolved_id_field = _node_id_field(model, id_field)
    return (
        model.__name__,
        resolved_id_field,
        _field_definitions(model, exclude={resolved_id_field}),
    )


def link_model_args(
    model: Type[Any],
) -> tuple[str, str, str, str, List[FieldDefinition]]:
    link_name = _required_str_attr(model, "__grm_link_name__")
    from_model = _required_str_attr(model, "__grm_from_model__")
    to_model = _required_str_attr(model, "__grm_to_model__")
    id_field = _required_str_attr(model, "__grm_id_field__")
    endpoint_fields = {
        id_field,
        _optional_str_attr(model, "__grm_from_id_field__"),
        _optional_str_attr(model, "__grm_to_id_field__"),
    }
    return (
        link_name,
        from_model,
        to_model,
        id_field,
        _field_definitions(model, exclude={name for name in endpoint_fields if name}),
    )


def node_create_args(instance: object) -> tuple[str, PropertyMap]:
    return type(instance).__name__, _properties(instance)


def edge_create_args(instance: object) -> tuple[str, GraphId, GraphId, PropertyMap]:
    model = type(instance)
    link_name = _required_str_attr(model, "__grm_link_name__")
    from_id_field = _required_str_attr(model, "__grm_from_id_field__")
    to_id_field = _required_str_attr(model, "__grm_to_id_field__")
    from_id = _endpoint_id(instance, from_id_field)
    to_id = _endpoint_id(instance, to_id_field)
    props = _properties(instance)
    props.pop(from_id_field, None)
    props.pop(to_id_field, None)
    id_field = _optional_str_attr(model, "__grm_id_field__")
    if id_field:
        props.pop(id_field, None)
    return link_name, from_id, to_id, props


def _node_id_field(model: Type[Any], explicit: Optional[str]) -> str:
    if explicit is not None:
        if not isinstance(explicit, str) or not explicit:
            raise TypeError("id_field must be a non-empty string")
        return explicit
    return _required_str_attr(model, "__grm_id_field__")


def _required_str_attr(model: Type[Any], name: str) -> str:
    value = getattr(model, name, None)
    if not isinstance(value, str) or not value:
        raise TypeError(f"{model.__name__} must define {name} as a non-empty string")
    return value


def _optional_str_attr(model: Type[Any], name: str) -> Optional[str]:
    value = getattr(model, name, None)
    if value is None:
        return None
    if not isinstance(value, str) or not value:
        raise TypeError(f"{model.__name__}.{name} must be a non-empty string")
    return value


def _field_definitions(model: Type[Any], *, exclude: set[str]) -> List[FieldDefinition]:
    pydantic_fields = _pydantic_fields(model)
    if pydantic_fields is not None:
        return [
            {
                "name": name,
                "type": _field_type(model, name, annotation),
                "required": required,
            }
            for name, annotation, required in pydantic_fields
            if name not in exclude and name not in _METADATA_NAMES
        ]
    if is_dataclass(model):
        return [
            {
                "name": field.name,
                "type": _field_type(model, field.name, field.type),
                "required": field.default is MISSING
                and field.default_factory is MISSING,
            }
            for field in dataclass_fields(model)
            if field.name not in exclude and field.name not in _METADATA_NAMES
        ]
    annotations = getattr(model, "__annotations__", {})
    return [
        {
            "name": name,
            "type": _field_type(model, name, annotation),
            "required": True,
        }
        for name, annotation in annotations.items()
        if name not in exclude
        and name not in _METADATA_NAMES
        and get_origin(annotation) is not ClassVar
    ]


def _pydantic_fields(model: Type[Any]) -> Optional[Sequence[tuple[str, object, bool]]]:
    model_fields = getattr(model, "model_fields", None)
    if isinstance(model_fields, dict):
        fields: List[tuple[str, object, bool]] = []
        for name, field in model_fields.items():
            required_fn = getattr(field, "is_required", None)
            required = bool(required_fn()) if callable(required_fn) else False
            fields.append((name, getattr(field, "annotation", Any), required))
        return fields
    legacy_fields = getattr(model, "__fields__", None)
    if isinstance(legacy_fields, dict):
        return [
            (
                name,
                getattr(field, "outer_type_", getattr(field, "type_", Any)),
                bool(getattr(field, "required", False)),
            )
            for name, field in legacy_fields.items()
        ]
    return None


def _field_type(model: Type[Any], name: str, annotation: object) -> FieldType:
    if get_origin(annotation) is not None or get_args(annotation):
        raise TypeError(
            f"{model.__name__}.{name} uses unsupported field type {annotation!r}; "
            f"supported GRM typed-object fields are {_SUPPORTED_VALUE_DISPLAY}"
        )
    try:
        return _ANNOTATION_ADAPTERS[annotation].field_type
    except KeyError as err:
        raise TypeError(
            f"{model.__name__}.{name} uses unsupported field type {annotation!r}; "
            f"supported GRM typed-object fields are {_SUPPORTED_VALUE_DISPLAY}"
        ) from err


def _properties(instance: object) -> PropertyMap:
    raw = _raw_properties(instance)
    props: PropertyMap = {}
    for name, value in raw.items():
        if name.startswith("_"):
            continue
        props[name] = _property_value(instance, name, value)
    return props


def _raw_properties(instance: object) -> Dict[str, object]:
    model_dump = getattr(instance, "model_dump", None)
    if callable(model_dump):
        raw = _model_dump_python(model_dump)
    elif is_dataclass(instance):
        raw = {
            field.name: getattr(instance, field.name)
            for field in dataclass_fields(instance)
        }
    else:
        raw = getattr(instance, "__dict__", None)
    if not isinstance(raw, dict):
        raise TypeError(
            f"{type(instance).__name__} must provide model_dump(), be a dataclass, "
            "or expose a __dict__ for GRM typed-object writes"
        )
    return {str(name): value for name, value in raw.items()}


def _model_dump_python(model_dump: Callable[..., object]) -> object:
    try:
        parameters = signature(model_dump).parameters
    except (TypeError, ValueError):
        return model_dump()
    accepts_mode = "mode" in parameters or any(
        parameter.kind is Parameter.VAR_KEYWORD
        for parameter in parameters.values()
    )
    if accepts_mode:
        return model_dump(mode="python")
    return model_dump()


def _property_value(instance: object, name: str, value: object) -> GraphValue:
    for adapter in _VALUE_ADAPTERS:
        if adapter.value_matches(value):
            return adapter.serialize(value)
    raise TypeError(
        f"{type(instance).__name__}.{name} has unsupported property value "
        f"{value!r}; supported GRM values are {_SUPPORTED_VALUE_DISPLAY}"
    )


def _endpoint_id(instance: object, field_name: str) -> GraphId:
    value = getattr(instance, field_name, None)
    if not isinstance(value, int) or isinstance(value, bool):
        raise TypeError(
            f"{type(instance).__name__}.{field_name} must be an integer GRM endpoint id"
        )
    return value
