//! Bun's `int32` (`import type { int32 } from "bun"`): slot typing, store
//! proofs and the TS100031–TS100034 diagnostics.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631), the "32-bit
//! integers" section of `docs/bundler/executables.mdx`. Each acceptance test
//! pins the HIR slot type; each rejection test pins the exact message text,
//! which follows Bun's wording.

use super::*;
use smelt_hir::IntWidth;

/// The `Cursor`/`advance` example from Bun's documentation, verbatim.
const CURSOR: &str = "import type { int32 } from \"bun\";\n\nclass Cursor {\n  line: int32 = 0;\n  column: int32 = 0;\n}\n\nfunction advance(cursor: Cursor, by: int32): int32 {\n  cursor.column = (cursor.column + by) | 0;\n  return cursor.column;\n}\n\nconst c = new Cursor();\nconsole.log(advance(c, 3));\n";

/// The single diagnostic of a failed lowering.
fn single_error(source: &str) -> Result<SmeltError, String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(source, &mut ctx)?;
    match errors.as_slice() {
        [error] => Ok(error.clone()),
        _ => Err(format!("expected exactly one diagnostic, got {errors:?}")),
    }
}

/// Asserts that lowering `source` fails with `code` and exactly `message`.
fn assert_rejected(source: &str, code: &str, message: &str) -> Result<(), String> {
    let error = single_error(source)?;
    ensure_eq!(error.code, code);
    ensure_eq!(error.message.as_str(), message);
    ensure_eq!(error.category, smelt_stdlib::DiagnosticCategory::TypeConstraint);
    Ok(())
}

/// The type of the field `field` of the class or interface named `owner`.
fn field_type(ctx: &HirCtx, owner: &str, field: &str) -> Option<Type> {
    ctx.krate.items.iter().find_map(|item| {
        let fields = match item {
            Item::Class(class) if ctx.krate.symbols.source(class.name) == Some(owner) => &class.fields,
            Item::Interface(interface) if ctx.krate.symbols.source(interface.name) == Some(owner) => {
                &interface.fields
            }
            _ => return None,
        };
        fields
            .iter()
            .find(|candidate| ctx.krate.symbols.source(candidate.name) == Some(field))
            .and_then(|candidate| ctx.krate.types.get(candidate.ty).cloned())
    })
}

/// The function item named `name`.
fn function_named<'a>(ctx: &'a HirCtx, name: &str) -> Option<&'a Function> {
    ctx.krate.items.iter().find_map(|item| match item {
        Item::Function(function) if ctx.krate.symbols.source(function.name) == Some(name) => {
            Some(function)
        }
        _ => None,
    })
}

/// Lowers `source` and asserts it is accepted.
fn accepted(source: &str) -> Result<HirCtx, String> {
    let mut ctx = HirCtx::new();
    lower_ok(source, &mut ctx)?;
    Ok(ctx)
}

const I32: Type = Type::FixedInt(IntWidth::I32);

#[test]
fn bun_cursor_example_types_fields_params_and_result_as_i32() -> Result<(), String> {
    let ctx = accepted(CURSOR)?;
    ensure_eq!(field_type(&ctx, "Cursor", "line"), Some(I32));
    ensure_eq!(field_type(&ctx, "Cursor", "column"), Some(I32));
    let advance = function_named(&ctx, "advance").ok_or("no advance")?;
    ensure_eq!(ctx.krate.types.get(advance.params[1].ty), Some(&I32));
    ensure_eq!(ctx.krate.types.get(advance.return_ty), Some(&I32));
    // `cursor` is a class, not an int32.
    ensure!(ctx.krate.types.get(advance.params[0].ty) != Some(&I32));
    Ok(())
}

#[test]
fn an_aliased_import_keeps_the_meaning() -> Result<(), String> {
    let ctx = accepted(
        "import type { int32 as i32 } from \"bun\";\ntype Count = i32;\nclass Box {\n  n: i32 = 1;\n  m: Count = 2;\n}\nconsole.log(new Box().n);\n",
    )?;
    ensure_eq!(field_type(&ctx, "Box", "n"), Some(I32));
    ensure_eq!(field_type(&ctx, "Box", "m"), Some(I32));
    Ok(())
}

