//! Conservative source-graph metaprogramming detection.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::HostLanguage;

/// Source graph input used by the centralized detector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleInput {
    /// Stable source graph module key.
    pub path: String,
    /// Source language.
    pub language: HostLanguage,
    /// Source text after graph resolution.
    pub source: String,
    /// Resolved imports and re-exports by module key.
    pub dependencies: Vec<String>,
}

/// Why a module requires host-runtime specialization.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionReason {
    /// Python decorator syntax is present.
    PythonDecorator,
    /// Python metaclass syntax is present.
    PythonMetaclass,
    /// A Python descriptor or class creation hook is defined.
    PythonDefinitionHook,
    /// A top-level Python definition is assigned from a callable result.
    PythonFactoryBinding,
    /// TypeScript standard decorator syntax is present.
    TypeScriptDecorator,
    /// The module imports or re-exports a candidate module.
    DependencyPropagation {
        /// Candidate dependency that caused propagation.
        dependency: String,
    },
}

/// Detection result for one candidate module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleDetection {
    /// Stable source graph module key.
    pub path: String,
    /// Source language.
    pub language: HostLanguage,
    /// Ordered, deduplicated reasons for candidacy.
    pub reasons: Vec<DetectionReason>,
}

/// Detects definition-time metaprogramming and propagates candidacy through
/// resolved imports and re-exports.
///
/// Detection is intentionally conservative: false positives cost one
/// specialization batch, while false negatives could silently skip observable
/// definition-time behavior.
#[must_use]
pub fn detect_specialization_modules(modules: &[ModuleInput]) -> Vec<ModuleDetection> {
    let known_paths = modules
        .iter()
        .map(|module| module.path.as_str())
        .collect::<BTreeSet<_>>();
    let mut detected = BTreeMap::<String, (HostLanguage, BTreeSet<DetectionReason>)>::new();

    for module in modules {
        let reasons = direct_reasons(module);
        if !reasons.is_empty() {
            detected.insert(module.path.clone(), (module.language, reasons));
        }
    }

    loop {
        let candidate_paths = detected.keys().cloned().collect::<BTreeSet<_>>();
        let mut changed = false;
        for module in modules {
            for dependency in &module.dependencies {
                if known_paths.contains(dependency.as_str()) && candidate_paths.contains(dependency)
                {
                    let entry = detected
                        .entry(module.path.clone())
                        .or_insert_with(|| (module.language, BTreeSet::new()));
                    changed |= entry.1.insert(DetectionReason::DependencyPropagation {
                        dependency: dependency.clone(),
                    });
                }
            }
        }
        if !changed {
            break;
        }
    }

    detected
        .into_iter()
        .map(|(path, (language, reasons))| ModuleDetection {
            path,
            language,
            reasons: reasons.into_iter().collect(),
        })
        .collect()
}

/// Finds direct language-specific metaprogramming markers.
fn direct_reasons(module: &ModuleInput) -> BTreeSet<DetectionReason> {
    match module.language {
        HostLanguage::Python => python_reasons(&module.source),
        HostLanguage::TypeScript => typescript_reasons(&module.source),
    }
}

/// Conservatively identifies Python definition-time behavior.
fn python_reasons(source: &str) -> BTreeSet<DetectionReason> {
    let mut reasons = BTreeSet::new();
    for line in source.lines().map(str::trim_start) {
        if line.starts_with('@') {
            reasons.insert(DetectionReason::PythonDecorator);
        }
        if line.starts_with("class ") && line.contains("metaclass") && line.contains('=') {
            reasons.insert(DetectionReason::PythonMetaclass);
        }
        if line.starts_with("def __init_subclass__(")
            || line.starts_with("def __set_name__(")
            || line.starts_with("def __get__(")
            || line.starts_with("def __set__(")
        {
            reasons.insert(DetectionReason::PythonDefinitionHook);
        }
        if is_python_factory_binding(line) {
            reasons.insert(DetectionReason::PythonFactoryBinding);
        }
    }
    reasons
}

/// Returns whether a Python line is a conservative exported factory binding.
fn is_python_factory_binding(line: &str) -> bool {
    if line.starts_with([' ', '\t', '#', '_']) || !line.contains('=') {
        return false;
    }
    let Some((raw_name, raw_value)) = line.split_once('=') else {
        return false;
    };
    let name = raw_name.trim();
    let value = raw_value.trim();
    !name.is_empty()
        && name.chars().all(|character| {
            character == '_' || character.is_ascii_alphanumeric() || character == ':'
        })
        && value.contains('(')
        && value.ends_with(')')
        && !value.starts_with(['[', '{', '('])
}

/// Conservatively identifies TypeScript standard decorator syntax.
fn typescript_reasons(source: &str) -> BTreeSet<DetectionReason> {
    let has_decorator = source.lines().map(str::trim_start).any(|line| {
        line.starts_with('@')
            || line
                .split_whitespace()
                .any(|word| word.starts_with('@') && word.len() > 1)
    });
    if has_decorator {
        BTreeSet::from([DetectionReason::TypeScriptDecorator])
    } else {
        BTreeSet::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(
        path: &str,
        language: HostLanguage,
        source: &str,
        dependencies: &[&str],
    ) -> ModuleInput {
        ModuleInput {
            path: path.to_owned(),
            language,
            source: source.to_owned(),
            dependencies: dependencies.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn detects_python_definition_hooks_and_propagates_dependents() -> Result<(), String> {
        let modules = vec![
            module(
                "models.py",
                HostLanguage::Python,
                "class Field:\n    def __set_name__(self, owner, name): pass",
                &[],
            ),
            module(
                "api.py",
                HostLanguage::Python,
                "from models import Field",
                &["models.py"],
            ),
        ];
        let detected = detect_specialization_modules(&modules);
        if detected.len() != 2 {
            return Err("candidate imports did not propagate".to_owned());
        }
        let api = detected
            .iter()
            .find(|detection| detection.path == "api.py")
            .ok_or_else(|| "api.py was not detected".to_owned())?;
        if !api.reasons.iter().any(|reason| {
            matches!(
                reason,
                DetectionReason::DependencyPropagation { dependency } if dependency == "models.py"
            )
        }) {
            return Err("the propagated dependency was not recorded".to_owned());
        }
        Ok(())
    }

    #[test]
    fn detects_typescript_member_decorators() -> Result<(), String> {
        let detected = detect_specialization_modules(&[module(
            "model.ts",
            HostLanguage::TypeScript,
            "class Model {\n  @bound accessor value = 1;\n}",
            &[],
        )]);
        let model = detected
            .first()
            .ok_or_else(|| "model.ts was not detected".to_owned())?;
        if model.reasons != vec![DetectionReason::TypeScriptDecorator] {
            return Err("standard member decorators were not detected".to_owned());
        }
        Ok(())
    }

    #[test]
    fn ignores_ordinary_undecorated_modules() {
        let detected = detect_specialization_modules(&[module(
            "plain.py",
            HostLanguage::Python,
            "def add(a: int, b: int) -> int:\n    return a + b",
            &[],
        )]);
        assert!(
            detected.is_empty(),
            "ordinary modules must avoid host startup"
        );
    }
}
