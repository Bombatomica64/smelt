//! `ModuleBuilder` construction: the builder's initial state and the items and
//! const literals carried over from modules lowered earlier.

use std::collections::{HashMap, HashSet};
use crate::HirCtx;
use smelt_hir::{FileId, Item, Span};
use super::super::{ConstLiteral, ModuleBuilder, SpecializationData};
use super::super::state::class_registry::ClassRegistry;
use super::super::state::const_registry::ConstRegistry;
use super::super::state::fn_ctx::FnCtx;
use super::super::state::function_registry::FunctionRegistry;
use super::super::state::import_scope::ImportScope;
use super::super::state::interface_registry::InterfaceRegistry;
use super::super::state::local_scope::LocalScope;
use super::super::state::lowering_mode::UnknownIndexAccess;
use super::super::state::type_scope::TypeScope;
use super::super::support::{const_literal_from_item, insert_visible_item, item_name};

impl<'ctx> ModuleBuilder<'ctx> {
    /// Create a new module builder.
    pub(in crate::lowering) fn new(
        file_id: FileId,
        path: String,
        source: String,
        ctx: &'ctx mut HirCtx,
        specialization: Option<SpecializationData>,
        excluded_modules: Vec<String>,
    ) -> Self {
        let (items, classes, interfaces) = Self::visible_items(ctx);
        let const_literals = Self::visible_const_literals(ctx);
        let enum_member_literals = ctx.enum_members.clone();
        let const_objects = ctx.object_consts.clone();
        let const_object_value_collections = ctx.object_value_collections.clone();
        let const_collections = ctx.const_collections.clone();
        let object_namespaces = ctx.object_namespaces.clone();
        let function_overloads = ctx.overloads.clone();
        let function_rests = ctx.function_rests.clone();
        let type_alias_fields = ctx.type_alias_fields.clone();
        let interface_extends = ctx.interface_extends.clone();
        let interface_index_values = ctx.interface_index_values.clone();
        let class_index_values = ctx.class_index_values.clone();
        let interface_call_signatures = ctx.interface_call_signatures.clone();
        let interface_construct_signatures = ctx.interface_construct_signatures.clone();
        let callable_fields = ctx.callable_fields.clone();
        let callable_object_aliases = ctx.callable_object_aliases.clone();
        let unknown_index_access = UnknownIndexAccess::for_module(
            Self::is_declaration_type_test_path(&path),
        );
        Self {
            file_id,
            path,
            source,
            ctx,
            scope: LocalScope::default(),
            module_globals: HashMap::new(),
            ambient_value_declarations: HashSet::new(),
            mutable_global_items: HashMap::new(),
            items,
            imports: ImportScope::default(),
            export_renames: HashMap::new(),
            conditional_bases: HashMap::new(),
            conditional_classes: HashMap::new(),
            forced_class_name: None,
            classes: ClassRegistry::new(classes, class_index_values),
            interfaces: InterfaceRegistry::new(
                interfaces,
                interface_extends,
                interface_index_values,
                interface_call_signatures,
                interface_construct_signatures,
            ),
            types: TypeScope::new(type_alias_fields, callable_fields, callable_object_aliases),
            current_class: None,
            fn_ctx: FnCtx::default(),
            array_view_receivers: Vec::new(),
            asymmetric_matchers_lowered: 0,
            presence_widened_bindings: HashMap::new(),
            defining_local_functions: Vec::new(),
            unknown_index_access,
            object_namespaces,
            consts: ConstRegistry::new(
                const_literals,
                enum_member_literals,
                const_objects,
                const_collections,
                const_object_value_collections,
            ),
            functions: FunctionRegistry::new(function_overloads, function_rests),
            specialization,
            pending_host_imports: Vec::new(),
            excluded_modules,
            ts_facts: None,
            int32_type_depth: 0,
            tuple_rest_arrows: HashMap::new(),
            type_param_owners: HashMap::new(),
            receiver_frames: Vec::new(),
            kept_receivers: Vec::new(),
            enum_reverse_names: HashMap::new(),
            source_facts: super::super::source_facts::SourceFacts::default(),
            asserted_call_return: None,
            erasure_anchor: Span::new(file_id, 0, 0),
        }
    }

    /// Install the module's facts and publish the member names it declares.
    pub(in crate::lowering) fn adopt_source_facts(&mut self, facts: super::super::source_facts::SourceFacts) {
        self.ctx.declared_member_names.extend(facts.declared_members().cloned());
        self.source_facts = facts;
    }

    /// Collect items already present in the shared crate for cross-module references.
    pub(in crate::lowering) fn visible_items(
        ctx: &HirCtx,
    ) -> (
        HashMap<String, smelt_hir::ItemId>,
        HashMap<String, smelt_hir::ItemId>,
        HashMap<String, smelt_hir::ItemId>,
    ) {
        let mut items = HashMap::new();
        let mut classes = HashMap::new();
        let mut interfaces = HashMap::new();
        for (idx, item) in ctx.krate.items.iter().enumerate() {
            let item_id = smelt_hir::ItemId(u32::try_from(idx).unwrap_or(u32::MAX));
            let Some(name) = item_name(&ctx.krate, item) else {
                continue;
            };
            if matches!(item, Item::Class(class) if ctx.local_class_expressions.contains(&class.name)) {
                continue;
            }
            insert_visible_item(
                &mut items,
                &mut classes,
                &mut interfaces,
                name,
                item_id,
                item,
            );
        }
        #[expect(
            clippy::iter_over_hash_type,
            reason = "keyed inserts of distinct aliases"
        )]
        for (alias, item_id) in &ctx.export_aliases {
            let Some(item) = ctx
                .krate
                .items
                .get(usize::try_from(item_id.0).unwrap_or(usize::MAX))
            else {
                continue;
            };
            insert_visible_item(
                &mut items,
                &mut classes,
                &mut interfaces,
                alias,
                *item_id,
                item,
            );
        }
        (items, classes, interfaces)
    }

    /// Collect literal constant items already present in the shared crate.
    pub(in crate::lowering) fn visible_const_literals(ctx: &HirCtx) -> HashMap<String, ConstLiteral> {
        let mut values = HashMap::new();
        for item in &ctx.krate.items {
            let Item::Const(const_item) = item else {
                continue;
            };
            let Some(name) = item_name(&ctx.krate, item) else {
                continue;
            };
            if let Some(value) = const_literal_from_item(&ctx.krate, const_item) {
                values.insert(name.to_owned(), value);
            }
        }
        #[expect(
            clippy::iter_over_hash_type,
            reason = "keyed inserts of distinct aliases"
        )]
        for (alias, item_id) in &ctx.export_aliases {
            let Some(Item::Const(const_item)) = ctx
                .krate
                .items
                .get(usize::try_from(item_id.0).unwrap_or(usize::MAX))
            else {
                continue;
            };
            if let Some(value) = const_literal_from_item(&ctx.krate, const_item) {
                values.insert(alias.to_owned(), value);
            }
        }
        values
    }
}
