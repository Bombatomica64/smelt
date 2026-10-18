//! Resolving `[packages.<name>]` declarations to TypeScript source trees.
//!
//! A bare import (`import { Hono } from 'hono'`) resolves first through these
//! declarations and only then through workspace `package.json` metadata (see
//! `manifest::DependencyCollector`). A declaration names either a local
//! directory or a git repository pinned to a revision; a git source is fetched
//! once into a cache directory keyed by package and revision and reused while
//! its checkout still matches that revision.
//!
//! The cache lives under `$SMELT_PACKAGE_CACHE`, else
//! `$XDG_CACHE_HOME/smelt/packages`, else `$HOME/.cache/smelt/packages`. A
//! fetch goes to a temporary sibling directory first and is renamed into
//! place, so an interrupted fetch never leaves a half checkout that a later
//! run would trust.

use std::{
    error::Error,
    io,
    path::{Path, PathBuf},
    process::Command,
};

use crate::config::{Config, PackageSource};

/// One `[packages.<name>]` declaration with its source tree on disk.
#[derive(Clone, Debug)]
pub(crate) struct ResolvedPackage {
    /// The package name bare specifiers use (`hono`, `@scope/name`).
    pub(crate) name: String,
    /// Canonical package root.
    pub(crate) root: PathBuf,
    /// Canonical entry module.
    pub(crate) entry: PathBuf,
    /// Package-root-relative exclude globs.
    pub(crate) excludes: Vec<String>,
}

impl ResolvedPackage {
    /// The module a bare specifier names: the entry for `name` itself, the
    /// matching path next to the entry for `name/sub`.
    pub(crate) fn module_base(&self, specifier: &str) -> Option<PathBuf> {
        if specifier == self.name {
            return Some(self.entry.clone());
        }
        let subpath = specifier.strip_prefix(&self.name)?.strip_prefix('/')?;
        Some(self.entry.parent().unwrap_or(&self.root).join(subpath))
    }

    /// Whether `path` lies in this package and matches one of its excludes.
    pub(crate) fn excludes_path(&self, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return false;
        };
        self.excludes
            .iter()
            .any(|pattern| crate::lowering::path_matches_glob(relative, pattern))
    }
}

/// Resolve every `[packages.<name>]` declaration of `config`, fetching git
/// sources that are not cached yet.
pub(crate) fn resolve_packages(
    config: &Config,
    manifest_dir: &Path,
) -> Result<Vec<ResolvedPackage>, Box<dyn Error>> {
    config
        .packages()
        .iter()
        .map(|(name, source)| resolve_package(name, source, manifest_dir))
        .collect()
}

/// Resolve one declaration.
fn resolve_package(
    name: &str,
    source: &PackageSource,
    manifest_dir: &Path,
) -> Result<ResolvedPackage, Box<dyn Error>> {
    let declared_root = match (&source.path, &source.git) {
        (Some(path), None) => manifest_dir.join(path),
        (None, Some(url)) => {
            let rev = source.rev.as_deref().ok_or_else(|| {
                invalid(format!(
                    "[packages.{name}]: `git` needs a pinned `rev` (a tag or a commit)"
                ))
            })?;
            fetch_git_package(name, url, rev)?
        }
        _ => {
            return Err(invalid(format!(
                "[packages.{name}]: give exactly one of `path` and `git`"
            )));
        }
    };
    let root = declared_root.canonicalize().map_err(|error| {
        invalid(format!(
            "[packages.{name}]: source root {}: {error}",
            declared_root.display()
        ))
    })?;
    let declared_entry = match &source.entry {
        Some(entry) => root.join(entry),
        None => ["src/index.ts", "index.ts"]
            .into_iter()
            .map(|candidate| root.join(candidate))
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| {
                invalid(format!(
                    "[packages.{name}]: no `entry` given and neither src/index.ts nor index.ts exists under {}",
                    root.display()
                ))
            })?,
    };
    let entry = declared_entry.canonicalize().map_err(|error| {
        invalid(format!(
            "[packages.{name}]: entry {}: {error}",
            declared_entry.display()
        ))
    })?;
    Ok(ResolvedPackage {
        name: name.to_owned(),
        root,
        entry,
        excludes: source.exclude.clone().unwrap_or_default(),
    })
}

/// The directory git package sources are cached in.
fn package_cache_dir() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(cache) = std::env::var_os("SMELT_PACKAGE_CACHE") {
        return Ok(PathBuf::from(cache));
    }
    if let Some(xdg_cache) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(xdg_cache).join("smelt/packages"));
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".cache/smelt/packages"))
        .ok_or_else(|| invalid("no package cache: set SMELT_PACKAGE_CACHE or HOME".to_owned()))
}

/// The checkout of `url` at `rev`, fetched into the cache on first use.
fn fetch_git_package(name: &str, url: &str, rev: &str) -> Result<PathBuf, Box<dyn Error>> {
    let cache = package_cache_dir()?;
    std::fs::create_dir_all(&cache)?;
    let slug = |text: &str| {
        text.chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>()
    };
    let checkout = cache.join(format!("{}-{}", slug(name), slug(rev)));
    if checkout.join(".git").is_dir() && checkout_matches(&checkout, rev) {
        return Ok(checkout);
    }
    let staging = cache.join(format!(
        ".{}-{}.{}",
        slug(name),
        slug(rev),
        std::process::id()
    ));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    let staging_text = staging.to_string_lossy().into_owned();
    git(
        &[
            "clone",
            "--quiet",
            "--filter=blob:none",
            "--no-checkout",
            url,
            &staging_text,
        ],
        None,
    )
    .map_err(|error| invalid(format!("[packages.{name}]: cloning {url}: {error}")))?;
    git(&["checkout", "--quiet", "--detach", rev], Some(&staging)).map_err(|error| {
        invalid(format!(
            "[packages.{name}]: checking out `{rev}` of {url}: {error}"
        ))
    })?;
    if checkout.exists() {
        std::fs::remove_dir_all(&checkout)?;
    }
    std::fs::rename(&staging, &checkout)?;
    Ok(checkout)
}

/// Whether the checkout's `HEAD` is the commit `rev` names.
fn checkout_matches(checkout: &Path, rev: &str) -> bool {
    let head = git(&["rev-parse", "HEAD"], Some(checkout));
    let wanted = git(&["rev-parse", &format!("{rev}^{{commit}}")], Some(checkout));
    matches!((head, wanted), (Ok(head_commit), Ok(wanted_commit)) if head_commit == wanted_commit)
}

/// Run `git` and return its trimmed stdout.
fn git(args: &[&str], working_dir: Option<&Path>) -> Result<String, Box<dyn Error>> {
    let mut command = Command::new("git");
    if let Some(cwd) = working_dir {
        command.current_dir(cwd);
    }
    let output = command.args(args).output()?;
    if !output.status.success() {
        return Err(invalid(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// An invalid-input error carrying `message`.
fn invalid(message: String) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message))
}
