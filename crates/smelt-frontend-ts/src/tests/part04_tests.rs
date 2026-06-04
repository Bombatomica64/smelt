use super::*;

#[test]
fn lowers_imported_unknown_calls_inside_unannotated_block_arrow() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { importDefault } from "@strapi/utils";

const loadJsFile = (file: string) => {
  try {
    const jsModule = importDefault(file);
    return jsModule;
  } catch (error) {
    return {};
  }
};
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn infers_async_arrow_const_return_from_erased_contextual_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Middleware = unknown;

async function load(): Promise<string> {
  return "ok";
}

export const middleware: Middleware = async (ctx, next) => {
  try {
    return await next();
  } catch (error) {
    return await load();
  }
};
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_await_expression_call_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function assoc(key: string, value: string, target: unknown): unknown {
  return target;
}

async function getDefaultLocale(): Promise<string> {
  return "en";
}

export const addLocale = async (params: unknown) => {
  return assoc("locale", await getDefaultLocale(), params);
};
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_lodash_predicate_factories_as_array_callbacks() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import _ from "lodash";
import { has } from "lodash/fp";

type Item = { id?: string | null };

function keep(items: Item[]): Item[] {
  return items.filter(has("id")).filter(_.negate(_.isNil));
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_lodash_omit_factory_as_array_map_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { omit } from "lodash/fp";

function stripId(items: Record<string, unknown>[]): Record<string, unknown>[] {
  return items.map(omit("id"));
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_curried_emit_event_factory_as_for_each_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function emitEvent(name: string): (entry: unknown) => void {
  return (_entry) => {};
}

function deleted(entries: unknown[]) {
  entries.forEach(emitEvent("entry.delete"));
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_async_pipe_factory_as_array_map_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
declare const async: {
  pipe: (...fns: unknown[]) => (value: unknown) => unknown;
};

function clone(entries: unknown[]) {
  return entries.map(async.pipe((value: unknown) => value));
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_forward_reference_to_nested_function_declaration() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function repository() {
  async function create(params: Record<string, unknown>) {
    return publish({ ...params, documentId: "doc" }).then((doc) => doc.entries[0]);
  }

  async function publish(opts = {} as any) {
    return { entries: [opts] };
  }

  return { create, publish };
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_forward_reference_to_nested_function_in_arrow_const() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export const repository = () => {
  async function create(params: Record<string, unknown>) {
    return publish({ ...params, documentId: "doc" }).then((doc) => doc.entries[0]);
  }

  async function publish(opts = {} as any) {
    return { entries: [opts] };
  }

  return { create, publish };
};
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn resolves_namespace_qualified_type_aliases() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
declare namespace Custom {
  export type Id = string;
}

function accept(id: Custom.Id): Custom.Id {
  return id;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = module
        .items
        .iter()
        .find_map(|item| match &ctx.krate.items[item.0 as usize] {
            Item::Function(function) if ctx.krate.symbols.get(function.name) == Some("accept") => {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "expected accept function".to_owned())?;
    let ty = ctx
        .krate
        .types
        .get(function.params[0].ty)
        .ok_or_else(|| "expected parameter type".to_owned())?;
    ensure!(
        matches!(ty, Type::String),
        "expected Custom.Id to resolve to string, got {ty:?}"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn resolves_imported_namespace_alias_members() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
export namespace Types {
  export type Id = string;
}
"#),
        "types.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import type { Types as LocalTypes } from "./types";

const seen = new Map<string, number>();

export function hasSeen(id: LocalTypes.Id): boolean {
  return seen.has(id);
}
"#),
        "consumer.ts",
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn resolves_namespace_reexport_alias_members() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
export type Id = string;
"#),
        "ids.ts",
        &mut ctx,
    )?;
    lower_path_ok(
        ts!(r#"
export type * as Ids from "./ids";
"#),
        "index.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import type { Ids } from "./index";

const seen = new Map<string, number>();

export function hasSeen(id: Ids.Id): boolean {
  return seen.has(id);
}
"#),
        "consumer.ts",
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_extract_utility_to_extracted_surface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type RegistryKey<TRegistry extends object, TIndexType extends string> = Extract<
  keyof TRegistry,
  TIndexType
>;

type Id = RegistryKey<Record<string, unknown>, string>;

const seen = new Map<string, number>();

export function hasSeen(id: Id): boolean {
  return seen.has(id);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_static_string_padding_utility_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import * as stringUtils from "string-utils";

export function pad(value: string): string {
  return stringUtils.padEnd(value.slice(1), 3, "0");
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(
                expr.kind,
                ExprKind::StringPad {
                    op: StringPadOp::End,
                    ..
                }
            )),
        "expected static padEnd utility to lower to StringPad"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_forward_module_global_callable_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Env = typeof envFn;

function envFn(key: string, defaultValue?: string): string | undefined {
  return defaultValue;
}

export function oneOf(key: string, defaultValue?: string): string | undefined {
  return env(key, defaultValue);
}

const env: Env = Object.assign(envFn, {});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn resolves_type_namespace_import_members() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
export type Id = string;
"#),
        "ids.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import type * as Ids from "./ids";

const seen = new Map<string, number>();

export function hasSeen(id: Ids.Id): boolean {
  return seen.has(id);
}
"#),
        "consumer.ts",
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn resolves_nested_namespace_import_and_reexport_aliases() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
export type Keys<TRegistry extends object, TIndexType extends string> = Extract<
  keyof TRegistry,
  TIndexType
>;
"#),
        "registry.ts",
        &mut ctx,
    )?;
    lower_path_ok(
        ts!(r#"
export type * as Registry from "./registry";
"#),
        "internal.ts",
        &mut ctx,
    )?;
    lower_path_ok(
        ts!(r#"
import type * as Internal from "./internal";

export type ContentType = Internal.Registry.Keys<Record<string, unknown>, string>;
"#),
        "uid.ts",
        &mut ctx,
    )?;
    lower_path_ok(
        ts!(r#"
export type * as UID from "./uid";
"#),
        "index.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import type { UID } from "./index";

const seen = new Map<string, number>();

export function hasSeen(id: UID.ContentType): boolean {
  return seen.has(id);
}
"#),
        "consumer.ts",
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_cast_for_erased_id_field() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Entry<T> = T;

function key<T>(entry: Entry<T>): string {
  return String(entry.id);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_id_field_on_erased_generic_input() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Input<T> = T;

function update<T>(value: Input<T>) {
  if ("id" in value && typeof value.id !== "undefined") {
    return { id: value.id };
  }
  return null;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_assign_status_onto_error_object() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function mark(): Error {
  const err = new Error("bad");
  Object.assign(err, { status: 400 });
  return err;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_callback_member_assignment_as_erased_side_effect() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Relation = { id: number; position?: { before?: number } };

function build(ids: number[], position: { before?: number }): Relation[] {
  return ids.map((id) => {
    const relation = { id } as Relation;
    if (position.before) {
      relation.position = position;
    }
    return relation;
  });
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_callback_object_literal_with_computed_key() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function update(relations: Record<string, unknown>[], column: string, newId: number) {
  return relations.map((relation) => {
    return { ...relation, [column]: newId };
  });
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_callback_array_literal_with_mixed_item_types() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function pairs(entries: Record<string, unknown>[]) {
  return entries.map((entry: any) => [`${entry.document_id}_${entry.locale}`, entry.id]);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_callback_dynamic_access_with_erased_key() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function pick(rows: Record<string, unknown>[], column: unknown) {
  return rows.map((row) => row[column as string]);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_entries_reduce_tuple_member_access() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Attribute = { type: string; target?: string };

function populate(model: { attributes: Record<string, Attribute> }) {
  const attributes = Object.entries(model.attributes);
  return attributes.reduce((acc: any, [attributeName, attribute]) => {
    switch (attribute.type) {
      case "relation":
        acc[attributeName] = { select: attribute.target };
        break;
      default:
        break;
    }
    return acc;
  }, {});
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_reduce_on_optional_array_fallback_tuple() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Attribute = { components?: string[] };

function collect(attribute: Attribute) {
  return (attribute.components || []).reduce((acc: any, componentUID: string) => {
    acc[componentUID] = true;
    return acc;
  }, {});
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_includes_with_union_argument_against_literal_list() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const EVENTS = {
  ENTRY_DELETE: "entry.delete",
  ENTRY_UNPUBLISH: "entry.unpublish",
};

type EventName = "entry.create" | "entry.delete" | "entry.unpublish";

function shouldPopulate(eventName: EventName): boolean {
  return ![EVENTS.ENTRY_DELETE, EVENTS.ENTRY_UNPUBLISH].includes(eventName);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_node_process_env_cwd_and_require_surface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import path from "path";

const envPath = process.env.ENV_PATH;
const mode = process.env.NODE_ENV || "development";
const configDir = path.resolve(process.cwd(), "config");
const interactive = process.stdout.isTTY;
const pkg = require(path.resolve(configDir, "package.json"));
const resolved = require.resolve("pkg");
"#),
        &mut ctx,
    )?;
    let lowered_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, lowered_module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictLit(_))),
        "expected require(...) to lower as an opaque record",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unknown_namespace_import_member_calls_as_opaque() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import * as z from "zod/v4";

const schema = z.record({}, {});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unknown_namespace_import_member_reads_as_opaque() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import * as z from "zod/v4";

const registry = z.globalRegistry;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_exported_const_assertion_array_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export const BOOLEAN_LITERAL_VALUES = ["t", "1", "true", "f", "0", "false"] as const;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_void_block_arrow_const_without_return_annotation() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export const safeGlobalRegistrySet = (value: string) => {
  console.log(value);
};
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_affix_on_opaque_class_like_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type SchemaUid = string & { readonly __brand: unique symbol };

function builtin(id: SchemaUid): boolean {
  return id.startsWith("plugin::") || id.startsWith("admin");
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_keys_on_erased_optional_chain_fallback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const schema: unknown = {};
const fieldCount = Object.keys((schema as any)?._def?.shape || {}).length || 0;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_instanceof_against_namespace_import_constructor_as_opaque_false() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import * as z from "zod/v4";

function maybeArray(schema: unknown): boolean {
  return schema instanceof z.ZodArray;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_url_constructor_with_unknown_string_source() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const adminAbsoluteUrl: unknown = {};
const sameOrigin = new URL(adminAbsoluteUrl).origin === new URL(adminAbsoluteUrl).origin;
"#),
        &mut ctx,
    )?;
    let lowered_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, lowered_module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::UrlField { .. })),
        "expected URL fields to lower with an unknown string-compatible source",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_primitive_object_spread_sources_as_empty_records() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const enabled = false;
const value = { id: "1", ...enabled };
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let _body = module_body(&ctx, module)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_temporal_global_as_opaque_unknown_surface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function zone(): unknown {
  return Temporal.Instant.fromEpochMilliseconds(0).toZonedDateTimeISO(
    Temporal.Now.timeZoneId(),
  );
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn skips_vitest_mock_dynamic_import_registration() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { vi } from "vitest";

vi.mock(import("./index.ts"), () => ({ add: 1 }));

await import("./test.ts");
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_shift_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
let values: string[] = ["a", "b"];
values.shift();
const item = values.shift();
"#),
        &mut ctx,
    )?;
    let module_ref = module(&ctx, module_id)?;
    let body = module_body(&ctx, module_ref)?;
    let shifts = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListShift { .. }))
        .count();
    ensure_eq!(shifts, 2);
    Ok(())
}

#[test]
fn rejects_unsupported_array_push_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let wrong_type = lowering_errors(
        ts!(r#"
let values: number[] = [1, 2];
values.push("x");
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&wrong_type, "argument must match")?;

    let mut ctx = HirCtx::new();
    let too_many = lowering_errors(
        ts!(r#"
let values: number[] = [1, 2];
values.push(3, 4);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&too_many, "exactly one item argument")
}

#[test]
fn rejects_unsupported_array_unshift_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let wrong_type = lowering_errors(
        ts!(r#"
let values: number[] = [1, 2];
values.unshift("x");
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&wrong_type, "arguments must match")?;

    let mut ctx = HirCtx::new();
    let non_local = lowering_errors(
        ts!(r#"
function values(): number[] { return [1, 2]; }
values().unshift(0);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&non_local, "local array receiver")
}

#[test]
fn rejects_unsupported_array_pop_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
let values: number[] = [1, 2];
values.pop(0);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "requires no arguments")
}

#[test]
fn rejects_unsupported_array_shift_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
let values: number[] = [1, 2];
values.shift(0);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "requires no arguments")
}

#[test]
fn rejects_unsupported_slice_argument_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let too_many = lowering_errors(
        ts!(r#"
const values: number[] = [1, 2, 3];
const bad = values.slice(0, 1, 2);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&too_many, "omitted, start, and end arguments")
}

#[test]
fn lowers_array_is_array_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2, 3];
const yes = Array.isArray(values);
const no = Array.isArray(1);
"#),
        &mut ctx,
    )?;
    let module_ref = module(&ctx, module_id)?;
    let body = module_body(&ctx, module_ref)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Bool(true))))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Bool(false))))
    );
    Ok(())
}

#[test]
fn lowers_math_sqrt_pow_sign() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = 4;
const root = Math.sqrt(value);
const cubeRoot = Math.cbrt(value);
const signed = Math.sign(value);
const sine = Math.sin(value);
const cosine = Math.cos(value);
const tangent = Math.tan(value);
const arcsine = Math.asin(value);
const arccosine = Math.acos(value);
const arctangent = Math.atan(value);
const arctangentTwo = Math.atan2(value, 2);
const logged = Math.log(value);
const logTen = Math.log10(value);
const logTwo = Math.log2(value);
const exponent = Math.exp(value);
const raised = Math.pow(value, 2);
const raisedOperator = 1000 ** value;
const distance = Math.hypot(value, 3);
const sample = Math.random();
"#),
        &mut ctx,
    )?;
    let lowered_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, lowered_module)?;

    for expected in [
        NumericUnaryFuncOp::Sqrt,
        NumericUnaryFuncOp::Cbrt,
        NumericUnaryFuncOp::Sign,
        NumericUnaryFuncOp::Sin,
        NumericUnaryFuncOp::Cos,
        NumericUnaryFuncOp::Tan,
        NumericUnaryFuncOp::Asin,
        NumericUnaryFuncOp::Acos,
        NumericUnaryFuncOp::Atan,
        NumericUnaryFuncOp::Log,
        NumericUnaryFuncOp::Log10,
        NumericUnaryFuncOp::Log2,
        NumericUnaryFuncOp::Exp,
    ] {
        ensure!(body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::NumericUnaryFunc { op, .. } if op == expected)
        ));
    }
    ensure!(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::NumericPow { .. }))
            .count()
            >= 2
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericAtan2 { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericHypot { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericRandom))
    );
    Ok(())
}

