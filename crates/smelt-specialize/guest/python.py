"""CPython guest for Smelt definition-time specialization.

The host writes one JSON request into the sandbox scratch directory. This
guest imports every candidate module in dependency order, snapshots final
bindings, and writes exactly one specialization manifest to stdout.
"""

from __future__ import annotations

import contextlib
import dataclasses
import enum
import hashlib
import importlib
import inspect
import json
import os
import pathlib
import random
import socket
import subprocess
import sys
import time
import traceback
import types
import typing


class SpecializationRejected(RuntimeError):
    """Raised when source performs a nondeterministic or forbidden operation."""


class EventStream:
    """Records ordered output writes without forwarding them to the host stream."""

    def __init__(self, events: list[dict[str, object]], stderr: bool) -> None:
        """Create a stream that appends writes to ``events``."""
        self.events = events
        self.stderr = stderr

    def write(self, text: str) -> int:
        """Record output emitted from an active definition-time callable."""
        caller = inspect.currentframe()
        caller = None if caller is None else caller.f_back
        definition_time = (
            caller is not None and caller.f_code.co_name != "<module>"
        )
        if text and definition_time:
            self.events.append({"kind": "output", "stderr": self.stderr, "text": text})
        return len(text)

    def flush(self) -> None:
        """Satisfy the text stream protocol."""


class Serializer:
    """Serialize Python values while preserving references, cycles, and identity."""

    def __init__(self, project_root: pathlib.Path) -> None:
        """Create an empty value graph rooted at ``project_root``."""
        self.project_root = project_root.resolve()
        self.nodes: list[dict[str, object]] = []
        self.identities: dict[int, int] = {}
        self.adapters: dict[str, dict[str, object]] = {}

    def add_adapter(
        self, adapter_id: str, reason: str, span: dict[str, object] | None = None
    ) -> None:
        """Record one deduplicated native specialization adapter requirement."""
        self.adapters.setdefault(
            adapter_id,
            {
                "id": adapter_id,
                "version": "1",
                "reason": reason,
                "span": span,
            },
        )

    def value(self, value: object) -> int:
        """Serialize ``value`` and return its graph node ID."""
        identity_bearing = not isinstance(
            value, (type(None), bool, int, float, str, bytes)
        )
        object_id = id(value)
        if identity_bearing and object_id in self.identities:
            return self.identities[object_id]

        node_id = len(self.nodes)
        self.nodes.append({})
        if identity_bearing:
            self.identities[object_id] = node_id
        static_type, payload = self._payload(value)
        self.nodes[node_id] = {"id": node_id, "ty": static_type, "value": payload}
        return node_id

    def _payload(self, value: object) -> tuple[dict[str, object], dict[str, object]]:
        """Return the static type and graph payload for one value."""
        if value is None:
            return unit_type("null"), {"kind": "null"}
        if isinstance(value, bool):
            return unit_type("bool"), {"kind": "bool", "value": value}
        if isinstance(value, int):
            return unit_type("int"), {"kind": "int", "value": str(value)}
        if isinstance(value, float):
            return unit_type("float"), {"kind": "float", "value": value}
        if isinstance(value, str):
            return unit_type("string"), {"kind": "string", "value": value}
        if isinstance(value, bytes):
            return unit_type("bytes"), {"kind": "bytes", "value": list(value)}
        if isinstance(value, enum.Enum):
            class_name = qualified_type_name(type(value))
            return named_type(class_name), {
                "kind": "enum",
                "value": {"class": class_name, "member": value.name},
            }
        if isinstance(value, tuple):
            values = [self.value(item) for item in value]
            return {
                "kind": "tuple",
                "value": [self.nodes[item]["ty"] for item in values],
            }, {"kind": "tuple", "value": values}
        if isinstance(value, list):
            values = [self.value(item) for item in value]
            return collection_type("list", values, self.nodes), {
                "kind": "list",
                "value": values,
            }
        if isinstance(value, (set, frozenset)):
            values = [self.value(item) for item in sorted(value, key=stable_key)]
            return collection_type("set", values, self.nodes), {
                "kind": "set",
                "value": values,
            }
        if isinstance(value, dict):
            entries = [
                [self.value(key), self.value(item)]
                for key, item in sorted(value.items(), key=lambda pair: stable_key(pair[0]))
            ]
            key_ids = [entry[0] for entry in entries]
            value_ids = [entry[1] for entry in entries]
            return {
                "kind": "dict",
                "value": [
                    common_type(key_ids, self.nodes),
                    common_type(value_ids, self.nodes),
                ],
            }, {"kind": "dict", "value": entries}
        if isinstance(value, property):
            return named_type("builtins.property"), {
                "kind": "instance",
                "value": {
                    "class": "builtins.property",
                    # Accessor provenance is serialized by `class_definition`;
                    # the built-in property object has no independent runtime
                    # state that generated Rust needs to construct.
                    "fields": {},
                },
            }
        if inspect.isclass(value):
            return named_type(qualified_type_name(value)), {
                "kind": "class_ref",
                "value": {
                    "module": value.__module__,
                    "qualified_name": value.__qualname__,
                },
            }
        if callable(value):
            provenance = callable_provenance(value, self)
            return function_static_type(value, self), {
                "kind": "function_ref",
                "value": provenance,
            }

        class_name = qualified_type_name(type(value))
        fields = object_fields(value)
        if fields is None:
            adapter_id = f"python.native-value:{class_name}"
            self.add_adapter(
                adapter_id,
                f"value of type '{class_name}' has no serializable concrete state",
            )
            fields = {}
        return named_type(class_name), {
            "kind": "instance",
            "value": {
                "class": class_name,
                "fields": {
                    name: self.value(item)
                    for name, item in sorted(fields.items())
                    if not name.startswith("__")
                },
            },
        }