#[test]
fn a_local_int32_alias_is_a_number() -> Result<(), String> {
    let ctx = accepted(
        "type int32 = number;\nclass Box {\n  n: int32 = 1.5;\n}\nconsole.log(new Box().n);\n",
    )?;
    ensure_eq!(field_type(&ctx, "Box", "n"), Some(Type::Float));
    Ok(())
}

#[test]
fn int32_inside_another_type_is_a_number() -> Result<(), String> {
    let ctx = accepted(
        "import type { int32 } from \"bun\";\ninterface Shape {\n  sizes: int32[];\n  maybe: int32 | undefined;\n  opt?: int32;\n}\nconst s: Shape = { sizes: [1.5], maybe: 2.5, opt: 3 };\nconsole.log(s.sizes[0], s.maybe, s.opt);\n",
    )?;
    let sizes = field_type(&ctx, "Shape", "sizes").ok_or("no sizes")?;
    let Type::List(item) = sizes else {
        return Err(format!("sizes is {sizes:?}"));
    };
    ensure_eq!(ctx.krate.types.get(item), Some(&Type::Float));
    let maybe = field_type(&ctx, "Shape", "maybe").ok_or("no maybe")?;
    let Type::Optional(inner) = maybe else {
        return Err(format!("maybe is {maybe:?}"));
    };
    ensure_eq!(ctx.krate.types.get(inner), Some(&Type::Float));
    // An optional property is an int32 or undefined.
    let opt = field_type(&ctx, "Shape", "opt").ok_or("no opt")?;
    let Type::Optional(inner) = opt else {
        return Err(format!("opt is {opt:?}"));
    };
    ensure_eq!(ctx.krate.types.get(inner), Some(&I32));
    Ok(())
}

#[test]
fn every_proven_form_of_buns_table_is_accepted() -> Result<(), String> {
    accepted(
        "import type { int32 } from \"bun\";\nfunction f(a: number, b: number, text: string, end: int32): int32 {\n  let x: int32 = 7;\n  x = -1;\n  x = 2 ** 31 - 1;\n  x = a | b;\n  x = a & b;\n  x = a ^ b;\n  x = ~a;\n  x = a << b;\n  x = a >> b;\n  x = a >>> 1;\n  x = Math.imul(a, b);\n  x = Math.clz32(a);\n  x = text.length;\n  x = text.indexOf(\"z\");\n  x = (a & 255) * 3 + 1;\n  x = (a & 255) % 7;\n  x = a as int32;\n  x = end;\n  for (let i: int32 = 0; i < end; i++) {\n    x = i + 1;\n  }\n  let j: int32 = 0;\n  while (j < text.length) {\n    j++;\n  }\n  return x + 0 > 3 ? x : j;\n}\nconsole.log(f(1, 2, \"abc\", 4));\n",
    )?;
    Ok(())
}

#[test]
fn as_int32_is_bitwise_or_zero() -> Result<(), String> {
    let ctx = accepted(
        "import type { int32 } from \"bun\";\nconst n: number = 3.7;\nconst k = n as int32;\nconsole.log(k);\n",
    )?;
    let body = ctx.krate.bodies.last().ok_or("no body")?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::BinOp {
                op: BinOp::BitOr,
                ..
            }
        )),
        "`as int32` must lower to `| 0`"
    );
    Ok(())
}

#[test]
fn unproven_sum_reports_buns_message() -> Result<(), String> {
    assert_rejected(
        "import type { int32 } from \"bun\";\nclass Cursor {\n  column: int32 = 0;\n}\nfunction advance(cursor: Cursor, by: int32): void {\n  cursor.column = cursor.column + by;\n}\nadvance(new Cursor(), 1);\n",
        "smelt::int32-unproven",
        "Property 'column' of 'Cursor' is an 'int32', and the compiler cannot prove that 'cursor.column + by' is one: it can be anything from -4294967296 to 4294967294. Write '(cursor.column + by) | 0'.",
    )
}

