//! The in-memory layer that lays typed versions of packages over `node_modules`.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631), typed-package
//! overlays ("During an `--aot` build the bundler reads the files under `lay/`
//! in place of (or beside) the installed ones"; `packages/bun-sound/README.md`).
//! What a typed version is and when it fits is [`crate::sound`]; this module is
//! only the seam through which the dependency collector sees one.
//!
//! # The seam
//!
//! [`SoundOverlay`] is an `oxc_resolver` [`FileSystem`]. The dependency
//! collector's TypeScript resolver runs on it, so module resolution — package
//! `main`, index files, extension probing, relative imports between laid files
//! — sees a laid file exactly where `sound.json` lays it, including a `"new"`
//! file that does not exist on disk, and sees the laid text in place of an
//! installed file it replaces. Everything not laid passes through to the real
//! filesystem unchanged. Nothing is ever written: the layer lives for one build.
//!
//! A package's typed version is looked up when the collector first resolves a
//! bare specifier into it ([`SoundOverlay::activate`]), before the resolver
//! touches any path inside the package, so the resolver's metadata cache never
//! holds a pre-overlay answer for a laid path. The note or warning is printed
//! then, once per installed package.
//!
//! # One resolution rule
//!
//! A resolved installed JavaScript file (`x.js`, `x.mjs`, `x.cjs`) inside a
//! package whose typed version lays the TypeScript input beside it (`x.ts`,
//! `x.mts`, `x.cts`) resolves to that laid input. This is the same `NodeNext`
//! output-to-input substitution `tsc` applies (and `crate::manifest` applies to
//! relative specifiers): the package's `main` names the JavaScript a build
//! would emit, the typed version is its input. Bun's README warns against
//! laying `x.ts` beside `x.js` when an `x.d.ts` exists *because* the checker
//! makes this substitution; Smelt is that checker and compiler in one, so the
//! laid `x.ts` is what it compiles.
//!
//! Behavioral requirement: the laid text is the installed code with types
//! added, so the program behaves as it does under Node with the installed
//! package. Type-preservation requirement: the package's functions and records
//! lower with the concrete types the laid text declares.

use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, PoisonError, RwLock},
};

use oxc_resolver::{FileMetadata, FileSystem, FileSystemOs, ResolveError};

use crate::sound::{SoundSettings, installed_package_dir, typed_package};

/// Which typed version a laid source came from, for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypedVersionOrigin {
    /// The package name as imported.
    pub(crate) name: String,
    /// Where the typed version lives, as the note shows it.
    pub(crate) shown: String,
}

/// One laid file as the layer serves it.
#[derive(Debug)]
struct LaidSource {
    /// The laid text.
    text: String,
    /// The typed version it belongs to.
    origin: Arc<TypedVersionOrigin>,
}

/// The mutable part of the layer, shared by every clone.
#[derive(Debug, Default)]
struct Layer {
    /// Lookup inputs; `None` disables typed versions (single-file commands).
    settings: Option<SoundSettings>,
    /// Canonical installed package directories already looked up.
    packages: HashSet<PathBuf>,
    /// Laid files by canonical virtual path.
    files: HashMap<PathBuf, LaidSource>,
    /// Directories that exist only because a `"new"` laid file is inside them.
    directories: HashSet<PathBuf>,
}

impl Layer {
    /// The stored spelling of a laid file or layer-only directory equal to
    /// `path`, if the layer has one.
    fn stored(&self, path: &Path) -> Option<PathBuf> {
        self.files
            .get_key_value(path)
            .map(|(stored, _)| stored)
            .or_else(|| self.directories.get(path))
            .cloned()
    }
}

/// The typed-version layer over the real filesystem; cheap to clone, and all
/// clones share one layer.
#[derive(Debug, Clone, Default)]
pub(crate) struct SoundOverlay {
    /// Shared layer state.
    layer: Arc<RwLock<Layer>>,
}

/// JavaScript output extensions and the TypeScript input laid beside them.
const LAID_INPUT_EXTENSIONS: &[(&str, &str)] = &[("js", "ts"), ("mjs", "mts"), ("cjs", "cts")];

impl SoundOverlay {
    /// A layer that looks typed versions up with `settings`.
    pub(crate) fn with_settings(settings: Option<SoundSettings>) -> Self {
        Self {
            layer: Arc::new(RwLock::new(Layer {
                settings,
                ..Layer::default()
            })),
        }
    }

