//! Test-tier detection: whether a program is a test module, and which vitest
//! globals it reaches without importing them.

use std::collections::HashSet;
use crate::test_support;
use crate::lowering::js_text::js_str_text;
use oxc::ast::ast::{BindingPattern, Program, Statement};
use super::super::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Return whether a source path is a type-test module (`*.test-d.ts` or `*.test.ts`).
    pub(in crate::lowering) fn is_declaration_type_test_path(path: &str) -> bool {
        path.ends_with(".test-d.ts") || path.ends_with(".test.ts")
    }

    /// Put the Vitest API names in scope for a test module that does not import
    /// them.
    ///
    /// Vitest's `globals: true` publishes `describe`/`it`/`test`/`expect` and
    /// the lifecycle hooks as globals inside test files, and most projects that
    /// enable it stop importing them (87 of Hono's 101 `*.test.ts` files do).
    /// Without this, `describe(...)` in such a file is an ordinary unresolved
    /// call and `expect(x).toBe(y)` lowers through the erased dynamic-call path
    /// — an assertion that cannot fail — so the file emitted no `#[test]` and
    /// reported a pass either way.
    ///
    /// The names are the host module's, so this is the same model as the import
    /// spelling and not a per-project rule. A name the file binds itself — an
    /// import, or its own top-level declaration — keeps its own meaning: the
    /// global is only the fallback vitest makes it.
    pub(super) fn mark_ambient_test_builtins(&mut self, program: &Program<'_>) {
        for name in ambient_test_builtin_names(program, &|name| self.imports.is_imported_binding(name)) {
            self.imports.mark_test_builtin(name.to_owned());
        }
    }

    /// Collect the names a program binds at its top level.
    ///
    /// Only the forms that could shadow a vitest global matter here: a
    /// `function`/`class` declaration and the identifier bindings of a
    /// `const`/`let`/`var` statement (a destructuring pattern that rebinds
    /// `expect` is not a shape any suite writes, and missing it only means the
    /// global is preferred, which is what an un-shadowed file wants anyway).
    pub(in crate::lowering) fn program_top_level_binding_names(program: &Program<'_>) -> HashSet<String> {
        let mut names = HashSet::new();
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    if let Some(id) = &function.id {
                        names.insert(id.name.as_str().to_owned());
                    }
                }
                Statement::ClassDeclaration(class) => {
                    if let Some(id) = &class.id {
                        names.insert(id.name.as_str().to_owned());
                    }
                }
                Statement::VariableDeclaration(declaration) => {
                    for declarator in &declaration.declarations {
                        if let BindingPattern::BindingIdentifier(binding) = &declarator.id {
                            names.insert(binding.name.as_str().to_owned());
                        }
                    }
                }
                _ => {}
            }
        }
        names
    }

    /// Return whether a program belongs to the test tier.
    ///
    /// Either the source path is a test file, or the module imports a
    /// Vitest-compatible framework. Both spellings appear in the corpora Smelt
    /// tracks, and the answer is needed before any body is lowered.
    pub(super) fn is_test_tier_program(&self, program: &Program<'_>) -> bool {
        is_test_tier(&self.path, program)
    }
}

/// Whether the program at `path` belongs to the test tier; see
/// [`ModuleBuilder::is_test_tier_program`].
pub(in crate::lowering) fn is_test_tier(path: &str, program: &Program<'_>) -> bool {
    if ModuleBuilder::is_declaration_type_test_path(path) || path.ends_with(".spec.ts") || path.ends_with(".test-d.tsx")
    {
        return true;
    }
    program.body.iter().any(|statement| {
        matches!(statement, Statement::ImportDeclaration(import)
            if test_support::is_vitest_compatible_module(&js_str_text(import.source.value)))
    })
}

/// The vitest API names a test-tier module reaches as globals: every one it
/// neither imports (`is_imported`) nor declares at its top level. This one
/// decision is what lowering models ([`ModuleBuilder::mark_ambient_test_builtins`])
/// and what the transpiler declares to the type checker
/// ([`crate::ambient_test_globals`]), so the two cannot disagree.
pub(in crate::lowering) fn ambient_test_builtin_names(
    program: &Program<'_>,
    is_imported: &dyn Fn(&str) -> bool,
) -> Vec<&'static str> {
    let declared = ModuleBuilder::program_top_level_binding_names(program);
    test_support::vitest_builtin_names()
        .iter()
        .copied()
        .filter(|name| !is_imported(name) && !declared.contains(*name))
        .collect()
}