#[test]
fn a_number_is_not_an_int32() -> Result<(), String> {
    assert_rejected(
        "import type { int32 } from \"bun\";\nfunction f(n: number): int32 {\n  return n;\n}\nconsole.log(f(1));\n",
        "smelt::int32-unproven",
        "The result of 'f' is an 'int32', and the compiler cannot prove that 'n' is one: all that is known is that it is a 'number', which can have a fraction, be out of range, or be NaN or -0. Write 'n | 0'.",
    )
}

#[test]
fn a_product_suggests_math_imul() -> Result<(), String> {
    assert_rejected(
        "import type { int32 } from \"bun\";\nfunction f(a: int32, b: int32): void {\n  const p: int32 = a * b;\n  console.log(p);\n}\nf(2, 3);\n",
        "smelt::int32-unproven",
        "'p' is an 'int32', and the compiler cannot prove that 'a * b' is one: a product can be too large to be exact, and it can be -0. Write 'Math.imul(a, b)'.",
    )
}

#[test]
fn a_division_is_not_proven() -> Result<(), String> {
    assert_rejected(
        "import type { int32 } from \"bun\";\nfunction f(a: int32): void {\n  const h: int32 = a / 2;\n  console.log(h);\n}\nf(4);\n",
        "smelt::int32-unproven",
        "'h' is an 'int32', and the compiler cannot prove that 'a / 2' is one: a division can leave a fraction. Write '(a / 2) | 0'.",
    )
}

#[test]
fn an_array_length_can_exceed_int32() -> Result<(), String> {
    assert_rejected(
        "import type { int32 } from \"bun\";\nfunction f(xs: number[]): void {\n  const n: int32 = xs.length;\n  console.log(n);\n}\nf([1]);\n",
        "smelt::int32-unproven",
        "'n' is an 'int32', and the compiler cannot prove that 'xs.length' is one: it can be anything from 0 to 4294967295. Write 'xs.length | 0'.",
    )
}

#[test]
fn an_unsigned_shift_can_exceed_int32() -> Result<(), String> {
    assert_rejected(
        "import type { int32 } from \"bun\";\nfunction f(a: number): void {\n  const n: int32 = a >>> 0;\n  console.log(n);\n}\nf(1);\n",
        "smelt::int32-unproven",
        "'n' is an 'int32', and the compiler cannot prove that 'a >>> 0' is one: it can be anything from 0 to 4294967295. Write '(a >>> 0) | 0'.",
    )
}

#[test]
fn an_increment_with_no_limit_in_sight_is_not_proven() -> Result<(), String> {
    assert_rejected(
        "import type { int32 } from \"bun\";\nfunction f(): void {\n  let i: int32 = 0;\n  i++;\n  console.log(i);\n}\nf();\n",
        "smelt::int32-unproven",
        "'i' is an 'int32', and the compiler cannot prove that 'i++' is one: it can be anything from -2147483647 to 2147483648. Write 'i = (i + 1) | 0'.",
    )
}

#[test]
fn an_increment_under_less_or_equal_is_not_proven() -> Result<(), String> {
    let error = single_error(
        "import type { int32 } from \"bun\";\nfunction f(end: int32): void {\n  for (let i: int32 = 0; i <= end; i++) {\n    console.log(i);\n  }\n}\nf(3);\n",
    )?;
    ensure_eq!(error.code, "smelt::int32-unproven");
    ensure!(error.message.starts_with("'i' is an 'int32'"), "{}", error.message);
    Ok(())
}

/// The loop test bounds the counter in the update only when the body never
/// writes the counter: here the body can set it to the maximum first.
#[test]
fn a_body_write_voids_the_loop_test_for_the_update() -> Result<(), String> {
    let error = single_error(
        "import type { int32 } from \"bun\";\nfunction f(end: int32): void {\n  for (let i: int32 = 0; i < end; i++) {\n    i = 2147483647;\n  }\n}\nf(3);\n",
    )?;
    ensure_eq!(error.code, "smelt::int32-unproven");
    ensure!(error.message.contains("'i++'"), "{}", error.message);
    Ok(())
}