#[test]
fn lowers_number_predicate_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = 4;
const finite = Number.isFinite(value);
const integer = Number.isInteger(value);
const nan = Number.isNaN(value);
const globalNan = isNaN(value);
const missing = undefined;
"#),
        &mut ctx,
    )?;
    let lowered_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, lowered_module)?;

    for expected in [
        NumericPredicateOp::IsFinite,
        NumericPredicateOp::IsInteger,
        NumericPredicateOp::IsNaN,
    ] {
        ensure!(body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::NumericPredicate { op, .. } if op == expected)
        ));
    }
    ensure_eq!(
        body.exprs
            .iter()
            .filter(|expr| matches!(
                expr.kind,
                ExprKind::NumericPredicate {
                    op: NumericPredicateOp::IsNaN,
                    ..
                }
            ))
            .count(),
        2
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::None)))
    );
    Ok(())
}

#[test]
fn lowers_object_projection_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const mapping: Record<string, number> = { a: 1, b: 2 };
const keys = Object.keys(mapping);
const values = Object.values(mapping);
const entries = Object.entries(mapping);
const rebuilt = Object.fromEntries([["a", 1], ["b", 2]]);
const remapped = Object.fromEntries(Object.entries(mapping).map(([key, value]) => [key, value + 1]));
"#),
        &mut ctx,
    )?;
    let first_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, first_module)?;

    for expected in [
        DictProjectionOp::Keys,
        DictProjectionOp::Values,
        DictProjectionOp::Entries,
    ] {
        ensure!(body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::DictProjection { op, .. } if op == expected)
        ));
    }
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictLit(_)))
    );
    Ok(())
}

#[test]
fn lowers_static_record_projection_utilities() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
declare const utils: unknown;
const mapping: Record<string, number> = { a: 1, b: 2 };
const keys = utils.keys(mapping);
const values = utils.values(mapping);
const entries = utils.entries(mapping);
"#),
        &mut ctx,
    )?;
    let second_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, second_module)?;

    for expected in [
        DictProjectionOp::Keys,
        DictProjectionOp::Values,
        DictProjectionOp::Entries,
    ] {
        ensure!(body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::DictProjection { op, .. } if op == expected)
        ));
    }
    Ok(())
}

#[test]
fn lowers_static_array_reduce_utility() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
declare const utils: unknown;
const values: number[] = [1, 2, 3];
const total = utils.reduce(values, (acc, value) => acc + value, 0);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListReduce { .. }))
    );
    Ok(())
}

#[test]
fn lowers_static_record_reduce_utility_to_accumulator_surface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
declare const utils: unknown;
const mapping: Record<string, { writable?: boolean }> = { a: { writable: false } };
const hidden = utils.reduce(
  mapping,
  (acc, attr, attrName) => (attr.writable === false ? acc.concat(attrName) : acc),
  [] as string[]
);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::ListLit(items) if items.is_empty()))
    );
    Ok(())
}

#[test]
fn lowers_interface_index_signature_field_access() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Attribute {
  type: string;
  [key: string]: any;
}
function requiresValidation(attribute: Attribute) {
  return attribute.required || attribute.unique;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(!ctx.interface_index_values.is_empty());
    Ok(())
}

#[test]
fn lowers_object_assign_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const source: Record<string, number> = { a: 1 };
const merged = Object.assign({}, source, { b: 2 });
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictAssign { .. }))
    );
    Ok(())
}

#[test]
fn lowers_object_assign_with_optional_interface_source() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Options {
  addSuffix?: boolean;
}

function merge(options?: Options): Record<string, unknown> {
  return Object.assign({}, options, {
    addSuffix: options?.addSuffix,
    comparison: 1,
  });
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_module_global_array_with_null_elements() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const daysInMonths = [31, null, 31];

function days(month: number): number {
  return daysInMonths[month] || 28;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_parse_iso_string_and_regexp_helpers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function parseDateUnit(value: string): number {
  return value ? parseInt(value) : 1;
}

function parseYear(dateString: string, additionalDigits: number): string[] | undefined {
  const regex = new RegExp("^(\\d{" + (4 + additionalDigits) + "})");
  const captures = dateString.substr(1, dateString.length).match(regex);
  const token = regex.exec(dateString);
  if (!captures) return undefined;
  return token || dateString.slice((captures[1] || captures[2]).length).match(regex);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_optional_string_length_after_truthy_guard() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface DateString {
  date?: string;
}

function read(dateStrings: DateString): number {
  if (dateStrings.date) {
    return dateStrings.date.length;
  }
  return 0;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_global_is_nan_with_coercible_unknown() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function read(): boolean {
  let offset;
  offset = 1;
  return isNaN(offset);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_global_is_nan_with_optional_number() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function read(value: number | undefined): boolean {
  return value != null && isNaN(value);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(
                expr.kind,
                ExprKind::NumericPredicate {
                    op: NumericPredicateOp::IsNaN,
                    ..
                }
            ))
    );
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::Conditional { .. }))
    );
    Ok(())
}

