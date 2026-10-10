//! Tests for definition-time name resolution (`crate::name_resolution`) and
//! the class lowering that matches its result against modeled stdlib names.
//!
//! The first group resolves expressions directly against collected bindings,
//! one binding form per test. The second group lowers whole modules and checks
//! the HIR each spelling of a modeled stdlib name produces: dotted, aliased
//! module, from-import, from-import alias, and a local that shadows the import.

use super::*;
use crate::name_resolution::{Denotation, EvalScope, ModuleBindings, StdlibMatch, StdlibName};
use ruff_python_ast::{Expr, Stmt, StmtClassDef};

// ---------------------------------------------------------------------------
// Resolver
// ---------------------------------------------------------------------------

/// Parse `source` and return its module body.
fn parse_body(source: &str) -> Result<Vec<Stmt>, String> {
    crate::parse_module(source, FileId(0))
        .map(|module| module.body.to_vec())
        .map_err(|errors| format!("parse failed: {errors:?}"))
}

/// Find the module-level class named `name`.
fn class_stmt<'a>(body: &'a [Stmt], name: &str) -> Result<&'a StmtClassDef, String> {
    body.iter()
        .find_map(|stmt| match stmt {
            Stmt::ClassDef(class) if class.name.as_str() == name => Some(class),
            _ => None,
        })
        .ok_or_else(|| format!("missing class {name}"))
}

/// The first base of the module-level class named `class_name`.
fn first_base<'a>(body: &'a [Stmt], class_name: &str) -> Result<&'a Expr, String> {
    class_stmt(body, class_name)?
        .arguments
        .as_deref()
        .and_then(|arguments| arguments.args.first())
        .ok_or_else(|| format!("class {class_name} has no base"))
}

/// Resolve the first base of class `class_name`, as the `class` statement
/// evaluates it.
fn base_candidates(source: &str, class_name: &str) -> Result<Vec<Denotation>, String> {
    let body = parse_body(source)?;
    let bindings = ModuleBindings::collect(&body);
    let class = class_stmt(&body, class_name)?;
    let base = ruff_python_ast::helpers::map_subscript(first_base(&body, class_name)?);
    let resolution = bindings
        .resolve(
            base,
            EvalScope::Module {
                at: class.range.start(),
            },
        )
        .ok_or("base is not a name or attribute chain")?;
    Ok(resolution.candidates().to_vec())
}

/// Shorthand for a qualified-name candidate.
fn qualified(name: &str) -> Denotation {
    Denotation::Qualified(name.to_owned())
}

/// Assert that the first base of class `C` in `source` resolves to `expected`.
fn assert_base(source: &str, expected: &[Denotation], label: &str) -> TestResult {
    ensure_eq(&base_candidates(source, "C")?, &expected.to_vec(), label)
}

#[test]
fn from_import_resolves_to_qualified_name() -> TestResult {
    assert_base(
        "from abc import ABC\nclass C(ABC): pass\n",
        &[qualified("abc.ABC")],
        "from-import",
    )
}

#[test]
fn from_import_alias_resolves_to_imported_name() -> TestResult {
    assert_base(
        "from abc import ABC as Base\nclass C(Base): pass\n",
        &[qualified("abc.ABC")],
        "from-import alias",
    )
}

#[test]
fn module_import_attribute_resolves_to_qualified_name() -> TestResult {
    assert_base(
        "import abc\nclass C(abc.ABC): pass\n",
        &[qualified("abc.ABC")],
        "dotted",
    )
}

#[test]
fn aliased_module_import_resolves_through_alias() -> TestResult {
    assert_base(
        "import typing as t\nT = t.TypeVar('T')\nclass C(t.Generic[T]): pass\n",
        &[qualified("typing.Generic")],
        "aliased module",
    )
}

#[test]
fn dotted_module_import_binds_top_level_package() -> TestResult {
    // `import a.b` binds `a`; `import a.b as x` binds `x` to `a.b`.
    assert_base(
        "import a.b.c\nclass C(a.b.c.Base): pass\n",
        &[qualified("a.b.c.Base")],
        "import a.b.c",
    )?;
    assert_base(
        "import a.b as x\nclass C(x.Base): pass\n",
        &[qualified("a.b.Base")],
        "import a.b as x",
    )
}