def unit_type(kind: str) -> dict[str, object]:
    """Build one unit static-type encoding."""
    return {"kind": kind}


def named_type(name: str) -> dict[str, object]:
    """Build one named static-type encoding."""
    return {"kind": "named", "value": name}


def qualified_type_name(value_type: type[object]) -> str:
    """Return a stable module-qualified type name."""
    return f"{value_type.__module__}.{value_type.__qualname__}"


def stable_key(value: object) -> tuple[str, str]:
    """Return a deterministic ordering key for set members and dictionary keys."""
    value_type = qualified_type_name(type(value))
    if value is None or isinstance(value, (bool, int, float, str, bytes)):
        return value_type, repr(value)
    if isinstance(value, enum.Enum):
        return value_type, value.name
    fields = object_fields(value)
    if fields is not None:
        simple = {
            name: repr(item)
            for name, item in sorted(fields.items())
            if isinstance(item, (type(None), bool, int, float, str, bytes))
        }
        return value_type, json.dumps(simple, sort_keys=True)
    return value_type, getattr(value, "__qualname__", getattr(value, "__name__", ""))


def object_fields(value: object) -> dict[str, object] | None:
    """Return concrete instance state without invoking arbitrary properties."""
    dynamic_attribute_override(value)
    if dataclasses.is_dataclass(value) and not inspect.isclass(value):
        try:
            fields = dataclasses.fields(value)
        except (AttributeError, TypeError, ValueError) as error:
            raise RuntimeError(
                "smelt::specialization-required: "
                f"cannot inspect dataclass '{qualified_type_name(type(value))}': {error}"
            ) from error
        return {
            field.name: object.__getattribute__(value, field.name) for field in fields
        }
    try:
        namespace = object.__getattribute__(value, "__dict__")
    except (AttributeError, TypeError):
        namespace = None
    if isinstance(namespace, dict):
        return dict(namespace)
    slots = getattr(type(value), "__slots__", ())
    if isinstance(slots, str):
        slots = (slots,)
    if slots:
        result: dict[str, object] = {}
        for slot in slots:
            try:
                result[slot] = object.__getattribute__(value, slot)
            except AttributeError:
                continue
        return result
    return None