#[test]
fn lowers_console_warn_and_error_like_console_log() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function warn(message: string): void {
  console.warn(message);
  console.error(message);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_error_constructor_with_unknown_message() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function fail(message: unknown): void {
  throw new RangeError(message);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_timezone_offset_as_number() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function offset(date: Date): number {
  return Math.abs(date.getTimezoneOffset());
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_vitest_date_timezone_offset_mock_lifecycle() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { vi } from "vitest";

const spy = vi.spyOn(Date.prototype, "getTimezoneOffset");
spy.mockReturnValue(480);
const offset = new Date().getTimezoneOffset();
spy.mockRestore();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let expressions = module_body(&ctx, module)?
        .exprs
        .iter()
        .map(|expr| &expr.kind)
        .collect::<Vec<_>>();
    ensure!(
        expressions
            .iter()
            .any(|expr| matches!(expr, ExprKind::DateSetTimezoneOffset { .. }))
    );
    ensure!(
        expressions
            .iter()
            .any(|expr| matches!(expr, ExprKind::DateTimezoneOffset))
    );
    ensure!(
        expressions
            .iter()
            .any(|expr| matches!(expr, ExprKind::DateResetTimezoneOffset))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_fns_timezone_context_factory() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { tz } from "@date-fns/tz";

const inMidway = tz("Pacific/Midway");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(
        module_body(&ctx, module)?
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DateTimezoneContext { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_qualified_external_type_reference_as_opaque_class() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function fakeDate(): void {
  let clock: sinon.SinonFakeTimers | undefined;
  clock = undefined;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_nested_function_declaration_as_local_closure() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function outer(value: number): { inner: (next: number) => void } {
  let current = value;
  function inner(next: number) {
    current = next;
  }
  return { inner };
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_sinon_fake_timers_helper_surface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function fakeDate(date: number | Date): { fakeNow: (date: number | Date) => void } {
  let clock: sinon.SinonFakeTimers | undefined;
  function fakeNow(date: number | Date) {
    clock?.restore();
    clock = sinon.useFakeTimers(+date);
  }
  return { fakeNow };
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_do_while_statement() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function countDown(value: number): number {
  let current = value;
  do {
    current = current - 1;
  } while (current > 0);
  return current;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unknown_static_field_access() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function read(value: unknown): unknown {
  return value.date;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unknown_index_access() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function read(values: unknown, index: number): unknown {
  return values[index];
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn tolerates_describe_scope_setup_and_dynamic_test_alias() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { describe, expect, it } from "vitest";

describe("group", () => {
  const enabled = true;
  const alias = enabled ? it : it.skip;
  alias("dynamic", () => {});

  describe("nested", () => {
    it("static", () => {
      expect(enabled).toBe(true);
    });
  });
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_values_through_partial_record_alias() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Boxed<T> = Partial<Record<string, T[]>>;

function values(result: Boxed<number>): number {
  return Object.values(result).length;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_literal_types_inside_tuples() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Items = [{ a: "cat" }, { a: string }?];

function first(items: Items): string {
  return items[0].a;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_constructor_identifier_as_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { expect, it } from "vitest";

it("checks date", () => {
  const result = Date.now();
  expect(result).toBeInstanceOf(Date);
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_intl_timezone_probe_for_test_labels() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const tzName = Intl.DateTimeFormat().resolvedOptions().timeZone || process.env.tz;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_new_intl_date_time_format_format_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function formatDate(date: Date, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { year: "numeric" }).format(date);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DateToIsoString { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_new_intl_relative_time_format_format_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Options extends Intl.RelativeTimeFormatOptions {
  unit?: string;
  locale?: string;
}

function formatDistance(value: number, unit: string, options?: Options): string {
  const rtf = new Intl.RelativeTimeFormat(options?.locale, {
    numeric: "auto",
    ...options,
  });
  return rtf.format(value, unit);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 1)?;
    let body = function_body(&ctx, function)?;
    ensure!(body.exprs.iter().any(
        |expr| matches!(expr.kind, ExprKind::Literal(Literal::String(ref value)) if value.is_empty())
    ));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_guarded_dynamic_date_constructor_identifier() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function transpose(constructor: unknown): Date {
  return new constructor(0);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(0.0))))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_local_arrow_defaults_referencing_prior_params() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const override = (
  base: Date,
  year = base.getFullYear(),
  month = base.getMonth(),
) => new Date(year, month);
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_for_each_statement_callback_as_loop() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function maxValue(values: number[]): number {
  let result = 0;
  values.forEach((value, index) => {
    if (index < 0) return;
    if (result < value) result = value;
  });
  return result;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_for_each_statement_on_logical_or_empty_array_fallback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Column = { name: string };
type Table = { columns?: Column[] };

declare function createColumn(column: Column): void;

function build(table: Table): void {
  (table.columns || []).forEach((column) => createColumn(column));
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn keeps_for_of_bindings_scoped_before_later_helper_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type ForeignKey = { name: string };

function dropForeignKey(key: ForeignKey): void {}

function alter(removed: ForeignKey[], updated: { object: ForeignKey }[]): void {
  for (const dropForeignKey of removed) {
    dropForeignKey.name;
  }
  for (const updatedForeignKey of updated) {
    dropForeignKey(updatedForeignKey.object);
  }
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn keeps_returned_object_method_locals_from_shadowing_later_arrow_helpers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type ForeignKey = { name: string };
type Table = { name: string, foreignKeys: { removed: ForeignKey[], updated: { object: ForeignKey }[] } };
type SchemaDiff = { tables: { updated: Table[], removed: Table[] } };

declare function debug(value: string): void;

export default () => {
  return {
    async updateSchema(schemaDiff: SchemaDiff) {
      for (const table of schemaDiff.tables.updated) {
        for (const updatedColumn of table.foreignKeys.updated) {
          debug(updatedColumn.object.name);
        }
      }
      for (const table of schemaDiff.tables.removed) {
        for (const dropForeignKey of table.foreignKeys.removed) {
          debug(dropForeignKey.name);
        }
      }
    },
  };

  const dropForeignKey = (foreignKey: ForeignKey, existingForeignKeys?: ForeignKey[]) => {
    debug(foreignKey.name);
  };

  const alterTable = async (table: Table, existingForeignKeys: ForeignKey[] = []) => {
    await schemaBuilder(async () => {
      for (const removedForeignKey of table.foreignKeys.removed) {
        dropForeignKey(removedForeignKey, existingForeignKeys);
      }
    });
  };

  return { alterTable };
};

declare function schemaBuilder(callback: () => Promise<void>): Promise<void>;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn pushes_async_transform_into_sync_or_promise_function_array() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Data = Record<string, unknown>;

function add(transforms: Array<(data: Data) => Data | Promise<Data>>): void {
  const routeBodySanitizeTransform = async (data: Data): Promise<Data> => data;
  (transforms as Array<(data: Data) => Data | Promise<Data>>).push(routeBodySanitizeTransform);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_method_call_on_erased_function_receiver_as_unknown() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type CallableObject = (() => void) & { getSchemaName(): string | undefined };

function read(db: CallableObject): string | undefined {
  return db.getSchemaName();
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_join_with_erased_instance_separator() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class Identifiers {
  IDENTIFIER_SEPARATOR = "_";

  getNameFromTokens(nameTokens: Array<{ name: string }>): string {
    return nameTokens.map((token) => token.name).join(this.IDENTIFIER_SEPARATOR);
  }
}

export const identifiers = new Identifiers();
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn captures_this_inside_class_field_arrow_functions() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class Identifiers {
  #replacementMap = { links: "lnk" };

  get replacementMap() {
    return this.#replacementMap;
  }

  mapshortNames = (name: string): string | undefined => {
    if (name in this.replacementMap) {
      return (this.replacementMap as any)[name];
    }
    return undefined;
  };

  serializeKey = (shortName: string) => {
    return `${shortName}.${this.options.maxLength}`;
  };

  get options() {
    return { maxLength: 55 };
  }
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_private_field_assignment_in_constructor() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Options = { maxLength: number };

class Identifiers {
  #options: Options;

  constructor(options: Options) {
    this.#options = options;
  }

  get options() {
    return this.#options;
  }
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_class_extending_builtin_map() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Meta = { uid: string };

class Metadata extends Map<string, Meta> {
  get(key: string): Meta {
    if (!super.has(key)) {
      throw new Error("missing");
    }
    return super.get(key) as Meta;
  }

  add(meta: Meta) {
    createRelation(meta.uid, this);
    return this.set(meta.uid, meta);
  }

  columns(meta: Meta, key: string, attribute: { columnName?: string }) {
    return Object.assign({}, { [attribute.columnName || key]: key });
  }
}

declare function createRelation(name: string, metadata: Metadata): void;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_exported_object_methods_without_return_annotations() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Event = { params: { data: unknown } };
type Subscriber = { beforeCreate(event: Event): void };

export const subscriber: Subscriber = {
  beforeCreate(event: Event) {
    const { data } = event.params;
    touch(data);
  },
};

declare function touch(value: unknown): void;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_for_each_statement_function_callback_as_list_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function call(data: readonly number[], callbackfn: (value: number, index: number, data: readonly number[]) => void): void {
  data.forEach(callbackfn);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = function_body(&ctx, function_item(&ctx, module, 0)?)?;

    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::ListCallback {
            op: ListCallbackOp::ForEach,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn packs_normal_and_spread_arguments_into_rest_parameter() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function collect(first: number, ...rest: number[]): number {
  return rest.length;
}

function call(values: number[]): number {
  return collect(1, 2, ...values);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn splits_spread_arguments_across_fixed_and_rest_parameters() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function collect(first: unknown, second: unknown, ...rest: unknown[]): unknown {
  return second;
}

function call(values: readonly unknown[]): unknown {
  return collect("prefix", ...values);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = function_body(&ctx, function_item(&ctx, module, 1)?)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Index { .. })),
        "missing fixed parameter read from spread list"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListSlice { .. })),
        "missing rest slice from spread list"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn casts_generic_array_spread_to_rest_list_before_concatenation() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function collect(first: unknown, ...rest: unknown[]): unknown[] {
  return rest;
}

function call<Values extends unknown[]>(values: Values): unknown[] {
  return collect("prefix", ...values);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = function_body(&ctx, function_item(&ctx, module, 1)?)?;

    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::UnknownCast { target, .. }
                if matches!(ctx.krate.types.get(target), Some(Type::List(_)))
        )),
        "generic spread array was not extracted as a rest list"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn selects_array_rest_overload_for_variable_spread_tail() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function collect(context: unknown, ...dates: [unknown, unknown]): [unknown, unknown];
function collect(context: unknown, ...dates: unknown[]): unknown[];
function collect(context: unknown, ...dates: unknown[]): unknown[] {
  return dates;
}

function call<Values extends unknown[]>(values: Values): unknown[] {
  return collect(undefined, "head", ...values);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 1)?;
    ensure!(
        matches!(ctx.krate.types.get(function.return_ty), Some(Type::List(_))),
        "variable spread tail selected a fixed tuple overload"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn selects_array_rest_overload_for_conditional_array_spread() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function normalize(context: unknown, ...values: [number, number]): [number, number];
function normalize(context: unknown, ...values: number[]): number[];
function normalize(context: unknown, ...values: number[]): number[] {
  return values;
}

function call(context: unknown, comparison: number, left: number, right: number): number[] {
  return normalize(context, ...(comparison > 0 ? [left, right] : [right, left]));
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 1)?;
    ensure!(
        matches!(ctx.krate.types.get(function.return_ty), Some(Type::List(_))),
        "conditional array spread did not select the list-rest overload"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_tuple_rest_destructuring_as_list() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function pick(index: number): number {
  const [first, ...rest] = [1, 2, 3] as [number, number, number];
  return rest[index];
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_sort_with_function_reference_comparator() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function compare(left: number, right: number): number {
  return left - right;
}

function sortValues(values: number[]): number[] {
  return values.sort(compare);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn preserves_set_item_type_through_spread_sort() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const sampleIndices = new Set<number>();
const sorted = [...sampleIndices].sort((a, b) => a - b);
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_callback_dynamic_index_with_non_null_assertion() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function sample<T>(data: readonly T[]): T[] {
  const sampleIndices = new Set<number>();
  return [...sampleIndices].sort((a, b) => a - b).map((index) => data[index]!);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_sort_with_comparator_function_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const sortByImplementation = <T>(
  data: readonly T[],
  compareFn: (left: T, right: T) => number,
): T[] => [...data].sort(compareFn);
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_instanceof_inside_expect_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { expect, it } from "vitest";

it("checks instance", () => {
  const value = new Date(0);
  expect(value instanceof Date).toBe(true);
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_instanceof_on_catch_like_unknown_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { expect, test } from "vitest";

test("range error", () => {
  try {
    throw new RangeError("bad");
  } catch (e) {
    expect(e instanceof RangeError).toBe(true);
  }
});
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_constructor_field_on_date_like_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = new Date(0);
const ctor = value.constructor;
class CustomDate extends Date {}
const custom = new CustomDate(0);
const customCtor = custom.constructor;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_class_getters_as_readonly_fields() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class User {
  public get name(): string {
    return "Ada";
  }
}
const user = new User();
const name = user.name;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(ctx.krate.types.all().iter().any(|ty| {
        matches!(
            ty,
            Type::Class { name, .. } if ctx.krate.symbols.get(*name) == Some("User")
        )
    }));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unannotated_class_getters_as_unknown_fields() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class Validator {
  public get scalarFieldsEnum() {
    return {};
  }
}
const validator = new Validator();
const value = validator.scalarFieldsEnum;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_block_scoped_class_declarations() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function make(): void {
  class CustomDate extends Date {}
  function acceptDate(value: CustomDate): void {}
  const value = new CustomDate(0);
  acceptDate(new CustomDate(0));
  const ctor = CustomDate;
  value instanceof CustomDate;
  const base = new Date(0);
  base instanceof CustomDate;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_block_scoped_type_declarations() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function check(): void {
  interface AB {
    a: number;
    b: number;
  }
  type Boxed = { value: AB };
  const item: Boxed = { value: { a: 1, b: 2 } };
  item.value.a;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_imported_constructor_as_opaque_class() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { UTCDate } from "@date-fns/utc";
import { expect, it } from "vitest";

it("checks extension date", () => {
  const result = new UTCDate();
  expect(result).toBeInstanceOf(UTCDate);
  expect(result instanceof UTCDate).toBe(true);
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn allows_map_get_with_union_member_key_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function lookup<T, S>(map: Map<S | T, number>, value: T): number | undefined {
  return map.get(value);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_keys_after_object_string_nullish_guards() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function empty(data: object | string | undefined): boolean {
  if (data === "" || data === undefined) {
    return true;
  }
  if (Array.isArray(data)) {
    return data.length === 0;
  }
  return Object.keys(data).length === 0;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_for_in_after_typeof_object_guard() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function hasEnumerable(data: unknown): boolean {
  if (typeof data !== "object") {
    return false;
  }
  for (const key in data) {
    return true;
  }
  return false;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_get_own_property_symbols_length() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function symbolCount(data: unknown): number {
  return Object.getOwnPropertySymbols(data).length;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_uninitialized_let_as_unknown_for_date_coercion() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function parseDate(): Date {
  return new Date(0);
}

function read(): number {
  let date;
  date = parseDate();
  return +date;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_assign_call_on_callable_target() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const fnValue = (value: number): number => value;
const assigned = Object.assign(fnValue, { lazy: fnValue });
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(
        |expr| matches!(expr.kind, ExprKind::CallableObjectAssign { ref props, .. } if props.len() == 1)
    ));
    Ok(())
}

#[test]
fn lowers_object_assign_call_on_inline_callable_target() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const assigned = Object.assign(
  (value: number): number => value,
  { flush: (): number => 1 },
);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(
        |expr| matches!(expr.kind, ExprKind::CallableObjectAssign { ref props, .. } if props.len() == 1)
    ));
    Ok(())
}

#[test]
fn local_function_implementations_shadow_cross_module_overloads() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type Debouncer<F> = { readonly call: () => void };
function debounce<F>(func: F): Debouncer<F>;
function debounce<F>(func: F): Debouncer<F> {
  return { call: () => {} };
}
"#),
        &mut ctx,
    )?;
    let module_id = lower_ok(
        ts!(r#"
function debounce(func: () => void) {
  return Object.assign(func, { cancel: () => {} });
}

const debounced = debounce(() => {});
debounced();
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn infers_async_arrow_const_return_type_from_await_body() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
async function sleep(ms: number): Promise<void> {
  await new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

async function run(): Promise<void> {
  await yieldExecution();
}

const yieldExecution = async () => await sleep(0);
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_ignored_promise_then_catch_chain() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
async function load(values: number[]): Promise<number[]> {
  return values;
}

function run(values: number[]): void {
  load(values)
    .then((response) => {
      response.length;
    })
    .catch((_error) => {});
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_spread_from_generic_accumulator_fallback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function append<T>(items: T | undefined, item: number): number[] {
  return [...(items ?? []), item];
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn keeps_vitest_mock_with_implementation_callable() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { vi } from "vitest";

async function run(): Promise<void> {
  const mockApi = vi.fn<(words: readonly string[]) => Promise<Record<string, number>>>(
    async (words) => ({ count: words.length }),
  );
  await mockApi(["a"]);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn keeps_captured_vitest_mock_callable_inside_async_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { vi } from "vitest";

function batch<Params extends any[], BatchResponse>(
  callback: (requests: readonly Params[]) => Promise<BatchResponse>,
): void {
  callback([]);
}

function run(): void {
  const mockApi = vi.fn<(words: readonly string[]) => Promise<Record<string, number>>>(
    async (words) => ({ count: words.length }),
  );
  batch(async (requests: readonly [word: string][]) => await mockApi(requests.flat()));
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_promise_resolve_and_exported_object_values_const() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const TYPED_ARRAY = new Uint8Array(1);
export const DATA = {
  promise: Promise.resolve(5),
  string: "text",
  typedArray: TYPED_ARRAY,
} as const;
export const VALUES = Object.values(DATA);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 2);
    ensure!(ctx.krate.types.all().iter().any(|ty| {
        matches!(
            ty,
            Type::Future(inner) if matches!(ctx.krate.types.get(*inner), Some(Type::Float))
        )
    }));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_generic_promise_constructor_executor_as_future() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function makeValue(): Promise<number> {
  return new Promise<number>((resolve) => {
    resolve(1);
  });
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_function_length_to_len_expr() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const fnValue = (left: number, right: number): number => left + right;
const arity = fnValue.length;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Len { .. }))
    );
    Ok(())
}

#[test]
fn lowers_function_bind_result_as_array_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function add(left: number, right: number): number {
  return left + right;
}

function shift(values: number[]): number[] {
  const addOne = add.bind(null, 1);
  return values.map(addOne);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(
                expr.kind,
                ExprKind::Closure(smelt_hir::ClosureExpr {
                    callback_body: None,
                    ..
                })
            )),
        "expected bind to lower to a first-class closure body"
    );
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::ListCallback { .. })),
        "expected bound function local to be accepted as an array callback"
    );
    Ok(())
}

#[test]
fn lowers_bind_captures_inside_for_each_callback_blocks() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function constructFrom(context: unknown, value: unknown): unknown {
  return value;
}

function max(dates: unknown[]): unknown {
  let context: ((value: unknown) => unknown) | undefined;
  dates.forEach((date) => {
    if (!context && typeof date === "object") {
      context = constructFrom.bind(null, date) as (value: unknown) => unknown;
    }
  });
  return context;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    let mut saw_nested_bind_capture = false;
    let mut saw_root_bind_arg_capture = false;
    for body in &ctx.krate.bodies {
        for block in &body.blocks {
            for stmt_id in &block.stmts {
                let stmt = &body.stmts[stmt_id.0 as usize];
                let Stmt::Let { pat, .. } = stmt else {
                    continue;
                };
                let smelt_hir::Pattern::Binding(local) = body.patterns[pat.0 as usize] else {
                    continue;
                };
                let Some(name) = body.locals[local.0 as usize]
                    .name
                    .and_then(|symbol| ctx.krate.symbols.get(symbol))
                else {
                    continue;
                };
                if name == "__smelt_bind_arg_0" {
                    if block.stmts == body.blocks[body.root.0 as usize].stmts {
                        saw_root_bind_arg_capture = true;
                    } else {
                        saw_nested_bind_capture = true;
                    }
                }
            }
        }
    }

    ensure!(
        saw_nested_bind_capture,
        "expected bound callback argument capture to be emitted inside the callback block"
    );
    ensure!(
        !saw_root_bind_arg_capture,
        "expected callback-local bind argument capture not to leak to the function root"
    );
    Ok(())
}

#[test]
fn selects_tuple_rest_overload_from_source_arguments() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function pair(...values: [number, number]): [number, number];
function pair(...values: number[]): number[] {
  return values;
}

const selected = pair(1, 2);

function pairWithSeed(seed: number, ...values: [number, number]): [number, number];
function pairWithSeed(seed: number, ...values: number[]): number[] {
  return values;
}

const selectedWithSeed = pairWithSeed(0, 1, 2);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.locals
            .iter()
            .any(|local| matches!(ctx.krate.types.get(local.ty), Some(Type::Tuple(items)) if items.len() == 2)),
        "expected tuple rest overload return type to be selected"
    );
    Ok(())
}

#[test]
fn selects_list_rest_overload_with_empty_tail() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function pick(seed: number, value: string, ...tail: string[]): number;
function pick(value: string, ...tail: string[]): (seed: number) => string;
function pick(...args: unknown[]): unknown {
  return args[0];
}

const selected = pick(1, "a");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.locals
            .iter()
            .any(|local| matches!(ctx.krate.types.get(local.ty), Some(Type::Float))),
        "expected data-first overload with empty list-rest tail to be selected"
    );
    Ok(())
}

#[test]
fn extracts_structural_fields_from_referenced_generic_interfaces_and_pick() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface LocaleOptions {
  weekStartsOn?: number;
}

interface Locale {
  options?: LocaleOptions;
  code: string;
}

interface LocalizedOptions<LocaleFields extends keyof Locale> {
  locale?: Pick<Locale, LocaleFields>;
}

interface WeekOptions {
  weekStartsOn?: number;
}

type DefaultOptions = LocalizedOptions<"options"> & WeekOptions;

function read(options?: DefaultOptions): number {
  const direct = options?.weekStartsOn;
  const locale = options?.locale?.options?.weekStartsOn;
  return 0;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    Ok(())
}

#[test]
fn lowers_never_rest_strict_function_spread_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type StrictFunction = (...args: never) => unknown;

function callStrict(fn: StrictFunction, args: readonly unknown[]): unknown {
  return fn(...args);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. }))
    );
    Ok(())
}

#[test]
fn allows_strict_function_as_function_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type StrictFunction = (...args: never) => unknown;

function dataLast(fn: StrictFunction, args: readonly unknown[]): unknown {
  return fn(...args);
}

function purry(fn: StrictFunction, args: readonly unknown[]): unknown {
  return dataLast(fn, args);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::Call { .. }))
    );
    Ok(())
}

#[test]
fn lowers_parenthesized_callable_intersection_type_surface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type LazyFn = (value: unknown) => unknown;
type LazyMeta = { readonly single?: boolean };
export type LazyDefinition = {
  readonly lazy: LazyMeta & ((...args: any) => LazyFn);
  readonly lazyArgs: readonly unknown[];
};
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(
        ctx.krate
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::Function(function) if function.params.len() == 1))
    );
    Ok(())
}

#[test]
fn keeps_callable_alias_intersections_callable_after_reference() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type LazyEvaluator<T = unknown, R = T> = (
  item: T,
  index: number,
  data: readonly T[],
) => R;

type PreparedLazyFunction<T> = LazyEvaluator<T> & {
  index: number;
  items: T[];
};

function processItem(lazyFn: PreparedLazyFunction<number>): number {
  const { index, items } = lazyFn;
  return lazyFn(1, index, items);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. })),
        "expected callable intersection alias references to lower as closure calls",
    );
    Ok(())
}

#[test]
fn narrows_typeof_function_out_of_callable_tuple_union() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const labels = { asc: true, desc: false } as const;

type Projection<T> = (value: T) => string;
type OrderRule<T> =
  | Projection<T>
  | readonly [projection: Projection<T>, direction: keyof typeof labels];

function projector<T>(primaryRule: OrderRule<T>): Projection<T> {
  return typeof primaryRule === "function" ? primaryRule : primaryRule[0];
}

function direction<T>(primaryRule: OrderRule<T>): string {
  return "function" !== typeof primaryRule ? primaryRule[1] : "asc";
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn calls_callable_branch_of_union_local_and_nested_result() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Curried = ((value: number) => Curried | string) | string;

function run(fn: Curried): string {
  const first = fn(3);
  return first(2) as string;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    let closure_calls = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. }))
        .count();
    ensure_eq!(closure_calls, 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn calls_overloaded_interface_call_signature_by_argument_count() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Step1 {
  (): Step1;
  (value: number): string;
}

interface Step2 {
  (): Step2;
  (value: number): Step1;
  (value: number, next: number): string;
}

function run(fn: Step2): string {
  const first = fn(2);
  return first(1);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    let closure_calls = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. }))
        .count();
    ensure_eq!(closure_calls, 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unhinted_function_expression_object_property_as_unknown_callable() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function build(args: { callback?: unknown }): unknown {
  return args.callback;
}

const result = build({
  callback: function (value) {
    return Number(value) - 1;
  },
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::Closure { .. })),
        "expected unhinted function expression property to lower as a closure",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_annotated_arrow_const_with_callable_alias_hint() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type LocalizeFn<Value> = (value: Value, options?: { unit?: string }) => string;

const ordinalNumber: LocalizeFn<number> = (dirtyNumber, options) => {
  const number = Number(dirtyNumber);
  const unit = options?.unit;
  return unit ? String(number) : "0";
};
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(
        ctx.krate
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::Function(function) if function.params.len() == 2))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn ignores_browser_guarded_describe_branch_and_skipped_test() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { describe, it, expect } from "vitest";

describe("browser guard", () => {
  if (typeof window !== "undefined") {
    it("browser only", () => {
      document.body.append("x");
    });
  } else {
    it.skip("browser only", () => {});
  }

  it("native", () => {
    expect(1).toBe(1);
  });
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    let tests = ctx
        .krate
        .items
        .iter()
        .filter(|item| matches!(item, Item::Function(function) if function.is_test))
        .count();
    ensure_eq!(tests, 1);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_for_each_statement_with_tuple_destructuring_param() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { describe, it, expect } from "vitest";

describe("forEach", () => {
  it("destructures tuple cases", () => {
    [
      ["do", "1er"],
      ["do M", "1er 1"],
    ].forEach(([formatString, expectedResult]) => {
      expect(formatString).toBe(formatString);
      expect(expectedResult).toBe(expectedResult);
    });
  });
});
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_module_symbol_const_used_by_arrow_closure() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const Marker = Symbol("marker");
const read = <T>(): T => Marker as T;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn captures_type_assertion_wrapped_closure_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function read(value: unknown): () => string {
  const local = "value";
  return () => local as string;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_async_arrow_expression_object_property() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type AsyncCaller = {
  readonly call: (...params: number[]) => Promise<void>;
};

export function makeCaller(): AsyncCaller {
  return {
    call: async (...params: number[]): Promise<void> =>
      new Promise<void>((resolve) => setTimeout(resolve, 1)),
  };
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    let async_closure_body = ctx.krate.bodies.iter().any(|body| {
        body.exprs.iter().any(|expr| {
            let ExprKind::Closure(closure) = &expr.kind else {
                return false;
            };
            let Some(Type::Function(function)) = ctx.krate.types.get(expr.ty) else {
                return false;
            };
            function.is_async
                && matches!(
                    ctx.krate.types.get(closure.return_ty),
                    Some(Type::Future(_))
                )
                && ctx
                    .krate
                    .bodies
                    .get(closure.body.0 as usize)
                    .is_some_and(|body| body.async_state_machine.is_some())
        })
    });
    ensure!(
        async_closure_body,
        "expected object-property async arrow to lower as an async closure"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_function_local_arrow_forward_references() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function run(): void {
  const first = (): void => {
    second();
  };
  const second = (): void => {};
  first();
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_nullish_assignment_on_optional_locals() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function read(value: number | undefined): number | undefined {
  const now = 1;
  value ??= now;
  return value;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_logical_or_assignment_as_lazy_value_selection() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function initialize(value: number): number {
  value ||= 3;
  return value;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = function_body(&ctx, function_item(&ctx, module, 0)?)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Conditional { .. })),
        "expected ||= to preserve short-circuit selection through a conditional"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn destructures_fields_from_union_intersection_aliases() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Timing =
  | ({ readonly triggerAt?: "end" } & (
      | { readonly minGapMs: number }
      | {
          readonly minQuietPeriodMs?: number;
          readonly maxBurstDurationMs?: number;
          readonly minGapMs?: never;
        }
    ))
  | {
      readonly triggerAt: "start" | "both";
      readonly minQuietPeriodMs?: number;
      readonly maxBurstDurationMs?: number;
      readonly minGapMs?: number;
    };

type Options<R> = {
  readonly reducer?: (accumulator: R | undefined) => R;
} & Timing;

export function read<R>({ minQuietPeriodMs }: Options<R>): number {
  return minQuietPeriodMs ?? 0;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn infers_function_parameter_types_from_defaults() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function delay(wait = 0): number {
  return wait + 1;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_console_members_inside_test_callbacks() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { it } from "vitest";

it("logs diagnostic output", () => {
  console.log("starting");
  console.warn("fallback");
  console.error("failed");
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_vitest_spy_on_console_mock_lifecycle() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { beforeEach, afterEach, describe, it, vi } from "vitest";
import type { MockInstance } from "vitest";

describe("console.warn", () => {
  let warn: MockInstance;

  beforeEach(() => {
    warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  });

  afterEach(() => {
    warn.mockRestore();
  });

  it("runs", () => {
    console.warn("hidden");
  });
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_vitest_fake_timers_to_date_now_mock_state() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { vi } from "vitest";

vi.useFakeTimers({ now: new Date(2020, 0, 1) });
vi.setSystemTime(new Date(2020, 0, 2));
const now = Date.now();
vi.useRealTimers();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DateSetNow { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DateResetNow))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DateNow))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_has_own_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const mapping: Record<string, number> = { a: 1, b: 2 };
const first = Object.hasOwn(mapping, "a");
const second = mapping.hasOwnProperty("b");
function generic<T>(value: T, key: string): boolean {
  return Object.hasOwn(value, key);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::DictContainsKey { .. }))
            .count()
            == 2
    );
    let generic_body = function_body(&ctx, function_item(&ctx, module, 0)?)?;
    ensure!(
        generic_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictContainsKey { .. }))
    );
    Ok(())
}

#[test]
fn lowers_computed_destructuring_key_with_type_assertion() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function pick<T>(value: T, key: string): unknown {
  const { [key as keyof T]: picked } = value;
  return picked;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = function_body(&ctx, function_item(&ctx, module, 0)?)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Index { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_generic_record_key_aliases_for_later_instantiation() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type UpsertProp<T, K extends PropertyKey, V> = T & Record<K, V>;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 1);
    ensure!(
        ctx.krate.types.all().iter().any(|ty| matches!(
            ty,
            Type::Dict(key, _) if matches!(ctx.krate.types.get(*key), Some(Type::TypeParam { .. }))
        )),
        "expected generic Record<K, V> to preserve K for later substitution",
    );
    Ok(())
}

#[test]
fn normalizes_record_property_key_surfaces() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type NumberRecord = Record<number, string>;
type LiteralRecord = Record<123 | "name", boolean>;
type PropertyKeyRecord = Record<PropertyKey, unknown>;
type ConditionalRecord<T extends boolean> = Record<T extends true ? number : string, string>;
type UnionRecord = Record<number, string> | Record<string, number>;
"#),
        &mut ctx,
    )?;

    let has_string_keyed_record = ctx.krate.types.all().iter().any(|ty| {
        matches!(
            ty,
            Type::Dict(key, _) if matches!(ctx.krate.types.get(*key), Some(Type::String))
        )
    });
    ensure!(
        has_string_keyed_record,
        "expected concrete Record key surfaces to normalize to string-key dictionaries",
    );
    Ok(())
}

#[test]
fn lowers_template_literal_tuple_element_types() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type Entry = readonly [`testing_${string}`, boolean];
type Entries = readonly Entry[];
type BigIntLiterals = 1n | 2n | 3n;
"#),
        &mut ctx,
    )?;
    ensure!(
        ctx.krate
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::Tuple(items) if items.iter().any(|item| matches!(ctx.krate.types.get(*item), Some(Type::String))))),
        "expected template literal tuple keys to lower as strings",
    );
    Ok(())
}

#[test]
fn lowers_top_level_arrow_const_used_by_later_function() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const compare = (left: number, right: number): number => left - right;

function sortValues(values: number[]): number[] {
  return values.toSorted(compare);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = function_body(&ctx, function_item(&ctx, module, 1)?)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListSort { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_static_function_references() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function useUnary(fn: (value: unknown) => unknown): unknown {
  return fn([]);
}

function useBinary(fn: (left: unknown, right: unknown) => boolean): boolean {
  return fn(1, 1);
}

const entries = useUnary(Object.entries);
const values = useUnary(Object.values);
const keys = useUnary(Object.keys);
const rebuilt = useUnary(Object.fromEntries);
const same = useBinary(Object.is);
const owned = useBinary(Object.hasOwn);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let closure_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::Closure(_)))
        .count();
    ensure!(
        closure_count >= 6,
        "expected Object static member references to lower as callables",
    );
    Ok(())
}

#[test]
fn lowers_callback_typeof_expression_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values = ["a", "b"].map((item) => typeof item);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::ListCallback {
                op: ListCallbackOp::Map,
                ..
            }
        )),
        "expected callback typeof expression to lower inside array map",
    );
    Ok(())
}

#[test]
fn lowers_object_spread_literals_as_ordered_assignments() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const base: Record<string, number> = { a: 1, b: 2 };
const merged: Record<string, number> = { ...base, b: 3, c: 4 };
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictAssign { .. })),
        "expected object spread to lower to an ordered dictionary assignment",
    );
    Ok(())
}

#[test]
fn lowers_generic_object_spread_with_computed_key() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type UpsertProp<T, K extends PropertyKey, V> = T & Record<K, V>;
export const addPropImplementation = <T, K extends PropertyKey, V>(
  obj: T,
  prop: K,
  value: V,
): UpsertProp<T, K, V> => ({ ...obj, [prop]: value });
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    let function = ctx
        .krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Function(function) => Some(function),
            _ => None,
        })
        .ok_or_else(|| "missing lowered function".to_owned())?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictAssign { .. })),
        "expected generic spread and computed key to lower through DictAssign",
    );
    Ok(())
}

