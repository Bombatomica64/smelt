//! Type and item reference formatting helpers.

use crate::ids::{ExprId, ItemId, LocalId, TypeId};
use crate::item::Item;
use crate::krate::Crate;
use crate::ty::Type;

/// Formats an item reference as text.
pub(super) fn item_ref(krate: &Crate, item: ItemId) -> String {
    let Some(item_value) = krate.items.get(item.0 as usize) else {
        return format!("item{}", item.0);
    };

    let name = match item_value {
        Item::Function(function) => krate.symbols.get(function.name),
        Item::Class(class) => krate.symbols.get(class.name),
        Item::Interface(interface) => krate.symbols.get(interface.name),
        Item::TypeAlias(alias) => krate.symbols.get(alias.name),
        Item::Const(item) => krate.symbols.get(item.name),
    }
    .unwrap_or("<unknown>");

    format!("@{}({})", item.0, name)
}

/// Formats an item as text.
pub(super) fn item_text(krate: &Crate, item: ItemId) -> String {
    let Some(item_idx) = usize::try_from(item.0).ok() else {
        return format!("invalid-item-{}", item.0);
    };
    let Some(item_value) = krate.items.get(item_idx) else {
        return format!("missing-item-{}", item.0);
    };
    match item_value {
        Item::Function(function) => {
            let name = krate.symbols.get(function.name).unwrap_or("<unknown>");
            format!("fn {name} owner {:?}", function.owner)
        }
        Item::Class(class) => class_item_text(krate, class),
        Item::Interface(interface) => interface_item_text(krate, interface),
        Item::TypeAlias(alias) => {
            let name = krate.symbols.get(alias.name).unwrap_or("<unknown>");
            format!("type {name} = {}", type_ref(krate, alias.ty))
        }
        Item::Const(const_item) => {
            let name = krate.symbols.get(const_item.name).unwrap_or("<unknown>");
            format!("const {name}: {}", type_ref(krate, const_item.ty))
        }
    }
}

/// Formats a list of fields as text.
fn fields_text(krate: &Crate, fields: &[crate::item::Field]) -> String {
    fields
        .iter()
        .map(|field| {
            format!(
                "{:?} {}{}: {}",
                field.visibility,
                krate.symbols.get(field.name).unwrap_or("<unknown>"),
                if field.optional { "?" } else { "" },
                type_ref(krate, field.ty)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Formats a class item as text.
fn class_item_text(krate: &Crate, class: &crate::item::Class) -> String {
    let name = krate.symbols.get(class.name).unwrap_or("<unknown>");
    let fields = fields_text(krate, &class.fields);
    let implements = class
        .implements
        .iter()
        .map(|sym| krate.symbols.get(*sym).unwrap_or("<unknown>").to_owned())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "class {name} fields [{fields}] constructor {:?} methods {:?} implements [{implements}]",
        class.constructor, class.methods
    )
}

/// Formats an interface item as text.
fn interface_item_text(krate: &Crate, interface: &crate::item::Interface) -> String {
    let name = krate.symbols.get(interface.name).unwrap_or("<unknown>");
    let type_params = type_params_text(krate, &interface.type_params);
    let fields = fields_text(krate, &interface.fields);
    let methods = interface
        .methods
        .iter()
        .map(|method| method_sig_text(krate, method))
        .collect::<Vec<_>>()
        .join(", ");
    format!("interface {name}{type_params} fields [{fields}] methods [{methods}]")
}

/// Formats generic type parameters.
fn type_params_text(krate: &Crate, params: &[crate::item::TypeParamDef]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let params = params
        .iter()
        .map(|param| {
            let name = krate.symbols.get(param.name).unwrap_or("<unknown>");
            let constraint = param
                .constraint
                .map(|ty| format!(" extends {}", type_ref(krate, ty)))
                .unwrap_or_default();
            let default = param
                .default
                .map(|ty| format!(" = {}", type_ref(krate, ty)))
                .unwrap_or_default();
            format!("{name}{constraint}{default}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("<{params}>")
}

/// Formats a method signature as text.
fn method_sig_text(krate: &Crate, method: &crate::item::MethodSig) -> String {
    let params = method
        .params
        .iter()
        .map(|param| {
            format!(
                "{}: {}",
                krate.symbols.get(param.name).unwrap_or("<unknown>"),
                type_ref(krate, param.ty)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{:?} {}({params}) -> {}",
        method.visibility,
        krate.symbols.get(method.name).unwrap_or("<unknown>"),
        type_ref(krate, method.return_ty)
    )
}

/// Formats a type reference as text.
pub(super) fn type_ref(krate: &Crate, ty: TypeId) -> String {
    let Some(ty_value) = krate.types.get(ty) else {
        return format!("t{}", ty.0);
    };
    type_text(krate, ty_value)
}

/// Formats a type as text.
pub(super) fn type_text(krate: &Crate, ty: &Type) -> String {
    match ty {
        Type::Bool => "Bool".to_owned(),
        Type::Int => "Int".to_owned(),
        Type::Float => "Float".to_owned(),
        Type::String => "String".to_owned(),
        Type::Unknown => "Unknown".to_owned(),
        Type::TypeParam { name } => krate.symbols.get(*name).unwrap_or("<unknown>").to_owned(),
        Type::None => "None".to_owned(),
        Type::List(item) => format!("List<{}>", type_ref(krate, *item)),
        Type::Set(item) => format!("Set<{}>", type_ref(krate, *item)),
        Type::Dict(key, value) => {
            format!(
                "Dict<{}, {}>",
                type_ref(krate, *key),
                type_ref(krate, *value)
            )
        }
        Type::Tuple(items) => {
            let items = items
                .iter()
                .map(|item| type_ref(krate, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({items})")
        }
        Type::Optional(item) => format!("Optional<{}>", type_ref(krate, *item)),
        Type::Union(items) => items
            .iter()
            .map(|item| type_ref(krate, *item))
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Class { name, args } => {
            let name = krate.symbols.get(*name).unwrap_or("<unknown>");
            if args.is_empty() {
                name.to_owned()
            } else {
                let args = args
                    .iter()
                    .map(|arg| type_ref(krate, *arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{name}<{args}>")
            }
        }
        Type::Function(function) => {
            let params = function
                .params
                .iter()
                .map(|param| type_ref(krate, *param))
                .collect::<Vec<_>>()
                .join(", ");
            let async_prefix = if function.is_async { "async " } else { "" };
            format!(
                "{async_prefix}fn({params}) -> {}",
                type_ref(krate, function.return_ty)
            )
        }
        Type::Future(item) => format!("Future<{}>", type_ref(krate, *item)),
    }
}

/// Formats a local variable reference as text.
pub(super) fn local_ref(local: LocalId) -> String {
    format!("%{}", local.0)
}

/// Formats an expression reference as text.
pub(super) fn expr_ref(expr: ExprId) -> String {
    format!("#{}", expr.0)
}

/// Formats an optional expression reference as text.
pub(super) fn optional_expr_ref(expr: Option<ExprId>) -> String {
    expr.map_or_else(|| "_".to_owned(), expr_ref)
}