def dynamic_attribute_override(value: object) -> None:
    """Reject runtime-dynamic attribute protocols before serializing a partial shell."""
    hooks = ("__getattr__", "__getattribute__", "__setattr__", "__delattr__")
    for owner in type(value).__mro__:
        if owner is object:
            continue
        namespace = vars(owner)
        for hook in hooks:
            if hook not in namespace:
                continue
            implementation = namespace[hook]
            try:
                source_file = inspect.getsourcefile(implementation) or inspect.getfile(owner)
            except TypeError:
                source_file = None
            try:
                _, line = inspect.getsourcelines(implementation)
            except (OSError, TypeError):
                line = 0
            location = f"{source_file}:{line}" if source_file else "<unknown>"
            raise RuntimeError(
                "smelt::dynamic-attribute-access: "
                f"'{qualified_type_name(type(value))}' inherits {owner.__qualname__}.{hook} "
                f"at {location}; runtime-dynamic attributes cannot be specialized"
            )


def common_type(value_ids: list[int], nodes: list[dict[str, object]]) -> dict[str, object]:
    """Return a concrete common graph-node type or an intentional metadata boundary."""
    if not value_ids:
        return named_type("builtins.object")
    resolved = [nodes[node_id]["ty"] for node_id in value_ids if "ty" in nodes[node_id]]
    if not resolved:
        return named_type("builtins.object")
    first = resolved[0]
    if all(item == first for item in resolved):
        return typing.cast(dict[str, object], first)
    return {"kind": "dynamic_metadata"}


def collection_type(
    kind: str, value_ids: list[int], nodes: list[dict[str, object]]
) -> dict[str, object]:
    """Build a homogeneous collection static type."""
    return {"kind": kind, "value": common_type(value_ids, nodes)}


def annotation_type(annotation: object) -> dict[str, object]:
    """Map a runtime annotation to the manifest static-type algebra."""
    if annotation in (inspect.Signature.empty, inspect.Parameter.empty):
        return named_type("builtins.object")
    if annotation is None or annotation is type(None):
        return unit_type("null")
    primitive = {
        bool: "bool",
        int: "int",
        float: "float",
        str: "string",
        bytes: "bytes",
    }.get(annotation)
    if primitive is not None:
        return unit_type(primitive)
    if annotation is typing.Any:
        return {"kind": "dynamic_metadata"}
    origin = typing.get_origin(annotation)
    arguments = typing.get_args(annotation)
    if origin in (list, set):
        item = annotation_type(arguments[0]) if arguments else named_type("builtins.object")
        return {"kind": origin.__name__, "value": item}
    if origin is tuple:
        return {"kind": "tuple", "value": [annotation_type(item) for item in arguments]}
    if origin is dict:
        key = annotation_type(arguments[0]) if arguments else named_type("builtins.object")
        item = annotation_type(arguments[1]) if len(arguments) > 1 else named_type(
            "builtins.object"
        )
        return {"kind": "dict", "value": [key, item]}
    if inspect.isclass(annotation):
        return named_type(qualified_type_name(annotation))
    if isinstance(annotation, str):
        return named_type(annotation)
    return named_type(str(annotation))


def function_static_type(
    function: object, serializer: Serializer
) -> dict[str, object]:
    """Build the static function type used by graph callable references."""
    return {
        "kind": "function",
        "value": function_signature(function, serializer),
    }