#[test]
fn unbound_name_resolves_to_builtins() -> TestResult {
    assert_base(
        "class C(object): pass\n",
        &[qualified("builtins.object")],
        "object",
    )?;
    // CPython raises NameError for an unimported `ABC`; it is not `abc.ABC`.
    assert_base(
        "class C(ABC): pass\n",
        &[qualified("builtins.ABC")],
        "unbound ABC",
    )
}

#[test]
fn local_rebinding_shadows_import() -> TestResult {
    for shadow in [
        "ABC = object",
        "class ABC: pass",
        "def ABC(): pass",
        "with open('f') as ABC: pass",
        "(ABC := object)",
        "type ABC = int",
    ] {
        let source = format!("from abc import ABC\n{shadow}\nclass C(ABC): pass\n");
        assert_base(&source, &[Denotation::Local], shadow)?;
    }
    Ok(())
}

#[test]
fn rebinding_after_class_statement_does_not_apply() -> TestResult {
    assert_base(
        "from abc import ABC\nclass C(ABC): pass\nABC = object\n",
        &[qualified("abc.ABC")],
        "later rebinding",
    )
}

#[test]
fn annotation_without_value_does_not_bind() -> TestResult {
    assert_base(
        "from abc import ABC\nABC: type\nclass C(ABC): pass\n",
        &[qualified("abc.ABC")],
        "bare annotation",
    )
}

#[test]
fn del_unbinds_to_builtins() -> TestResult {
    assert_base(
        "from abc import ABC\ndel ABC\nclass C(ABC): pass\n",
        &[qualified("builtins.ABC")],
        "del",
    )
}

#[test]
fn conditional_import_keeps_every_reaching_binding() -> TestResult {
    assert_base(
        "if flag:\n    from abc import ABC\nclass C(ABC): pass\n",
        &[qualified("abc.ABC"), qualified("builtins.ABC")],
        "if without else",
    )?;
    // Either the import or the handler's import ran, so `Protocol` cannot be
    // unbound here.
    assert_base(
        "try:\n    from typing import Protocol\nexcept ImportError:\n    from typing_extensions import Protocol\nclass C(Protocol): pass\n",
        &[
            qualified("typing.Protocol"),
            qualified("typing_extensions.Protocol"),
        ],
        "try/except fallback",
    )
}

#[test]
fn loop_target_binds_conditionally() -> TestResult {
    assert_base(
        "from abc import ABC\nfor ABC in [object]:\n    pass\nclass C(ABC): pass\n",
        &[Denotation::Local, qualified("abc.ABC")],
        "for target",
    )
}

#[test]
fn branches_that_all_bind_the_same_name_decide_it() -> TestResult {
    assert_base(
        "if flag:\n    from abc import ABC\nelse:\n    import abc\n    ABC = abc.ABC\n    from abc import ABC\nclass C(ABC): pass\n",
        &[qualified("abc.ABC")],
        "if/else",
    )?;
    assert_base(
        "try:\n    from abc import ABC\nexcept ImportError:\n    from abc import ABC\nclass C(ABC): pass\n",
        &[qualified("abc.ABC")],
        "try/except",
    )
}

#[test]
fn unconditional_binding_hides_earlier_conditional_ones() -> TestResult {
    assert_base(
        "if flag:\n    ABC = object\nfrom abc import ABC\nclass C(ABC): pass\n",
        &[qualified("abc.ABC")],
        "later import",
    )
}

#[test]
fn try_without_handler_binds_unconditionally() -> TestResult {
    assert_base(
        "try:\n    from abc import ABC\nfinally:\n    pass\nclass C(ABC): pass\n",
        &[qualified("abc.ABC")],
        "try/finally",
    )
}

#[test]
fn star_import_adds_a_possible_binding() -> TestResult {
    assert_base(
        "from abc import *\nclass C(ABC): pass\n",
        &[qualified("abc.ABC"), qualified("builtins.ABC")],
        "star import",
    )?;
    // An explicit import after the star import decides the name.
    assert_base(
        "from shapes import *\nfrom abc import ABC\nclass C(ABC): pass\n",
        &[qualified("abc.ABC")],
        "explicit after star",
    )
}