#[test]
fn lowers_optional_object_spread_for_option_bags() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Options {
  value?: number;
}

function merge(options?: Options): Record<string, number> {
  return { ...options, value: 1 };
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_conditional_object_spread_sources() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function merge(maxWait: number | undefined): Record<string, number> {
  return {
    minQuietPeriodMs: 0,
    ...(maxWait !== undefined && { maxBurstDurationMs: maxWait }),
  };
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function) => Some(function),
            _ => None,
        })
        .ok_or_else(|| "missing lowered function".to_owned())?;
    let body = function_body(&ctx, function)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Conditional { .. })),
        "expected conditional object spread source to lower as a conditional record",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_ternary_object_spread_sources_with_record_context() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function merge(trailing: boolean, leading: boolean): Record<string, string> {
  return {
    mode: "wait",
    ...(trailing
      ? leading
        ? { triggerAt: "both" }
        : { triggerAt: "end" }
      : { triggerAt: "start" }),
  };
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_type_assertion_call_arguments_with_asserted_object_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function readA(value: Record<string, string>): string {
  return value.a;
}
const result = readA({} as { a: string });
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::DictLit(_)
                if matches!(ctx.krate.types.get(expr.ty), Some(Type::Dict(_, value)) if ctx.krate.types.get(*value) == Some(&Type::String))
        )),
        "expected asserted object call argument to use the asserted object type",
    );
    Ok(())
}