def function_signature(
    function: object, serializer: Serializer
) -> dict[str, object]:
    """Serialize a Python callable signature and concrete defaults."""
    try:
        signature = inspect.signature(function, follow_wrapped=False)
    except (TypeError, ValueError):
        return {
            "parameters": [],
            "return_type": named_type("builtins.object"),
            "is_async": False,
            "throws": True,
        }
    parameters = []
    for parameter in signature.parameters.values():
        default = (
            None
            if parameter.default is inspect.Parameter.empty
            else serializer.value(parameter.default)
        )
        parameters.append(
            {
                "name": parameter.name,
                "ty": annotation_type(parameter.annotation),
                "kind": parameter_kind(parameter.kind),
                "default": default,
                "annotation": (
                    None
                    if parameter.annotation is inspect.Parameter.empty
                    else annotation_text(parameter.annotation)
                ),
            }
        )
    return {
        "parameters": parameters,
        "return_type": annotation_type(signature.return_annotation),
        "is_async": inspect.iscoroutinefunction(function),
        "throws": True,
    }


def parameter_kind(kind: inspect._ParameterKind) -> str:
    """Map CPython's signature kind to the manifest spelling."""
    return {
        inspect.Parameter.POSITIONAL_ONLY: "positional_only",
        inspect.Parameter.POSITIONAL_OR_KEYWORD: "positional",
        inspect.Parameter.VAR_POSITIONAL: "variadic_positional",
        inspect.Parameter.KEYWORD_ONLY: "keyword_only",
        inspect.Parameter.VAR_KEYWORD: "variadic_keyword",
    }[kind]


def annotation_text(annotation: object) -> str:
    """Return a deterministic annotation spelling."""
    if isinstance(annotation, str):
        return annotation
    return getattr(annotation, "__qualname__", repr(annotation))


def source_span(function: object) -> tuple[dict[str, object], str] | None:
    """Resolve a callable to a byte span and source-code hash."""
    code = getattr(function, "__code__", None)
    if code is None:
        return None
    path = pathlib.Path(code.co_filename)
    try:
        source_bytes = path.read_bytes()
        source_text = source_bytes.decode("utf-8")
    except (OSError, UnicodeError, TypeError):
        return None
    line_offsets = [0]
    for line in source_text.splitlines(keepends=True):
        line_offsets.append(line_offsets[-1] + len(line.encode("utf-8")))
    first_line = max(code.co_firstlineno, 1)
    code_lines = [
        line
        for _, _, line in code.co_lines()
        if line is not None and line >= first_line
    ]
    last_line = max(code_lines, default=first_line)
    start_index = min(first_line - 1, len(line_offsets) - 1)
    end_index = min(last_line, len(line_offsets) - 1)
    start = line_offsets[start_index]
    end = line_offsets[end_index]
    body = source_bytes[start:end]
    return {
        "file": str(path.resolve()),
        "start": start,
        "end": end,
    }, hashlib.sha256(body).hexdigest()


