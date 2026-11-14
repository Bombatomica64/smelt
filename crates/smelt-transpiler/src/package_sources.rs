//! Resolving `[packages.<name>]` declarations to TypeScript source trees.
//!
//! A bare import (`import { Hono } from 'hono'`) resolves first through these
//! declarations and only then through workspace `package.json` metadata (see
//! `manifest::DependencyCollector`). A declaration names either a local
//! directory or a git repository pinned to a revision; a git source is fetched
//! once into a cache directory keyed by package, URL and resolved commit
//! (`GitCheckoutKey`) and reused while its checkout still holds that commit.
//! A `rev` that is not a full commit id (a branch, a tag) is resolved against
//! the remote on every build, so a moved branch is fetched again.
//!
//! The cache lives under `$SMELT_PACKAGE_CACHE`, else
//! `$XDG_CACHE_HOME/smelt/packages`, else `$HOME/.cache/smelt/packages`.
//!
//! # Cache protocol
//!
//! The protocol only ever renames into place, so concurrent builds need no
//! lock. A fetch goes to a staging sibling (`.<key>.<pid>`) first and is
//! renamed onto the checkout, so an interrupted fetch never leaves a half
//! checkout that a later run would trust. When two builds fetch the same key
//! at once, the second rename finds the first build's checkout in place: it
//! verifies it, discards its own staging and reuses it, never deleting a tree
//! another build may be reading. Only a checkout that does not match its
//! revision is replaced, by first renaming it aside. Staging directories whose
//! owning process is gone (a killed fetch) are removed before a new fetch.

use std::{
    error::Error,
    io,
    path::{Path, PathBuf},
    process::Command,
};