#[test]
fn comprehension_target_does_not_bind_but_walrus_does() -> TestResult {
    assert_base(
        "from abc import ABC\nxs = [ABC for ABC in [1]]\nclass C(ABC): pass\n",
        &[qualified("abc.ABC")],
        "comprehension target",
    )?;
    assert_base(
        "from abc import ABC\nxs = [(ABC := x) for x in [1]]\nclass C(ABC): pass\n",
        &[Denotation::Local, qualified("abc.ABC")],
        "walrus in comprehension",
    )
}

#[test]
fn except_and_match_captures_bind_conditionally() -> TestResult {
    // The handler binds `ABC`, then deletes it on exit.
    assert_base(
        "from abc import ABC\ntry:\n    pass\nexcept Exception as ABC:\n    pass\nclass C(ABC): pass\n",
        &[qualified("abc.ABC"), qualified("builtins.ABC")],
        "except as",
    )?;
    assert_base(
        "from abc import ABC\nmatch [1]:\n    case [ABC]:\n        pass\nclass C(ABC): pass\n",
        &[Denotation::Local, qualified("abc.ABC")],
        "match capture",
    )
}

#[test]
fn relative_import_never_resolves_to_stdlib() -> TestResult {
    assert_base(
        "from .abc import ABC\nclass C(ABC): pass\n",
        &[qualified(".abc.ABC")],
        "relative import",
    )
}

#[test]
fn class_body_binding_shadows_module_import_for_method_decorators() -> TestResult {
    let source = "from abc import ABC, abstractmethod\n\
                  class C(ABC):\n    \
                      @abstractmethod\n    \
                      def first(self) -> int: ...\n    \
                      abstractmethod = staticmethod\n    \
                      @abstractmethod\n    \
                      def second(self) -> int: ...\n";
    let body = parse_body(source)?;
    let bindings = ModuleBindings::collect(&body);
    let class = class_stmt(&body, "C")?;
    let decorator_of = |method: &str| -> Result<Vec<Denotation>, String> {
        let function = class
            .body
            .iter()
            .find_map(|stmt| match stmt {
                Stmt::FunctionDef(function) if function.name.as_str() == method => Some(function),
                _ => None,
            })
            .ok_or_else(|| format!("missing method {method}"))?;
        let decorator = function.decorator_list.first().ok_or("missing decorator")?;
        let scope = EvalScope::ClassBody {
            class_start: class.range.start(),
            at: function.range.start(),
        };
        Ok(bindings
            .resolve(&decorator.expression, scope)
            .ok_or("decorator is not a name")?
            .candidates()
            .to_vec())
    };
    ensure_eq(
        &decorator_of("first")?,
        &vec![qualified("abc.abstractmethod")],
        "before the class-body rebinding",
    )?;
    ensure_eq(
        &decorator_of("second")?,
        &vec![Denotation::Local],
        "after the class-body rebinding",
    )
}

#[test]
fn stdlib_match_requires_every_candidate_to_agree() -> TestResult {
    let resolve = |source: &str| -> Result<StdlibMatch, String> {
        let body = parse_body(source)?;
        let bindings = ModuleBindings::collect(&body);
        let class = class_stmt(&body, "C")?;
        let base = first_base(&body, "C")?;
        Ok(bindings
            .resolve(
                base,
                EvalScope::Module {
                    at: class.range.start(),
                },
            )
            .ok_or("base is not a name")?
            .stdlib())
    };
    ensure_eq(
        &resolve("import enum\nclass C(enum.IntEnum): pass\n")?,
        &StdlibMatch::Exact(StdlibName::IntEnum),
        "exact",
    )?;
    ensure_eq(
        &resolve("if flag:\n    from abc import ABC\nclass C(ABC): pass\n")?,
        &StdlibMatch::Ambiguous,
        "ambiguous",
    )?;
    ensure_eq(
        &resolve("from shapes import Shape\nclass C(Shape): pass\n")?,
        &StdlibMatch::NotStdlib,
        "not stdlib",
    )?;
    ensure_eq(
        &resolve("import x\nclass C(x.ABC): pass\n")?,
        &StdlibMatch::NotStdlib,
        "same last segment, other module",
    )
}