def callable_provenance(function: object, serializer: Serializer) -> dict[str, object]:
    """Map a runtime callable back to source and serialize closure captures."""
    unwrapped = function
    if isinstance(unwrapped, (staticmethod, classmethod)):
        unwrapped = unwrapped.__func__
    resolved = source_span(unwrapped)
    module = getattr(unwrapped, "__module__", "")
    code = getattr(unwrapped, "__code__", None)
    qualified_name = (
        getattr(code, "co_qualname", code.co_name)
        if code is not None
        else getattr(
            unwrapped,
            "__qualname__",
            getattr(unwrapped, "__name__", type(unwrapped).__name__),
        )
    )
    if resolved is None:
        adapter_id = f"python.native-callable:{module}.{qualified_name}"
        serializer.add_adapter(
            adapter_id,
            f"callable '{module}.{qualified_name}' cannot be mapped to transpiled source",
        )
        span = {"file": "", "start": 0, "end": 0}
        code_hash = ""
    else:
        span, code_hash = resolved
        try:
            pathlib.Path(typing.cast(str, span["file"])).resolve().relative_to(
                serializer.project_root
            )
        except ValueError:
            adapter_id = f"python.external-callable:{module}.{qualified_name}"
            serializer.add_adapter(
                adapter_id,
                f"callable '{module}.{qualified_name}' is outside the transpiled source graph",
                span,
            )
    captures: dict[str, int] = {}
    closure = getattr(unwrapped, "__closure__", None)
    code = getattr(unwrapped, "__code__", None)
    if closure is not None and code is not None:
        if len(code.co_freevars) != len(closure):
            serializer.add_adapter(
                f"python.invalid-closure:{module}.{qualified_name}",
                f"callable '{module}.{qualified_name}' has inconsistent closure metadata",
                span,
            )
            return {
                "language": "python",
                "module": module,
                "qualified_name": qualified_name,
                "span": span,
                "code_hash": code_hash,
                "captures": captures,
                "binding_mode": binding_mode(qualified_name, function),
            }
        for name, cell in zip(code.co_freevars, closure):
            try:
                captures[name] = serializer.value(cell.cell_contents)
            except ValueError:
                continue
    return {
        "language": "python",
        "module": module,
        "qualified_name": qualified_name,
        "span": span,
        "code_hash": code_hash,
        "captures": captures,
        "binding_mode": binding_mode(qualified_name, function),
    }


def binding_mode(qualified_name: str, function: object) -> str:
    """Infer receiver binding from callable metadata."""
    if isinstance(function, classmethod):
        return "class"
    if "." in qualified_name and "<locals>" not in qualified_name:
        return "instance"
    return "free"


def wrapper_chain(function: object, serializer: Serializer) -> list[dict[str, object]]:
    """Serialize the ``__wrapped__`` chain without executing wrappers."""
    chain = []
    seen: set[int] = set()
    current = function
    while callable(current) and id(current) not in seen:
        seen.add(id(current))
        chain.append(callable_provenance(current, serializer))
        current = getattr(current, "__wrapped__", None)
    return chain


def function_definition(
    name: str, function: object, serializer: Serializer, mode: str | None = None
) -> dict[str, object]:
    """Materialize one final callable binding."""
    chain = wrapper_chain(function, serializer)
    callable_record = chain[0] if chain else callable_provenance(function, serializer)
    final_mode = mode or callable_record["binding_mode"]
    return {
        "name": name,
        "signature": function_signature(function, serializer),
        "callable": callable_record,
        "wrapper_chain": chain[1:],
        "binding_mode": final_mode,
    }


def source_provenance(
    module_name: str, binding_name: str, value: object, serializer: Serializer
) -> dict[str, object]:
    """Resolve the original source name and span for one final binding."""
    target = value
    seen: set[int] = set()
    while callable(target) and id(target) not in seen and hasattr(target, "__wrapped__"):
        seen.add(id(target))
        target = target.__wrapped__
    resolved = source_span(target)
    if resolved is None and inspect.isclass(value):
        try:
            path = pathlib.Path(inspect.getsourcefile(value) or "")
            lines, first_line = inspect.getsourcelines(value)
            source = path.read_text(encoding="utf-8")
            offsets = [0]
            for line in source.splitlines(keepends=True):
                offsets.append(offsets[-1] + len(line.encode("utf-8")))
            start = offsets[max(first_line - 1, 0)]
            end = offsets[min(first_line - 1 + len(lines), len(offsets) - 1)]
            resolved = (
                {"file": str(path.resolve()), "start": start, "end": end},
                hashlib.sha256(source.encode("utf-8")[start:end]).hexdigest(),
            )
        except (OSError, TypeError):
            resolved = None
    if resolved is None:
        serializer.add_adapter(
            f"python.opaque-definition:{module_name}.{binding_name}",
            f"definition '{module_name}.{binding_name}' has no source span",
        )
        span = {"file": "", "start": 0, "end": 0}
    else:
        span = resolved[0]
    return {
        "module": getattr(target, "__module__", module_name),
        "qualified_name": getattr(target, "__qualname__", binding_name),
        "span": span,
    }