#[test]
fn lowers_vitest_expect_type_of_as_type_only_noop() -> Result<(), String> {
    let source = ts!(r#"
import { expectTypeOf, test } from "vitest";

test("type assertion", () => {
  const result = {} as { a: string };
  expectTypeOf(result).toEqualTypeOf<{ a: string }>();
  expectTypeOf(result).toEqualTypeOf<{ [Symbol.iterator]: string }>();
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/type.test-d.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::None))),
        "expected type-test assertion to lower to a no-op expression",
    );
    Ok(())
}

#[test]
fn lowers_json_stringify_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2];
const text = JSON.stringify(values);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::JsonStringify { .. })),
        "expected JSON.stringify lowering",
    );
    Ok(())
}

#[test]
fn lowers_json_stringify_with_optional_union_fields() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const result: {
  license?: string | null;
  error?: string;
  lastCheckAt?: number;
} = { lastCheckAt: Date.now() };

const text = JSON.stringify(result);
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn captures_unannotated_module_let_literals_in_arrow_functions() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
let initialized = false;

const init = () => {
  if (initialized) {
    return;
  }

  initialized = true;
};
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_node_path_join_and_resolve_static_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import path from 'path';

const configPath = path.join('/tmp', '.strapi-updater.json');
const resourcePath = path.resolve(__dirname, '../resources/key.pub');
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_json_parse_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const text = "[1,2]";
const values = JSON.parse<number[]>(text);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::JsonParse { .. })),
        "expected JSON.parse lowering",
    );
    Ok(())
}