// ---------------------------------------------------------------------------
// Class lowering
// ---------------------------------------------------------------------------

/// Find the class item named `name` in the lowered crate.
fn class_named<'a>(ctx: &'a HirCtx, name: &str) -> Result<&'a smelt_hir::Class, String> {
    ctx.krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Class(class) if symbol(ctx, class.name).ok()? == name => Some(class),
            _ => None,
        })
        .ok_or_else(|| format!("missing class {name}"))
}

/// The source name of a class's base, if it has one.
fn base_name(ctx: &HirCtx, class: &smelt_hir::Class) -> Result<Option<String>, String> {
    class
        .base
        .map(|base| symbol(ctx, base).map(ToOwned::to_owned))
        .transpose()
}

/// Lower `source` and return the codes of its diagnostics.
fn error_codes(source: &str) -> Result<Vec<&'static str>, String> {
    let mut ctx = HirCtx::new();
    Ok(lower_errors(source, &mut ctx)?
        .iter()
        .map(|error| error.code)
        .collect())
}

/// Every import spelling of `module.name`, as `(imports, spelling)` pairs:
/// dotted, aliased module, from-import, and from-import alias.
fn spellings(module: &str, name: &str) -> [(String, String); 4] {
    [
        (format!("import {module}"), format!("{module}.{name}")),
        (format!("import {module} as m"), format!("m.{name}")),
        (format!("from {module} import {name}"), name.to_owned()),
        (format!("from {module} import {name} as Alias"), "Alias".to_owned()),
    ]
}

#[test]
fn dataclass_decorator_lowers_for_every_import_spelling() -> TestResult {
    for (import, spelled) in spellings("dataclasses", "dataclass") {
        for (decorator, frozen) in [
            (spelled.clone(), false),
            (format!("{spelled}(frozen=True)"), true),
        ] {
            let source = format!("{import}\n\n@{decorator}\nclass Point:\n    x: int\n    y: str\n");
            let mut ctx = HirCtx::new();
            lower_module(&source, &mut ctx)?;
            let class = class_named(&ctx, "Point")?;
            ensure_eq(
                &class.kind,
                &smelt_hir::ClassKind::DataclassLike { frozen },
                &source,
            )?;
            ensure(class.constructor.is_some(), "synthesized __init__")?;
            let field_types: Vec<Option<&Type>> = class
                .fields
                .iter()
                .map(|field| ctx.krate.types.get(field.ty))
                .collect();
            ensure_eq(
                &field_types,
                &vec![Some(&Type::Int), Some(&Type::String)],
                "dataclass field types",
            )?;
        }
    }
    Ok(())
}

#[test]
fn shadowed_or_foreign_dataclass_decorator_is_not_the_stdlib_one() -> TestResult {
    // Each decorator ends in `dataclass` but denotes something else, so the
    // class needs a specialization manifest exactly like any unknown decorator.
    for source in [
        "from dataclasses import dataclass\n\ndef dataclass(cls):\n    return cls\n\n@dataclass\nclass P:\n    x: int\n",
        "import attrs as x\n\n@x.dataclass\nclass P:\n    x: int\n",
        "@dataclass\nclass P:\n    x: int\n",
    ] {
        ensure_eq(
            &error_codes(source)?,
            &vec!["smelt::specialization-required"],
            source,
        )?;
    }
    Ok(())
}

#[test]
fn abc_base_marks_class_abstract_for_every_import_spelling() -> TestResult {
    for (import, spelled) in spellings("abc", "ABC") {
        let source = format!("{import}\n\nclass Shape({spelled}):\n    pass\n");
        let mut ctx = HirCtx::new();
        lower_module(&source, &mut ctx)?;
        let class = class_named(&ctx, "Shape")?;
        ensure_eq(&class.kind, &smelt_hir::ClassKind::Abstract, &source)?;
        ensure_eq(&base_name(&ctx, class)?, &None, "ABC adds no base")?;
    }
    Ok(())
}