def class_definition(
    class_value: type[object], serializer: Serializer
) -> dict[str, object]:
    """Materialize concrete class layout after metaclass and descriptor hooks."""
    annotations = dict(getattr(class_value, "__annotations__", {}))
    fields: list[dict[str, object]] = []
    for name, annotation in sorted(annotations.items()):
        default_value = class_value.__dict__.get(name, inspect.Parameter.empty)
        fields.append(
            {
                "name": name,
                "ty": annotation_type(annotation),
                "default": (
                    None
                    if default_value is inspect.Parameter.empty
                    else serializer.value(default_value)
                ),
                "is_static": False,
                "is_private": name.startswith("__") and not name.endswith("__"),
                "span": None,
            }
        )
    django_structure = django_model_structure(class_value)
    if django_structure is not None:
        known_fields = {field["name"] for field in fields}
        fields.extend(
            field
            for field in typing.cast(list[dict[str, object]], django_structure["fields"])
            if field["name"] not in known_fields
        )

    descriptors = []
    methods = []
    static_values: dict[str, int] = {}
    for name, raw_value in sorted(class_value.__dict__.items()):
        if name.startswith("__") and name.endswith("__"):
            continue
        if name == "_meta" and django_structure is not None:
            continue
        method, mode = unwrap_method(raw_value)
        if method is not None:
            methods.append(function_definition(name, method, serializer, mode))
            continue
        if hasattr(type(raw_value), "__get__") or isinstance(raw_value, property):
            if isinstance(raw_value, property):
                getter = raw_value.fget
                setter = raw_value.fset
                data_descriptor = True
            else:
                getter = getattr(type(raw_value), "__get__", None)
                setter = getattr(type(raw_value), "__set__", None)
                data_descriptor = setter is not None
            read_annotation = (
                inspect.signature(getter).return_annotation
                if getter is not None and source_span(getter) is not None
                else inspect.Signature.empty
            )
            descriptors.append(
                {
                    "name": name,
                    "value": serializer.value(raw_value),
                    "read_type": annotation_type(read_annotation),
                    "write_type": descriptor_write_type(setter),
                    "getter": (
                        None
                        if getter is None
                        else callable_provenance(getter, serializer)
                    ),
                    "setter": (
                        None
                        if setter is None
                        else callable_provenance(setter, serializer)
                    ),
                    "data_descriptor": data_descriptor,
                }
            )
            continue
        static_values[name] = serializer.value(raw_value)

    metadata = []
    if annotations:
        metadata.append(
            {"key": "python.annotations", "value": serializer.value(annotations)}
        )
    if dataclasses.is_dataclass(class_value):
        metadata.append(
            {
                "key": "python.dataclass_fields",
                "value": serializer.value(
                    {
                        field.name: {
                            "init": field.init,
                            "repr": field.repr,
                            "compare": field.compare,
                            "kw_only": field.kw_only,
                        }
                        for field in dataclasses.fields(class_value)
                    }
                ),
            }
        )
    if django_structure is not None:
        metadata.append(
            {
                "key": "python.django.model",
                "value": serializer.value(django_structure["metadata"]),
            }
        )

    constructor = class_value.__dict__.get("__init__")
    replacement = (
        callable_provenance(constructor, serializer)
        if callable(constructor) and source_span(constructor) is not None
        else None
    )
    slots = getattr(class_value, "__slots__", ())
    if isinstance(slots, str):
        slots = (slots,)
    return {
        "mro": [
            qualified_type_name(base)
            for base in inspect.getmro(class_value)
            if base is not object
        ],
        "bases": [serializer.value(base) for base in class_value.__bases__],
        "fields": fields,
        "descriptors": descriptors,
        "methods": methods,
        "static_values": static_values,
        "slots": list(slots),
        "metadata": metadata,
        "constructor": {
            "signature": function_signature(class_value, serializer),
            "replacement": replacement,
        },
        "initializers": [],
    }