    /// Read access to the layer, tolerating a poisoned lock (the layer is only
    /// ever extended, so a panicking writer cannot leave it inconsistent).
    fn layer(&self) -> std::sync::RwLockReadGuard<'_, Layer> {
        self.layer.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Look up the typed version of the package a bare `specifier` imported
    /// from `importer` names, once per installed package, and lay its files
    /// when it fits. Prints Bun's note or warning to stderr.
    ///
    /// Relative and absolute specifiers name source files, not packages, and
    /// a package that is not installed has nothing to lay over: both are left
    /// alone.
    #[expect(
        clippy::print_stderr,
        reason = "the note or warning is the build's report to the user, as in Bun"
    )]
    pub(crate) fn activate(&self, importer: &Path, specifier: &str) {
        if specifier.starts_with('.') || specifier.starts_with('/') {
            return;
        }
        let Some(settings) = self.layer().settings.clone() else {
            return;
        };
        let name = crate::manifest::bare_package_name(specifier);
        let from = importer.parent().unwrap_or_else(|| Path::new("."));
        let Some(installed) = installed_package_dir(from, &name) else {
            return;
        };
        let directory = installed.canonicalize().unwrap_or(installed);
        let mut layer = self.layer.write().unwrap_or_else(PoisonError::into_inner);
        if !layer.packages.insert(directory.clone()) {
            return;
        }
        let Some(found) = typed_package(&settings, &directory, &name) else {
            return;
        };
        eprintln!("{}", found.message);
        let origin = Arc::new(TypedVersionOrigin {
            name: found.name,
            shown: found.shown,
        });
        for file in found.files {
            let mut parent = file.path.parent();
            while let Some(directory_path) = parent
                && !directory_path.exists()
            {
                layer.directories.insert(directory_path.to_path_buf());
                parent = directory_path.parent();
            }
            layer.files.insert(
                file.path,
                LaidSource {
                    text: file.text,
                    origin: Arc::clone(&origin),
                },
            );
        }
    }

    /// The laid text at `path` and the typed version it came from, if laid.
    pub(crate) fn laid(&self, path: &Path) -> Option<(String, Arc<TypedVersionOrigin>)> {
        self.layer()
            .files
            .get(path)
            .map(|source| (source.text.clone(), Arc::clone(&source.origin)))
    }

    /// Whether `path` is a laid file.
    pub(crate) fn is_laid(&self, path: &Path) -> bool {
        self.layer().files.contains_key(path)
    }

    /// Read a file's text through the layer.
    pub(crate) fn read_text(&self, path: &Path) -> io::Result<String> {
        match self.laid(path) {
            Some((text, _)) => Ok(text),
            None => fs::read_to_string(path),
        }
    }

    /// Apply the layer's one resolution rule (see the module docs): an
    /// installed JavaScript file whose TypeScript input is laid beside it
    /// resolves to that input. Any other path is returned unchanged.
    pub(crate) fn redirect(&self, path: PathBuf) -> PathBuf {
        if self.is_laid(&path) {
            return path;
        }
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            return path;
        };
        LAID_INPUT_EXTENSIONS
            .iter()
            .find(|(output, _)| *output == extension)
            .map(|(_, input)| path.with_extension(input))
            .filter(|input| self.is_laid(input))
            .unwrap_or(path)
    }

    /// Canonicalize a path that may name a laid file or a directory that only
    /// the layer has: the nearest ancestor that exists is canonicalized and
    /// the rest is appended.
    pub(crate) fn canonical_path(&self, path: &Path) -> io::Result<PathBuf> {
        let error = match fs::canonicalize(path) {
            Ok(canonical) => return Ok(canonical),
            Err(error) => error,
        };
        // The stored spelling, not the caller's: `Path` equality ignores an
        // interior `.` component, and the returned path becomes the module's
        // identity.
        let stored = self.layer().stored(path);
        if let Some(stored_path) = stored {
            return Ok(stored_path);
        }
        let Some((existing, canonical)) = path
            .ancestors()
            .skip(1)
            .find_map(|ancestor| Some((ancestor, fs::canonicalize(ancestor).ok()?)))
        else {
            return Err(error);
        };
        let Ok(rest) = path.strip_prefix(existing) else {
            return Err(error);
        };
        self.layer().stored(&canonical.join(rest)).ok_or(error)
    }

    /// The layer's metadata for a laid file or layer-only directory.
    fn laid_metadata(&self, path: &Path) -> Option<FileMetadata> {
        let (is_file, is_directory) = {
            let layer = self.layer();
            (
                layer.files.contains_key(path),
                layer.directories.contains(path),
            )
        };
        if is_file {
            return Some(FileMetadata::new(true, false, false));
        }
        is_directory.then(|| FileMetadata::new(false, true, false))
    }
}

impl FileSystem for SoundOverlay {
    /// An empty layer: no typed versions, every call passes through.
    fn new() -> Self {
        Self::default()
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        match self.laid(path) {
            Some((text, _)) => Ok(text.into_bytes()),
            None => fs::read(path),
        }
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.read_text(path)
    }

    fn metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        self.laid_metadata(path)
            .map_or_else(|| FileSystemOs::metadata(path), Ok)
    }

    fn symlink_metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        self.laid_metadata(path)
            .map_or_else(|| FileSystemOs::symlink_metadata(path), Ok)
    }

    fn read_link(&self, path: &Path) -> Result<PathBuf, ResolveError> {
        FileSystemOs::read_link(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.canonical_path(path)
    }
}