#[test]
fn shadowed_abc_base_is_an_ordinary_base_class() -> TestResult {
    let source = py!(r"
from abc import ABC

class ABC:
    pass

class Shape(ABC):
    pass
");
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    let class = class_named(&ctx, "Shape")?;
    ensure_eq(&class.kind, &smelt_hir::ClassKind::Plain, "not abstract")?;
    ensure_eq(
        &base_name(&ctx, class)?,
        &Some("ABC".to_owned()),
        "inherits the local ABC",
    )
}

#[test]
fn abcmeta_metaclass_marks_class_abstract_for_every_import_spelling() -> TestResult {
    for (import, spelled) in spellings("abc", "ABCMeta") {
        let source = format!("{import}\n\nclass Shape(metaclass={spelled}):\n    pass\n");
        let mut ctx = HirCtx::new();
        lower_module(&source, &mut ctx)?;
        let class = class_named(&ctx, "Shape")?;
        ensure_eq(&class.kind, &smelt_hir::ClassKind::Abstract, &source)?;
    }
    Ok(())
}

#[test]
fn shadowed_abcmeta_is_a_custom_metaclass() -> TestResult {
    let source = py!(r"
from abc import ABCMeta

class ABCMeta(type):
    pass

class Shape(metaclass=ABCMeta):
    pass
");
    ensure_eq(
        &error_codes(source)?,
        &vec!["smelt::specialization-required"],
        "local metaclass needs specialization",
    )
}

#[test]
fn abstractmethod_decorator_lowers_for_every_import_spelling() -> TestResult {
    for (import, spelled) in spellings("abc", "abstractmethod") {
        let source = format!(
            "{import}\nimport abc as abc_module\n\nclass Shape(abc_module.ABC):\n    @{spelled}\n    def area(self) -> float:\n        pass\n"
        );
        let mut ctx = HirCtx::new();
        lower_module(&source, &mut ctx)?;
        let class = class_named(&ctx, "Shape")?;
        let abstract_names: Vec<&str> = class
            .abstract_methods
            .iter()
            .map(|method| symbol(&ctx, method.name))
            .collect::<Result<_, _>>()?;
        ensure_eq(&abstract_names, &vec!["area"], &source)?;
        ensure(class.methods.is_empty(), "abstract method has no body item")?;
    }
    Ok(())
}

#[test]
fn foreign_or_class_shadowed_abstractmethod_is_not_abstract() -> TestResult {
    // `helpers.abstractmethod` only shares the last segment; the class-body
    // `abstractmethod` shadows the import for the methods after it. Neither
    // marks the method abstract, so both reach the unsupported-decorator check.
    for source in [
        "import helpers\n\nclass Shape:\n    @helpers.abstractmethod\n    def area(self) -> float:\n        return 0.0\n",
        "from abc import abstractmethod\n\nclass Shape:\n    def abstractmethod(self) -> int:\n        return 0\n\n    @abstractmethod\n    def area(self) -> float:\n        return 0.0\n",
    ] {
        let mut ctx = HirCtx::new();
        let errors = lower_errors(source, &mut ctx)?;
        let error = first_error(&errors)?;
        ensure_eq(&error.code, &"smelt::unsupported-py", source)?;
        ensure(
            error.message.contains("decorator '@abstractmethod' is not supported"),
            format!("unexpected message: {}", error.message),
        )?;
    }
    Ok(())
}

#[test]
fn generic_marker_lowers_type_params_for_every_import_spelling() -> TestResult {
    for (import, spelled) in spellings("typing", "Generic") {
        let source = format!(
            "{import}\nfrom typing import TypeVar\n\nT = TypeVar(\"T\")\n\nclass Box({spelled}[T]):\n    value: T\n\n    def __init__(self, value: T) -> None:\n        self.value = value\n"
        );
        let mut ctx = HirCtx::new();
        lower_module(&source, &mut ctx)?;
        let class = class_named(&ctx, "Box")?;
        let params: Vec<&str> = class
            .type_params
            .iter()
            .map(|param| symbol(&ctx, param.name))
            .collect::<Result<_, _>>()?;
        ensure_eq(&params, &vec!["T"], &source)?;
        ensure_eq(&base_name(&ctx, class)?, &None, "Generic adds no base")?;
        let field = class.fields.first().ok_or("missing field")?;
        let field_ty = ctx.krate.types.get(field.ty).ok_or("missing field type")?;
        ensure(
            matches!(field_ty, Type::Class { name, args }
                if args.is_empty() && symbol(&ctx, *name).ok() == Some("T")),
            format!("field should keep the type parameter, got {field_ty:?}"),
        )?;
    }
    Ok(())
}

#[test]
fn int_enum_base_lowers_members_for_every_import_spelling() -> TestResult {
    for (import, spelled) in spellings("enum", "IntEnum") {
        let source = format!("{import}\n\nclass Color({spelled}):\n    RED = 1\n    GREEN = 2\n");
        let mut ctx = HirCtx::new();
        lower_module(&source, &mut ctx)?;
        let class = class_named(&ctx, "Color")?;
        ensure_eq(
            &base_name(&ctx, class)?,
            &Some("IntEnum".to_owned()),
            "IntEnum base is recorded under its own name",
        )?;
        let members = ctx.enum_members.get("Color").ok_or("missing enum members")?;
        ensure_eq(&members.get("RED"), &Some(&1), &source)?;
        ensure_eq(&members.get("GREEN"), &Some(&2), &source)?;
        ensure(class.static_fields.is_empty(), "members are not class variables")?;
    }
    Ok(())
}

#[test]
fn shadowed_int_enum_base_is_an_ordinary_class() -> TestResult {
    let source = py!(r"
from enum import IntEnum

class IntEnum:
    pass

class Color(IntEnum):
    RED = 1
");
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    let class = class_named(&ctx, "Color")?;
    ensure(
        !ctx.enum_members.contains_key("Color"),
        "a local IntEnum is not enum.IntEnum",
    )?;
    ensure_eq(&class.static_fields.len(), &1, "RED is a class variable")
}

#[test]
fn protocol_marker_adds_no_base_for_every_import_spelling() -> TestResult {
    for (import, spelled) in spellings("typing", "Protocol") {
        let source = format!(
            "{import}\n\nclass HasArea({spelled}):\n    def area(self) -> float:\n        pass\n"
        );
        let mut ctx = HirCtx::new();
        lower_module(&source, &mut ctx)?;
        ensure_eq(&base_name(&ctx, class_named(&ctx, "HasArea")?)?, &None, &source)?;
    }
    Ok(())
}

#[test]
fn shadowed_protocol_base_is_an_ordinary_base_class() -> TestResult {
    let source = py!(r"
from typing import Protocol

class Protocol:
    pass

class HasArea(Protocol):
    pass
");
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    ensure_eq(
        &base_name(&ctx, class_named(&ctx, "HasArea")?)?,
        &Some("Protocol".to_owned()),
        "inherits the local Protocol",
    )
}

#[test]
fn object_base_adds_no_base_for_every_spelling() -> TestResult {
    for source in [
        "class C(object):\n    pass\n",
        "import builtins\n\nclass C(builtins.object):\n    pass\n",
    ] {
        let mut ctx = HirCtx::new();
        lower_module(source, &mut ctx)?;
        ensure_eq(&base_name(&ctx, class_named(&ctx, "C")?)?, &None, source)?;
    }
    Ok(())
}

#[test]
fn conditionally_bound_stdlib_name_is_reported_as_ambiguous() -> TestResult {
    let source = py!(r"
try:
    from abc import ABC
except ImportError:
    ABC = object

class Shape(ABC):
    pass
");
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::ambiguous-binding", "error code")?;
    ensure(
        error.message.contains("'abc.ABC'") && error.message.contains("a definition in this module"),
        format!("message should list the candidates: {}", error.message),
    )?;
    ensure_eq(
        &error_codes("from abc import *\n\nclass Shape(ABC):\n    pass\n")?,
        &vec!["smelt::ambiguous-binding"],
        "star import",
    )
}

#[test]
fn conditionally_bound_dataclass_needs_specialization() -> TestResult {
    let source = py!(r"
if flag:
    from dataclasses import dataclass

@dataclass
class Point:
    x: int
");
    ensure_eq(
        &error_codes(source)?,
        &vec!["smelt::specialization-required"],
        "decided only by running the module",
    )
}