#[test]
fn lowers_untyped_json_parse_to_unknown_record() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const text = "{\"enabled\":true}";
const values = JSON.parse(text);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs.iter().any(|expr| {
            matches!(expr.kind, ExprKind::JsonParse { .. })
                && matches!(ctx.krate.types.get(expr.ty), Some(Type::Dict(_, _)))
        }),
        "expected untyped JSON.parse to lower as an unknown record",
    );
    Ok(())
}

#[test]
fn lowers_json_parse_with_erased_text_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const parseStored = (result: any) => (result ? JSON.parse(result.value) : result);
const parseTyped = (result: any) => JSON.parse<Record<string, unknown>>(result.value);
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_json_parse_with_assertion_target() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface ServerConfig {
  host: string;
  port: number;
}

const text = "{\"host\":\"localhost\",\"port\":1337}";
const config = JSON.parse(text) as ServerConfig;
const bag = JSON.parse(text) as Record<string, unknown>;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure_eq!(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::JsonParse { .. }))
            .count(),
        2
    );
    Ok(())
}

#[test]
fn lowers_regexp_test_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const text = "abc123";
const pattern = "\\d+";
const hasDigits = new RegExp(pattern).test(text);
const alsoHasDigits = RegExp(pattern).test(text);
const literalHasDigits = /\d+/.test(text);
const savedPattern = /\w+/;
const savedHasWord = savedPattern.test(text);
const patterns = { delimiter: /[T ]/, timezone: /([Z+-].*)$/ };
const objectPatternMatches = patterns.delimiter.test(text);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .filter(|expr| {
                matches!(
                    expr.kind,
                    ExprKind::RegexExec { .. }
                        | ExprKind::RegexIsMatch {
                            op: smelt_hir::RegexMatchOp::Search,
                            ..
                        }
                )
            })
            .count()
            == 5,
        "expected RegExp.test lowering",
    );
    Ok(())
}

#[test]
fn lowers_regexp_test_with_erased_haystack() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function hasDigits(value: unknown) {
  return /\d+/.test(value);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn does_not_route_validation_test_methods_as_regexp() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { yup } from '@strapi/utils';

const schema = yup
  .string()
  .test('is-valid-text', 'Text must be defined', (text: unknown) => {
    return typeof text === 'string' || text === '';
  });
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn rejects_unsupported_regexp_test_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const text = "abc123";
const hasDigits = new RegExp("\\d+", "g").test(text);
"#),
        &mut ctx,
    )?;

    let mut ctx = HirCtx::new();
    let non_string = lowering_errors(
        ts!(r#"
const text = "abc123";
    const hasDigits = new RegExp(1).test(text);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&non_string, "string pattern")?;
    Ok(())
}

#[test]
fn lowers_json_stringify_replacer_and_class_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2];
const text = JSON.stringify(values, null);
"#),
        &mut ctx,
    )?;
    let first_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, first_module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::JsonStringify { .. })),
        "expected JSON.stringify with replacer argument to lower",
    );

    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class User {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}
const user = new User("Ada");
const text = JSON.stringify(user);
"#),
        &mut ctx,
    )?;
    let second_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, second_module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::JsonStringify { .. })),
        "expected JSON.stringify class value to lower",
    );
    Ok(())
}

#[test]
fn rejects_unsupported_json_parse_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let too_many_args = lowering_errors(
        ts!(r#"
const text = "[1,2]";
const values = JSON.parse<number[]>(text, 1);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&too_many_args, "exactly one text argument")
}

#[test]
fn lowers_string_includes_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "Smelt";
const has = word.includes("mel");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::StringContains { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_includes_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2, 3];
const has = values.includes(2);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListContains { .. })),
        "array includes did not lower to ListContains"
    );
    Ok(())
}

#[test]
fn lowers_optional_array_includes_before_string_includes() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
interface Includes {
  populate?: string[];
}
function run(includes?: Includes) {
  return includes?.populate?.includes('nonAttributesOperators');
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_set_constructor_and_has_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: Set<number> = new Set([1, 2, 3]);
const has = values.has(2);
const empty: Set<string> = new Set();
const genericEmpty = new Set<number>();
const genericEmptyLiteral = new Set<string>([]);
const source: readonly number[] = [1, 2, 3];
const fromSource = new Set(source);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::SetLit(_)))
            .count()
            >= 4,
        "Set constructor did not lower to SetLit"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::SetContains { .. })),
        "Set.has did not lower to SetContains"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListToSet { .. })),
        "Set constructor from array did not lower to ListToSet"
    );
    Ok(())
}

#[test]
fn lowers_rest_parameters_with_type_level_tuple_alias_constraints() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type StrictFunction = (...args: never) => unknown;
type IterableContainer = readonly unknown[];
type TuplePrefix<T extends IterableContainer> = readonly unknown[];
type TupleSuffix<T extends IterableContainer> = readonly unknown[];
type RemovePrefix<
  T extends IterableContainer,
  Prefix extends TuplePrefix<T>,
> = readonly unknown[];
type RemoveSuffix<
  T extends IterableContainer,
  Suffix extends TupleSuffix<T>,
> = readonly unknown[];

export function partialBind<
  F extends StrictFunction,
  PrefixArgs extends TuplePrefix<Parameters<F>>,
  RemovedPrefix extends RemovePrefix<Parameters<F>, PrefixArgs>,
>(
  func: F,
  ...partial: PrefixArgs
): (
  ...rest: RemovedPrefix extends IterableContainer ? RemovedPrefix : never
) => ReturnType<F> {
  return (...rest) => func(...partial, ...rest);
}

export function partialLastBind<
  F extends StrictFunction,
  SuffixArgs extends TupleSuffix<Parameters<F>>,
  RemovedSuffix extends RemoveSuffix<Parameters<F>, SuffixArgs>,
>(
  func: F,
  ...partial: SuffixArgs
): (
  ...rest: RemovedSuffix extends IterableContainer ? RemovedSuffix : never
) => ReturnType<F> {
  return (...rest) => func(...rest, ...partial);
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_all_rest_tuple_spread_return_type_as_list() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type IterableContainer = readonly unknown[];

export const concatImplementation = <
  T1 extends IterableContainer,
  T2 extends IterableContainer,
>(
  arr1: T1,
  arr2: T2,
): [...T1, ...T2] => [...arr1, ...arr2];
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function)
                if matches!(
                    ctx.krate.symbols.get(function.name),
                    Some("concatImplementation" | "concat_implementation")
                ) =>
            {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "missing concatImplementation".to_owned())?;
    ensure!(
        matches!(ctx.krate.types.get(function.return_ty), Some(Type::List(_))),
        "expected all-rest tuple spread return type to lower as a list",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_random_bigint_stdlib_surface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function asBigInt(bytes: Iterable<number>): bigint {
  let result = 0n;
  for (const byte of bytes) {
    result = (result << 8n) + BigInt(byte);
  }
  return result >> 1n;
}

function random(numBytes: number): Uint8Array {
  const output = new Uint8Array(numBytes);
  if (typeof crypto === "undefined") {
    for (let index = 0; index < numBytes; index += 1) {
      output[index] = Math.floor(Math.random() * 256);
    }
  } else {
    crypto.getRandomValues(output);
  }
  return output;
}

const text = (10n).toString(2);
const bits = text.length;
const pivot = (4 + 10) >>> 1;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        ctx.krate.bodies.iter().any(|body| {
            body.exprs.iter().any(|expr| {
                matches!(
                    expr.kind,
                    ExprKind::BinOp {
                        op: BinOp::Shl | BinOp::Shr | BinOp::UShr,
                        ..
                    }
                )
            })
        }),
        "bitwise shift operators did not lower"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericToStringRadix { .. })),
        "number.toString(radix) did not lower"
    );
    Ok(())
}