def django_model_structure(
    class_value: type[object],
) -> dict[str, object] | None:
    """Return a dependency-free structural snapshot of Django model options."""
    options = class_value.__dict__.get("_meta")
    if options is None or not hasattr(options, "fields") or not hasattr(options, "managers"):
        return None
    model_fields = []
    for field in options.fields:
        try:
            python_type = field.python_type
        except (AttributeError, NotImplementedError):
            python_type = object
        model_fields.append(
            {
                "name": field.name,
                "ty": annotation_type(python_type),
                "default": None,
                "is_static": False,
                "is_private": False,
                "span": None,
            }
        )
    manager_names = sorted(manager.name for manager in options.managers)
    return {
        "fields": model_fields,
        "metadata": {
            "app_label": options.app_label,
            "model_name": options.model_name,
            "db_table": options.db_table,
            "field_names": [field["name"] for field in model_fields],
            "manager_names": manager_names,
        },
    }


def unwrap_method(value: object) -> tuple[object | None, str | None]:
    """Return the callable and binding mode for a class namespace value."""
    if isinstance(value, staticmethod):
        return value.__func__, "free"
    if isinstance(value, classmethod):
        return value.__func__, "class"
    if inspect.isfunction(value):
        return value, "instance"
    return None, None


def descriptor_write_type(setter: object | None) -> dict[str, object] | None:
    """Resolve a descriptor setter value annotation."""
    if setter is None:
        return None
    try:
        parameters = list(inspect.signature(setter).parameters.values())
    except (TypeError, ValueError):
        return named_type("builtins.object")
    if not parameters:
        return named_type("builtins.object")
    return annotation_type(parameters[-1].annotation)


def materialized_definition(
    module_name: str, name: str, value: object, serializer: Serializer
) -> dict[str, object]:
    """Build one manifest definition from a final module binding."""
    if inspect.isclass(value):
        source = source_provenance(module_name, name, value, serializer)
        binding_type = named_type(qualified_type_name(value))
        definition = {"kind": "class", **class_definition(value, serializer)}
    elif callable(value):
        source = source_provenance(module_name, name, value, serializer)
        function = function_definition(name, value, serializer)
        binding_type = {"kind": "function", "value": function["signature"]}
        definition = {"kind": "function", **function}
    else:
        source = {
            "module": module_name,
            "qualified_name": name,
            "span": {"file": "", "start": 0, "end": 0},
        }
        value_id = serializer.value(value)
        binding_type = serializer.nodes[value_id]["ty"]
        definition = {"kind": "value", "value": value_id}
    return {
        "original_name": source["qualified_name"].split(".")[-1],
        "binding_name": name,
        "source": source,
        "binding_type": binding_type,
        "definition": definition,
    }


def should_materialize(
    module_name: str, name: str, value: object, candidate_modules: set[str]
) -> bool:
    """Return whether a public module binding belongs in the static snapshot."""
    if name.startswith("_") or isinstance(value, types.ModuleType):
        return False
    owner = getattr(value, "__module__", module_name)
    if inspect.isclass(value) or callable(value):
        return (
            owner == module_name
            or owner in candidate_modules
            or hasattr(value, "__wrapped__")
        )
    return True


def module_record(
    module: types.ModuleType,
    serializer: Serializer,
    candidate_modules: set[str],
) -> dict[str, object]:
    """Materialize final public bindings for one imported module."""
    definitions = []
    globals_record: dict[str, int] = {}
    for name, value in sorted(vars(module).items()):
        if not should_materialize(module.__name__, name, value, candidate_modules):
            continue
        definitions.append(
            materialized_definition(module.__name__, name, value, serializer)
        )
        globals_record[name] = serializer.value(value)
    return {
        "path": str(pathlib.Path(module.__file__ or "").resolve()),
        "definitions": definitions,
        "globals": globals_record,
    }