use crate::config::{Config, PackageLocation, PackageSource};

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
            .any(|pattern| crate::source_files::path_matches_glob(relative, pattern))
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
    let declared_root = match &source.location {
        PackageLocation::Path(path) => manifest_dir.join(path),
        PackageLocation::Git { url, rev } => fetch_git_package(name, url, rev)?,
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

/// A full commit id: 40 (SHA-1) or 64 (SHA-256) lowercase hex digits.
///
/// Only [`Self::parse`] and [`resolve_revision`] build one, so a cache key that
/// holds one names exactly one immutable tree.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CommitSha(String);

impl CommitSha {
    /// `text` as a commit id, when it is a full one.
    fn parse(text: &str) -> Option<Self> {
        let full = matches!(text.len(), 40 | 64);
        (full && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| Self(text.to_ascii_lowercase()))
    }
}

/// Where one git package source is cached: a key no other source shares.
///
/// The URL is part of it (as a hash), so two repositories that publish the
/// same package name at the same tag never share a checkout; the revision is
/// the RESOLVED commit, so a branch that moved is a new key and a new fetch.
#[derive(Debug)]
struct GitCheckoutKey {
    /// A file-name-safe spelling of the package name, for a readable cache.
    name: String,
    /// The first 16 hex digits of the SHA-256 of the URL.
    url_hash: String,
    /// The commit the checkout holds.
    commit: CommitSha,
}

impl GitCheckoutKey {
    /// The key of `name` from `url` at `commit`.
    fn new(name: &str, url: &str, commit: CommitSha) -> Self {
        use sha2::Digest as _;
        let digest = hex::encode(sha2::Sha256::digest(url.as_bytes()));
        Self {
            name: cache_slug(name),
            url_hash: digest.chars().take(16).collect(),
            commit,
        }
    }

    /// The checkout's directory name in the cache.
    fn dir_name(&self) -> String {
        format!("{}-{}-{}", self.name, self.url_hash, self.commit.0)
    }
}

/// The commit `rev` names in the repository at `url`.
///
/// A full commit id is its own answer. Anything else (a branch, a tag) can
/// move, so it is resolved against the remote on every build with
/// `git ls-remote`; the build then uses the commit it names now, never a
/// checkout fetched when it named another one. Refs are tried in git's order:
/// a peeled tag, a tag, a branch, then the exact ref name.
fn resolve_revision(name: &str, url: &str, rev: &str) -> Result<CommitSha, Box<dyn Error>> {
    if let Some(commit) = CommitSha::parse(rev) {
        return Ok(commit);
    }
    let listing = git(&["ls-remote", url, rev], None).map_err(|error| {
        invalid(format!(
            "[packages.{name}]: resolving `{rev}` of {url}: {error}"
        ))
    })?;
    commit_from_ls_remote(&listing, rev).ok_or_else(|| {
        invalid(format!(
            "[packages.{name}]: `{rev}` is not a full commit id, a branch or a tag of {url}; \
             pin `rev` to a full commit id"
        ))
    })
}

/// The commit `git ls-remote <url> <rev>` output says `rev` names, if any.
fn commit_from_ls_remote(listing: &str, rev: &str) -> Option<CommitSha> {
    let refs = listing
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect::<Vec<_>>();
    [
        format!("refs/tags/{rev}^{{}}"),
        format!("refs/tags/{rev}"),
        format!("refs/heads/{rev}"),
        rev.to_owned(),
    ]
    .iter()
    .find_map(|wanted| refs.iter().find(|(_, name)| name == wanted))
    .and_then(|(commit, _)| CommitSha::parse(commit))
}

/// The checkout of `url` at `rev`, fetched into the cache on first use.
///
/// See the module docs for the rename-only protocol this follows.
fn fetch_git_package(name: &str, url: &str, rev: &str) -> Result<PathBuf, Box<dyn Error>> {
    let commit = resolve_revision(name, url, rev)?;
    let cache = package_cache_dir()?;
    std::fs::create_dir_all(&cache)?;
    let wanted = commit.0.clone();
    let key = GitCheckoutKey::new(name, url, commit).dir_name();
    let checkout = cache.join(&key);
    let is_current = |path: &Path| path.join(".git").is_dir() && checkout_matches(path, &wanted);
    if is_current(&checkout) {
        return Ok(checkout);
    }
    remove_stale_staging(&cache, &key, process_is_gone)?;
    let staging = cache.join(format!(".{key}.{}", std::process::id()));
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
    git(
        &["checkout", "--quiet", "--detach", &wanted],
        Some(&staging),
    )
    .map_err(|error| {
        invalid(format!(
            "[packages.{name}]: checking out `{rev}` ({wanted}) of {url}: {error}"
        ))
    })?;
    if std::fs::rename(&staging, &checkout).is_ok() {
        return Ok(checkout);
    }
    // The checkout appeared (or was already there) while we fetched.
    if is_current(&checkout) {
        // Another build won the race; its tree is as good as ours.
        std::fs::remove_dir_all(&staging)?;
        return Ok(checkout);
    }
    // A checkout of the wrong revision (a half or foreign tree): move it aside
    // atomically, put ours in place, then delete the old one.
    let retired = cache.join(format!(".{key}.retired.{}", std::process::id()));
    std::fs::rename(&checkout, &retired)?;
    std::fs::rename(&staging, &checkout)?;
    std::fs::remove_dir_all(&retired)?;
    Ok(checkout)
}

/// Whether the process `pid` no longer exists.
///
/// Answers only where `/proc` lists processes; elsewhere every process is
/// assumed alive, so a staging directory is never removed from under a live
/// fetch on a platform where liveness cannot be told.
fn process_is_gone(pid: u32) -> bool {
    let proc_root = Path::new("/proc");
    proc_root.join("self").exists() && !proc_root.join(pid.to_string()).exists()
}

/// A file-name-safe spelling of a package name or revision.
fn cache_slug(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

/// Remove the staging (and retired) directories of `key` in `cache` whose
/// owning process `is_gone`, plus this process's own leftovers.
///
/// A staging directory is named `.<key>.<pid>` and a retired checkout
/// `.<key>.retired.<pid>`; anything else in the cache is left alone.
fn remove_stale_staging(cache: &Path, key: &str, is_gone: impl Fn(u32) -> bool) -> io::Result<()> {
    let prefix = format!(".{key}.");
    let own = std::process::id();
    for listed in std::fs::read_dir(cache)? {
        let dir_entry = listed?;
        let file_name = dir_entry.file_name();
        let Some(suffix) = file_name
            .to_str()
            .and_then(|text| text.strip_prefix(&prefix))
        else {
            continue;
        };
        let pid_text = suffix.strip_prefix("retired.").unwrap_or(suffix);
        let Ok(pid) = pid_text.parse::<u32>() else {
            continue;
        };
        if pid == own || is_gone(pid) {
            std::fs::remove_dir_all(dir_entry.path())?;
        }
    }
    Ok(())
}

/// Whether the checkout's `HEAD` is the commit `commit`.
fn checkout_matches(checkout: &Path, commit: &str) -> bool {
    git(&["rev-parse", "HEAD"], Some(checkout)).is_ok_and(|head| head.eq_ignore_ascii_case(commit))
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

#[cfg(test)]
mod tests {
    //! Cache-protocol helpers.

    use super::{
        CommitSha, GitCheckoutKey, cache_slug, commit_from_ls_remote, remove_stale_staging,
    };

    /// A full id parses; an abbreviation or a branch name does not.
    #[test]
    fn only_full_commit_ids_are_commits() {
        assert!(CommitSha::parse(&"a".repeat(40)).is_some());
        assert!(CommitSha::parse(&"A".repeat(64)).is_some());
        assert!(CommitSha::parse("abc1234").is_none());
        assert!(CommitSha::parse("main").is_none());
    }

    /// The key holds the URL: one name and commit from two repositories are
    /// two keys, and `@a/b` and `_a_b` from two repositories never share one.
    #[test]
    fn the_key_holds_the_url_and_the_commit() {
        let commit = || CommitSha::parse(&"0".repeat(40)).expect("full id");
        let first = GitCheckoutKey::new("@a/b", "https://one.example/r.git", commit()).dir_name();
        let second = GitCheckoutKey::new("_a_b", "https://two.example/r.git", commit()).dir_name();
        assert_ne!(first, second);
        assert!(first.ends_with(&"0".repeat(40)), "{first}");
    }

    /// `ls-remote` output resolves a peeled tag before the tag object, and a
    /// tag before a branch of the same name; a ref of another name never matches.
    #[test]
    fn ls_remote_resolves_in_git_order() {
        let (tag_object, tag_commit, branch) = ("1".repeat(40), "2".repeat(40), "3".repeat(40));
        let listing = format!(
            "{branch}\trefs/heads/v1\n{tag_object}\trefs/tags/v1\n{tag_commit}\trefs/tags/v1^{{}}\n{branch}\trefs/heads/x/v1\n"
        );
        assert_eq!(
            commit_from_ls_remote(&listing, "v1"),
            CommitSha::parse(&tag_commit)
        );
        let branch_only = format!("{branch}\trefs/heads/main\n");
        assert_eq!(
            commit_from_ls_remote(&branch_only, "main"),
            CommitSha::parse(&branch)
        );
        assert_eq!(
            commit_from_ls_remote(&format!("{branch}\trefs/heads/x/v1\n"), "v1"),
            None
        );
    }

    /// Dead owners' staging and retired trees go; live owners', other keys'
    /// and the checkout itself stay.
    #[test]
    fn stale_staging_of_the_key_is_removed_and_nothing_else() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let cache = std::env::temp_dir().join(format!("smelt_package_cache_{unique}"));
        let key = format!("{}-{}", cache_slug("@scope/pkg"), cache_slug("v1"));
        let own = std::process::id();
        for dir in [
            format!(".{key}.123"),
            format!(".{key}.retired.124"),
            format!(".{key}.4567"),
            format!(".{key}.{own}"),
            key.clone(),
            format!(".{key}-other.123"),
        ] {
            std::fs::create_dir_all(cache.join(dir)).expect("create");
        }
        // 123 and 124 are dead; 4567 is a live concurrent build.
        remove_stale_staging(&cache, &key, |pid| pid == 123 || pid == 124).expect("clean");
        let mut left = std::fs::read_dir(&cache)
            .expect("list")
            .map(|listed| {
                listed
                    .expect("entry")
                    .file_name()
                    .into_string()
                    .expect("utf8")
            })
            .collect::<Vec<_>>();
        left.sort();
        std::fs::remove_dir_all(&cache).expect("remove");
        let mut expected = vec![
            key.clone(),
            format!(".{key}-other.123"),
            format!(".{key}.4567"),
        ];
        expected.sort();
        assert_eq!(left, expected);
    }
}
