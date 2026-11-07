//! `smelt new`: scaffold a project directory (see `specs/cli.md`).
//!
//! The layout is `Smelt.toml`, `.gitignore`, `README.md` and one entry source
//! under `src/`. The manifest is the smallest one the parser accepts, so a new
//! project builds as soon as it is created.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// The language of a new project's entry source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryLanguage {
    /// `src/main.ts`.
    TypeScript,
    /// `src/main.py`.
    Python,
}

impl EntryLanguage {
    /// The entry's path inside the project.
    const fn entry(self) -> &'static str {
        match self {
            Self::TypeScript => "src/main.ts",
            Self::Python => "src/main.py",
        }
    }

    /// The entry's text: a program that greets by the project's name.
    fn source(self, name: &str) -> String {
        match self {
            Self::TypeScript => format!("console.log(\"Hello from {name}!\");\n"),
            Self::Python => format!("print(\"Hello from {name}!\")\n"),
        }
    }
}

/// Creates project `name` in `parent`, returning its directory.
///
/// `name` is a single directory name: a letter, then letters, digits, `-` or
/// `_`, so it is valid in the manifest and as the base of a crate name. An
/// existing non-empty directory of that name is an error; nothing is written.
pub(crate) fn new_project(parent: &Path, name: &str, language: EntryLanguage) -> Result<PathBuf, String> {
    validate_name(name)?;
    let root = parent.join(name);
    if fs::read_dir(&root).is_ok_and(|mut entries| entries.next().is_some()) {
        return Err(format!("`{}` already exists and is not empty", root.display()));
    }
    write_files(&root, name, language).map_err(|error| format!("cannot create `{}`: {error}", root.display()))?;
    Ok(root)
}

/// Rejects a name that is not one plain directory name.
fn validate_name(name: &str) -> Result<(), String> {
    let mut characters = name.chars();
    let starts_with_letter = characters.next().is_some_and(|first| first.is_ascii_alphabetic());
    if starts_with_letter && characters.all(|rest| rest.is_ascii_alphanumeric() || rest == '-' || rest == '_') {
        return Ok(());
    }
    Err(format!(
        "invalid project name `{name}`: use a letter followed by letters, digits, `-` or `_`"
    ))
}

/// Writes the project files under `root`.
fn write_files(root: &Path, name: &str, language: EntryLanguage) -> io::Result<()> {
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("Smelt.toml"), manifest(name, language))?;
    fs::write(root.join(".gitignore"), "dist/\n.smelt-cache/\n")?;
    fs::write(
        root.join("README.md"),
        format!(
            "# {name}\n\nA Smelt project: `{}` is transpiled to a Rust crate in `dist/`. \
             Run `smelt build` to generate and build it.\n",
            language.entry()
        ),
    )?;
    fs::write(root.join(language.entry()), language.source(name))
}

/// The project's `Smelt.toml`.
fn manifest(name: &str, language: EntryLanguage) -> String {
    let crate_name = name.replace('-', "_").to_ascii_lowercase();
    format!(
        "[project]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"{entry}\"]\n\n\
         [output]\ntarget = \"./dist\"\ncrate-name = \"{crate_name}\"\nbuild = true\n",
        entry = language.entry(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only one plain directory name is accepted.
    #[test]
    fn names_are_one_plain_directory_name() {
        for good in ["app", "my-app", "app_2"] {
            assert_eq!(validate_name(good), Ok(()), "{good}");
        }
        for bad in ["", "2app", "-app", "a/b", "../app", "a b", "app\""] {
            assert!(validate_name(bad).is_err(), "{bad:?} was accepted");
        }
    }

    /// The crate name is the project name in Rust's spelling.
    #[test]
    fn the_manifest_names_the_crate_in_snake_case() {
        let text = manifest("My-App", EntryLanguage::TypeScript);
        assert!(text.contains("crate-name = \"my_app\""), "{text}");
        assert!(text.contains("entries = [\"src/main.ts\"]"), "{text}");
        let python = manifest("app", EntryLanguage::Python);
        assert!(python.contains("entries = [\"src/main.py\"]"), "{python}");
    }
}