def reject_operation(name: str) -> typing.NoReturn:
    """Reject one nondeterministic or externally observable operation."""
    raise SpecializationRejected(
        f"native specialization adapter required: forbidden definition-time operation '{name}'"
    )


def install_guards(allowed_environment: set[str], allowed_extensions: set[str]) -> None:
    """Install deterministic-operation and native-extension guards."""
    original_getenv = os.getenv

    def guarded_getenv(key: str, default: object = None) -> object:
        """Reject reads outside the explicit environment allowlist."""
        if key not in allowed_environment:
            reject_operation(f"environment:{key}")
        return original_getenv(key, default)

    os.getenv = guarded_getenv
    time.time = lambda: reject_operation("time.time")
    time.time_ns = lambda: reject_operation("time.time_ns")
    random.random = lambda: reject_operation("random.random")
    random.randrange = lambda *args, **kwargs: reject_operation("random.randrange")
    random.randint = lambda *args, **kwargs: reject_operation("random.randint")
    socket.socket = lambda *args, **kwargs: reject_operation("network")
    subprocess.Popen = lambda *args, **kwargs: reject_operation("subprocess")
    os.system = lambda *args, **kwargs: reject_operation("subprocess")

    def audit(event: str, arguments: tuple[object, ...]) -> None:
        """Reject dynamically loaded native extensions not on the allowlist."""
        if event != "import" or len(arguments) < 2:
            return
        filename = arguments[1]
        if not isinstance(filename, str):
            return
        if filename.endswith((".so", ".pyd", ".dylib")):
            identity = pathlib.Path(filename).name
            if identity not in allowed_extensions:
                reject_operation(f"native-extension:{identity}")

    sys.addaudithook(audit)


def load_modules(request: dict[str, object], events: list[dict[str, object]]) -> list[types.ModuleType]:
    """Import all candidates once in source-graph order."""
    project_root = pathlib.Path(typing.cast(str, request["project_root"])).resolve()
    sys.path.insert(0, str(project_root))
    modules = []
    output = EventStream(events, False)
    errors = EventStream(events, True)
    with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
        for record in typing.cast(list[dict[str, str]], request["modules"]):
            modules.append(importlib.import_module(record["name"]))
    return modules


def build_manifest(request: dict[str, object]) -> dict[str, object]:
    """Execute candidate imports and build the specialization manifest."""
    events: list[dict[str, object]] = []
    environment = typing.cast(dict[str, str], request["sandbox_policy"]["environment"])
    extensions = typing.cast(list[str], request["sandbox_policy"]["native_extensions"])
    install_guards(set(environment), set(extensions))
    modules = load_modules(request, events)
    serializer = Serializer(pathlib.Path(typing.cast(str, request["project_root"])))
    candidate_modules = {module.__name__ for module in modules}
    records = [
        module_record(module, serializer, candidate_modules) for module in modules
    ]
    return {
        "smelt_version": request["smelt_version"],
        "schema_version": request["schema_version"],
        "language": "python",
        "host_runtime_version": sys.version,
        "hashes": request["hashes"],
        "sandbox_policy": request["sandbox_policy"],
        "modules": records,
        "values": {"nodes": serializer.nodes},
        "effects": events,
        "required_adapters": list(serializer.adapters.values()),
    }


def main() -> int:
    """Read one request file and write one manifest."""
    if len(sys.argv) != 2:
        raise SpecializationRejected("expected one specialization request path")
    request_path = pathlib.Path(sys.argv[1])
    request = json.loads(request_path.read_text(encoding="utf-8"))
    manifest = build_manifest(request)
    json.dump(manifest, sys.stdout, sort_keys=True, separators=(",", ":"))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception:
        traceback.print_exc(file=sys.stderr)
        raise SystemExit(2)