#[test]
fn lowers_array_from_length_mapper() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function range(start: number, length: number, step: number): number[] {
  return Array.from({ length }, (_, i) => (i === 0 ? start : start + i * step));
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(
        ctx.krate.bodies.iter().any(|body| {
            body.exprs.iter().any(|expr| {
                let ExprKind::ListFromLengthMap { callback, .. } = expr.kind else {
                    return false;
                };
                matches!(
                    body.exprs.get(callback.0 as usize).map(|expr| &expr.kind),
                    Some(ExprKind::Closure(closure)) if closure.callback_body.is_none()
                )
            })
        }),
        "Array.from({{ length }}, mapper) did not lower through a normal closure body"
    );
    Ok(())
}

#[test]
fn lowers_array_from_length_without_mapper() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("const sparse = Array.from({ length: 1000 });"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListFromLength { .. })),
        "Array.from({{ length }}) did not lower"
    );
    Ok(())
}

#[test]
fn lowers_zero_arg_computed_member_function_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type QueryParam = 'fields' | 'populate';

const map: Record<QueryParam, () => string> = {
  fields: () => 'fields',
  populate: () => 'populate',
};

const param: QueryParam = 'fields';
const value = map[param]();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. })),
        "zero-argument computed member function call did not lower"
    );
    Ok(())
}