/// A test proves nothing about a counter read after the counter was
/// written again in the same guarded region.
#[test]
fn a_read_after_the_increment_is_not_bounded_by_the_test() -> Result<(), String> {
    let error = single_error(
        "import type { int32 } from \"bun\";\nfunction f(end: int32): int32 {\n  let i: int32 = 0;\n  let x: int32 = 0;\n  while (i < end) {\n    i++;\n    x = i + 1;\n  }\n  return x;\n}\nf(3);\n",
    )?;
    ensure_eq!(error.code, "smelt::int32-unproven");
    ensure!(error.message.starts_with("'x' is an 'int32'"), "{}", error.message);
    Ok(())
}

/// An increment inside a loop nested under the test runs many times per
/// test, so the test does not bound it.
#[test]
fn an_increment_in_a_nested_loop_is_not_bounded_by_the_outer_test() -> Result<(), String> {
    let error = single_error(
        "import type { int32 } from \"bun\";\nfunction f(end: int32, k: number): int32 {\n  let i: int32 = 0;\n  while (i < end) {\n    let j = 0;\n    while (j < k) {\n      i++;\n      j++;\n    }\n  }\n  return i;\n}\nf(3, 2);\n",
    )?;
    ensure_eq!(error.code, "smelt::int32-unproven");
    ensure!(error.message.contains("'i++'"), "{}", error.message);
    Ok(())
}

#[test]
fn a_fraction_is_not_an_int32() -> Result<(), String> {
    assert_rejected(
        "import type { int32 } from \"bun\";\nconst x: int32 = 1.5;\nconsole.log(x);\n",
        "smelt::int32-not-int32",
        "'x' is an 'int32', and '1.5' is not: it has a fraction.",
    )
}

#[test]
fn two_to_the_31_is_not_an_int32() -> Result<(), String> {
    assert_rejected(
        "import type { int32 } from \"bun\";\nconst x: int32 = 2 ** 31;\nconsole.log(x);\n",
        "smelt::int32-not-int32",
        "'x' is an 'int32', and '2 ** 31' is not: an int32 is a whole number from -2147483648 to 2147483647.",
    )
}

#[test]
fn negative_zero_is_not_an_int32() -> Result<(), String> {
    assert_rejected(
        "import type { int32 } from \"bun\";\nconst x: int32 = -0;\nconsole.log(x);\n",
        "smelt::int32-not-int32",
        "'x' is an 'int32', and '-0' is not: it is -0.",
    )
}

#[test]
fn an_argument_is_a_store() -> Result<(), String> {
    assert_rejected(
        "import type { int32 } from \"bun\";\nfunction half(n: int32): number {\n  return n / 2;\n}\nconsole.log(half(2.5));\n",
        "smelt::int32-not-int32",
        "Parameter 'n' of 'half' is an 'int32', and '2.5' is not: it has a fraction.",
    )
}

#[test]
fn an_object_literal_property_is_a_store() -> Result<(), String> {
    assert_rejected(
        "import type { int32 } from \"bun\";\ninterface Point {\n  x: int32;\n}\nconst p: Point = { x: 0.5 };\nconsole.log(p.x);\n",
        "smelt::int32-not-int32",
        "Property 'x' of 'Point' is an 'int32', and '0.5' is not: it has a fraction.",
    )
}

#[test]
fn a_function_with_an_int32_parameter_used_as_a_value() -> Result<(), String> {
    assert_rejected(
        "import type { int32 } from \"bun\";\nfunction half(n: int32): number {\n  return n / 2;\n}\nconsole.log([2, 4].map(half));\n",
        "smelt::int32-function-as-value",
        "Parameter 'n' of 'half' is an 'int32', and the function is used as a value here, so the compiler cannot see every call. Call the function directly, or declare the parameter 'number' and convert it inside with '| 0'.",
    )
}

#[test]
fn a_member_that_is_int32_in_only_one_of_two_types() -> Result<(), String> {
    assert_rejected(
        "import type { int32 } from \"bun\";\ninterface Point {\n  x: number;\n}\nclass Pixel implements Point {\n  x: int32 = 0;\n}\nconsole.log(new Pixel().x);\n",
        "smelt::int32-differs",
        "A 'Pixel' is used as a 'Point', and 'x' is an 'int32' in only one of the two. Declare it 'int32' in both, or in neither.",
    )
}