#[test]
fn lowers_for_each_statement_on_opaque_array_field() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function addRoutes(routes: any) {
  routes.routes.forEach((route) => {
    route.handler;
  });
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_template_literals_inside_array_literals() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const prefix = 'api::';
const route = { handler: 'article.find' };
const scope = [`${route.handler.startsWith(prefix) ? '' : prefix}${route.handler}`];
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_lodash_for_each_collection_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import _ from 'lodash';

function register(routes: any) {
  _.forEach(routes, (router) => {
    router.type = router.type || 'admin';
  });
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_strapi_register_routes_lodash_for_each_shape() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import _ from 'lodash';

const createRouteScopeGenerator = (namespace: string) => (route: any) => {
  const prefix = namespace.endsWith('::') ? namespace : `${namespace}.`;

  if (typeof route.handler === 'string') {
    route.config = {
      auth: {
        scope: [`${route.handler.startsWith(prefix) ? '' : prefix}${route.handler}`],
      },
    };
  }
};

function register(strapi: any) {
  const generateRouteScope = createRouteScopeGenerator(`admin::`);

  _.forEach(strapi.admin.routes, (router) => {
    router.type = router.type || 'admin';
    router.prefix = router.prefix || `/admin`;
    router.routes.forEach((route) => {
      generateRouteScope(route);
      route.info = { pluginName: 'admin' };
    });
    strapi.server.routes(router);
  });
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_yup_test_and_await_opaque_async_surfaces() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { yup } from '@strapi/utils';

const schema = yup.mixed().test(() => false);
const arraySchema = yup.array().of(
  yup.lazy((value) => {
    return yup.mixed().test(() => false);
  }) as any
);

const validate = async (config: unknown) => {
  await schema.validate(config, { strict: true });
  await arraySchema.validate(config, { strict: true });
};
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_callback_instanceof_dynamic_constructor() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const mappings = [{ classError: Error, status: 400 }];

function format(error: unknown) {
  return mappings.find((pair) => error instanceof pair.classError);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_ambient_module_and_this_in_function_expression() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
declare module 'koa' {
  interface BaseResponse {
    send: (data: any, status?: number) => void;
  }
}

const response: any = {};
response.send = function send(data, status = 200) {
  this.status = status;
  this.body = data;
};
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_top_level_destructured_module_globals_in_functions() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { contentTypes as contentTypesUtils } from '@strapi/utils';

const {
  CREATED_AT_ATTRIBUTE,
  UPDATED_AT_ATTRIBUTE,
} = contentTypesUtils.constants;

function addTimestamps(schema: any) {
  Object.assign(schema.attributes, {
    [CREATED_AT_ATTRIBUTE]: { type: 'datetime' },
    [UPDATED_AT_ATTRIBUTE]: { type: 'datetime' },
  });
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_lodash_has_path_predicates() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import _ from 'lodash';

function addOptions(schema: any) {
  if (!_.has(schema, 'options.draftAndPublish')) {
    schema.options = { draftAndPublish: false };
  }
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_typeof_undefined_checks() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function shouldCount(withCount?: boolean): boolean {
  if (typeof withCount === 'undefined') {
    return false;
  }

  return typeof withCount !== 'undefined';
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_external_static_member_new_expressions() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { errors } from '@strapi/utils';

function fail(): never {
  throw new errors.ValidationError('invalid');
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_object_metadata_mutation_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const target = {};
const proto = {};
Object.setPrototypeOf(target, proto);
Object.defineProperty(target, Symbol('custom'), {
  writable: false,
});
Object.freeze(target);
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_node_dirname_buffer_and_error_call_surface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { resolve } from 'path';

const keyPath = resolve(__dirname, '../key.pub');
const [signature, content] = Buffer.from('encoded', 'base64').toString().split('\n');

function fail(): never {
  throw Error('bad license');
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_error_subclass_construction() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
class LicenseCheckError extends Error {
  shouldFallback = false;

  constructor(message: string, shouldFallback = false) {
    super(message);
    this.shouldFallback = shouldFallback;
  }
}

function fail(): never {
  throw new LicenseCheckError('bad license', true);
}

const failLater = () => {
  throw new LicenseCheckError('bad license', true);
};
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_cron_style_math_floor_and_negated_filter_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { isEmpty, negate } from 'lodash/fp';

const COMPONENTS: { limit: number }[] = [{ limit: 60 }];

const shift = (component: string, index: number) => {
  const { limit } = COMPONENTS[index];
  const [, step] = component.split('/');
  const frequency = Math.floor(limit / Number(step));
  return Array.from({ length: frequency }, (_, index) => index * Number(step));
};

export const clean = (rule: string) => rule.trim().split(' ').filter(negate(isEmpty));
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_complex_object_getters_as_opaque_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
let cached: string[];

const router = {
  get routes(): string[] {
    if (!cached) {
      cached = Object.values({ a: 'route' });
    }

    return cached;
  },
};
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_asserted_arrow_array_callbacks() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const values = [{ type: 'dynamiczone' }, { type: 'text' }];
const has = values.some(
  (({ type }: { type: string }) => type === 'dynamiczone' || type === 'component') as any
);
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_imported_value_alias_const_references() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { createId } from '@paralleldrive/cuid2';

export const createDocumentId = createId;

const attribute = {
  documentId: { type: 'string', default: createDocumentId },
};
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_computed_member_object_keys() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function model(identifiers: any, entityId: string) {
  return {
    [identifiers.ID_COLUMN]: { type: 'increments' },
    [entityId]: { type: 'integer' },
  };
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_truthy_dynamic_index_filter_callbacks() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const model: { attributes: Record<string, { type: string }> } = {
  attributes: { documentId: { type: 'string' } },
};

const columns = ['documentId', 'locale', 'publishedAt'].filter((name) => model.attributes[name]);
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_nested_function_self_property_references() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function createFetch() {
  function strapiFetch(url: string) {
    const options = {
      ...(strapiFetch.dispatcher ? { dispatcher: strapiFetch.dispatcher } : {}),
    };

    return options;
  }

  strapiFetch.dispatcher = {};
  return strapiFetch;
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_function_expression_captures_and_qualified_interface_extends() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import http from 'http';

export interface Server extends http.Server {
  destroy: () => Promise<void>;
}

function create(koaApp: any) {
  let handler: http.RequestListener;
  const listener: http.RequestListener = function handleRequest(req, res) {
    if (!handler) {
      handler = koaApp.callback();
    }

    return handler(req, res);
  };

  const server: Server = http.createServer({}, listener);
  if (!server.listening) {
    return listener;
  }

  return listener;
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_unannotated_catch_error_property_guards() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function resolve(resolve: string) {
  try {
    return require.resolve(resolve);
  } catch (error) {
    if (error instanceof Error && 'code' in error && error.code === 'MODULE_NOT_FOUND') {
      return resolve;
    }

    throw error;
  }
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn local_unknown_callable_shadows_same_named_item() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const run = () => {
  const { get } = ({} as any);
  return get();
};

const get = (featureName: string) => featureName;
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn preserves_callable_return_for_erased_function_surface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import type { Modules } from '@strapi/types';

export const createStrapiFetch = (): Modules.Fetch.Fetch => {
  function strapiFetch(url: RequestInfo | URL, options?: RequestInit) {
    return fetch(url, options);
  }

  return strapiFetch;
};

async function readTrial(): Promise<unknown> {
  const silentFetch = createStrapiFetch();
  const res = await silentFetch('https://example.com', { method: 'GET' });
  return res.json();
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn accepts_assignable_arrow_return_annotations() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function use<T, R>(items: readonly T[], fn: (item: T) => R): R {
  return fn(items[0]);
}

const stringValue = use(
  [
    { a: "cat", b: 123 },
    { a: "dog", b: 456 },
  ] as const,
  (x): string => x.a,
);

const numberValue = use(
  [
    { a: "cat", b: 123 },
    { a: "dog", b: 456 },
  ] as const,
  (x): number => x.b,
);

const unionValue = use(
  [
    { a: "cat", b: 123 },
    { a: "dog", b: 456 },
  ] as const,
  (x): number | string => x.b,
);
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_untyped_map_and_erased_map_mutations() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const pairs = new Map();

function remember(rawKey: unknown, rawValue: unknown) {
  pairs.set(rawKey, rawValue);
  pairs.delete(rawKey);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_from_collection_and_unknown_sources() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function keysFromMap(input: Map<string, number>, opaque: unknown) {
  const keys = Array.from(input.keys());
  const anyItems = Array.from(opaque);
  return keys.length + anyItems.length;
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_erased_union_callable_and_nullish_fallback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type MaybeBuilder = unknown | ((table: string) => unknown);

function run(trx: MaybeBuilder, maybeName: unknown) {
  const tableName = maybeName ?? 'articles';
  return trx(tableName as string);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_any_tuple_elements_and_erased_number_casts() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type Row = [string, any];

function read(row: Row, raw: { value: unknown }) {
  return Number(raw.value) + row.length;
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_map_from_existing_entries_and_boolean_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function clone(input: Map<string, number>, values: unknown[]) {
  const copied = new Map(input.entries());
  return values.filter(Boolean).length + copied.size;
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_zero_arg_callbacks_and_opaque_reduce_receiver() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function run(items: unknown) {
  const count = (items as unknown).reduce(() => 0, 0);
  return [1, 2].some(() => true) ? count : 0;
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_dynamic_in_checks_and_erased_math_min() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function pick(keys: unknown[], row: unknown, raw: unknown) {
  return keys.filter((key) => key in row).length + Math.min(raw, 10);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_push_spread_and_callback_nullish_coalesce() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function collect(batches: Array<Array<string | null>>) {
  const out: Array<string | null> = [];
  batches.forEach((batch) => {
    out.push(...batch.map((value) => value ?? 'draft'));
  });
  return out;
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_for_await_of_as_async_iterable_loop() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
async function run(source: unknown) {
  for await (const batch of source) {
    await Promise.resolve(batch);
  }
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_for_await_of_future_batches_with_record_indexing() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function batches(): Promise<Array<Array<string>>> {
  return Promise.resolve([['a']]);
}

async function run(records: Record<string, string>) {
  for await (const batch of batches()) {
    const first = batch[0];
    records[first];
  }
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_erased_none_index_access_as_unknown() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function run(key: string) {
  const erased = undefined as unknown as Record<string, string>;
  return erased[key];
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_strapi_async_map_with_options() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { async } from '@strapi/utils';

async function run(batch: Array<{ documentId: string; locale: string }>) {
  const discardDraft = async (entry: { documentId: string; locale: string }) => entry.documentId;
  await async.map(batch, discardDraft, { concurrency: 10 });
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_buffer_to_string_with_encoding_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function run(bytes: unknown) {
  return bytes.toString('hex');
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_class_method_reference_for_bind_assignment() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
class Manager {
  generateSessionId(): string {
    return 'id';
  }
}

function wire(api: { generateSessionId?: () => string }, manager: Manager) {
  api.generateSessionId = manager.generateSessionId.bind(manager);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_new_from_destructured_import_object_member() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { errors } from '@strapi/utils';

const { ValidationError } = errors;

function run(message: string) {
  throw new ValidationError(message);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_class_method_signature_with_destructured_parameter() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
interface Event {
  event: string;
  info: unknown;
}

class Runner {
  async executeListener({ event, info }: Event): Promise<void> {
    event;
    info;
  }
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_abort_signal_timeout_global() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function run() {
  return AbortSignal.timeout(10000);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_map_for_each_statement_receiver() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function run(items: Map<string, string[]>) {
  items.forEach((values, key) => {
    const filtered = values.filter((value) => value !== key);
    if (filtered.length === 0) {
      items.delete(key);
    } else {
      items.set(key, filtered);
    }
  });
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_concat_on_array_is_array_conditional_receiver() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function run(attr: { enum: unknown }) {
  return (Array.isArray(attr.enum) ? attr.enum : [attr.enum]).concat(null as any);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_spread_from_optional_erased_path() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function run(metas: { componentContext?: { pathToComponent?: string[] } }, name: string) {
  return [
    ...(metas?.componentContext?.pathToComponent ?? []),
    name,
  ];
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_callback_capturing_destructured_function_parameter() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function run({ model }: { model: { attributes: Record<string, string> } }, data: string[]) {
  return data.reduce((out, name) => {
    out[name] = model.attributes[name];
    return out;
  }, {} as Record<string, string>);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_opaque_method_function_callback_without_body_capture() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function run(model: { uid: string }, schema: unknown) {
  return schema.test('relations-test', 'check relations', async function validate(data: unknown) {
    return model.uid === String(data);
  });
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_callback_conditional_object_or_value_branch() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function run(source: Array<object | number>) {
  return source.map((value) => ({
    id: typeof value === 'object' ? value.id : value,
  }));
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_stacked_switch_case_with_block_break() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function run(kind: string) {
  let value = 0;
  switch (kind) {
    case 'relation':
    case 'media': {
      value = 1;
      break;
    }
    default:
      break;
  }
  return value;
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_call_through_asserted_function_callee() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function run<T>(schemaOrFactory: T | ((value: string) => T)): T {
  if (typeof schemaOrFactory === 'function') {
    return (schemaOrFactory as (value: string) => T)('z');
  }
  return schemaOrFactory;
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_push_erased_structural_item() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type Entry = { param: string; schema: { name: string }; matchRoute?: unknown };

function run(entries: Entry[], schema: unknown, matchRoute: unknown) {
  entries.push({ param: 'sort', schema, matchRoute });
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_callback_object_literal_with_opaque_spread() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function run(routes: Array<{ path: string }>, prefix: string) {
  return routes.map((route) => ({
    ...route,
    path: `${prefix}${route.path}`,
  }));
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unannotated_rest_arrow_closure_as_unknown_array() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type Visitor = (...args: unknown[]) => void;
declare const first: Visitor;
declare const second: Visitor;
declare function use(visitor: Visitor): void;

use((...args) => {
  first(...args);
  second(...args);
});
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_forward_typed_arrow_const_inside_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type Fn = (value: unknown) => unknown;
const outer = () => {
  const visit: Fn = (value: unknown) => {
    if (Array.isArray(value)) {
      return value.map((entry) => visit(entry));
    }
    return value;
  };
  return visit;
};
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_calls_to_const_items_with_callable_surface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function wrap<T extends (...args: any[]) => any>(value: T): T {
  return value;
}

export const validate = wrap((value: unknown, extra: string[]) => value);

export const run = (value: unknown) => {
  return validate(value, ['a']);
};
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_const_callable_items_inside_callbacks() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function wrap<T extends (...args: any[]) => any>(value: T): T {
  return value;
}

export const validate = wrap((value: unknown) => value);

export const run = (values: unknown[]) => {
  return values.map((value) => validate(value));
};
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_namespace_const_callable_member_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function wrap<T extends (...args: any[]) => any>(value: T): T {
  return value;
}

const validate = wrap((value: unknown) => value);
const validators = { validate };

export const run = (value: unknown) => {
  return validators.validate(value);
};
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_optional_interface_methods_as_optional_function_fields() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
interface Options {
  validator?(config: unknown): void;
  handler(...args: any[]): any;
}

export const run = (options: Options, config: unknown) => {
  const { validator, handler } = options;
  if (validator) {
    validator(config);
  }
  return handler(config);
};
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_imported_static_string_split_helper() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import _ from "lodash";

export const run = (value: string) => _.split(value, '/');
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_imported_static_array_join_helper() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import _ from "lodash";

export const run = (values: string[]) => _.join(values, '/');
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_global_string_constructor_as_callback_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const castIncludes = (arr: unknown[], val: unknown, cast: (val: unknown) => unknown): boolean =>
  arr.map((val) => cast(val)).includes(cast(val));

export const includesString = (arr: unknown[], val: unknown) => castIncludes(arr, val, String);
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_empty_callback_arrays_with_asserted_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const keysDeep = (obj: object): string[] =>
  Object.keys(obj).reduce((acc, key) => acc, [] as string[]);
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_imported_static_array_concat_helper() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import _ from "lodash";

export const run = (left: string[], right: string[]) => _.concat(left, right);
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_imported_static_array_concat_helper_with_erased_right() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import _ from "lodash";

declare const right: unknown;
export const run = (left: string[]) => _.concat(left, right);
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_concat_with_erased_left_and_concrete_list_right() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import _ from "lodash";

declare const left: unknown[];
declare const right: string[];
export const run = () => _.concat(left, right);
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_print_value_stdlib_surfaces() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const symbolToString = typeof Symbol !== 'undefined' ? Symbol.prototype.toString : () => '';
const errorToString = Error.prototype.toString;

function printSimpleValue(val: unknown, quoteStrings = false) {
  if (typeof val === 'function') return `[Function ${val.name || 'anonymous'}]`;
  if (typeof val === 'symbol') return symbolToString.call(val);
  if (val instanceof Error) return errorToString.call(val);
  const v = val as Date;
  return Number.isNaN(v.getTime()) ? `${v}` : v.toISOString();
}

function printValue(value: unknown, quoteStrings: boolean) {
  return JSON.stringify(
    value,
    function replacer(key, value) {
      const result = printSimpleValue(this[key], quoteStrings);
      if (result !== null) return result;
      return value;
    },
    2
  );
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_in_check_on_generic_object_receiver() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const isObjKey = <T extends object>(key: string | symbol | number, obj: T): key is keyof T => {
  return key in obj;
};
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_forward_class_construction_and_imported_namespace_base() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import * as yup from "yup";

export const make = (): InstanceType<typeof StrapiIDSchema> => new StrapiIDSchema();

export class StrapiIDSchema extends yup.MixedSchema {
  constructor() {
    super({ type: 'strapiID' });
  }
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_imported_static_member_as_array_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import _ from "lodash";

export const run = (value: object) => Object.values(value).every(_.isFunction);
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_nested_static_member_as_array_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
declare const providers: { condition: { get: (value: string) => unknown } };

export const run = (values: string[]) => values.map(providers.condition.get);
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_imported_prop_factory_as_array_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import _ from "lodash/fp";

export const run = (values: Array<{ result: unknown }>) => values.map(_.prop('result'));
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_imported_functional_map_factory() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import _ from "lodash/fp";

const pickResults = _.map(_.prop('result'));
export const run = (values: Array<{ result: unknown }>) => pickResults(values).filter(_.isObject);
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_call_expression_factory_as_array_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
declare const resultPropEq: (value: boolean) => (entry: { result: unknown }) => boolean;

export const run = (values: Array<{ result: unknown }>) => values.every(resultPropEq(false));
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_global_string_constructor_as_array_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export const run = (values: unknown[]) => values.map(String);
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn falls_back_for_arrow_callback_side_effect_if_blocks() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
declare function mergeWith(
  callback: (objValue: unknown, srcValue: unknown) => unknown,
  left: unknown,
  right: unknown
): unknown;

export const run = (left: unknown, right: unknown) =>
  mergeWith((objValue, srcValue) => {
    if (Array.isArray(objValue)) {
      return Array.from(new Set(objValue.concat(srcValue)));
    }
    return undefined;
  }, left, right);
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_parenthesized_union_tuple_element_types() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type Fragment = { id: string };
type Nested = { path: string };
type Params = [string, (Fragment | Nested)];
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_concat_on_optional_list_after_array_guard() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
interface Permission {
  conditions?: string[];
}

export const run = (condition: string, permission: Permission) => {
  const { conditions } = permission;
  return Array.isArray(conditions) ? conditions.concat(condition) : [condition];
};
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_this_return_types_in_interfaces_as_erased_self() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export interface Registry {
  add(path: string): this;
  has(path: string): boolean;
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_destructured_interface_method_parameters() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
interface Event {
  event: string;
  info: Record<string, unknown>;
}

export interface Runner {
  executeListener({ event, info }: Event): Promise<void>;
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn allows_interface_extends_object_type_alias() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type Generic = {
  [method: string | number | symbol]: unknown;
};

export interface RouterConfig extends Generic {
  find?: unknown;
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}
