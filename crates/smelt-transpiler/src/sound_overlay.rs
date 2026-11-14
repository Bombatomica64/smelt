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

/// What looking up one installed package's typed version found.
#[derive(Debug)]
enum Lookup {
    /// The project has no typed version for the package; it compiles as installed.
    Absent,
    /// A typed version exists but lays nothing (a mismatch was warned about).
    NotUsed,
    /// The typed version's files are laid over the package (each laid file
    /// carries its [`TypedVersionOrigin`]).
    Laid,
}

/// The mutable part of an enabled layer, shared by every clone.
#[derive(Debug, Default)]
struct LayerState {
    /// Outcome of the typed-version lookup per canonical installed package
    /// directory; a package is looked up (and its note printed) once.
    packages: HashMap<PathBuf, Lookup>,
    /// Laid files by canonical virtual path.
    files: HashMap<PathBuf, LaidSource>,
    /// Directories that exist only because a `"new"` laid file is inside them.
    directories: HashSet<PathBuf>,
}

impl LayerState {
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

/// An enabled typed-version layer: the lookup inputs plus the shared state.
#[derive(Debug)]
struct Layer {
    /// Lookup inputs.
    settings: SoundSettings,
    /// What has been looked up and laid so far.
    state: RwLock<LayerState>,
}

/// The typed-version layer over the real filesystem; cheap to clone, and all
/// clones share one layer.
///
/// A disabled overlay (`layer: None`, single-file commands) is a different
/// value, not an enabled one with missing settings: it never locks, never
/// looks anything up and passes every call through.
#[derive(Debug, Clone, Default)]
pub(crate) struct SoundOverlay {
    /// Shared layer, when typed versions are enabled.
    layer: Option<Arc<Layer>>,
}

/// JavaScript output extensions and the TypeScript input laid beside them.
const LAID_INPUT_EXTENSIONS: &[(&str, &str)] = &[("js", "ts"), ("mjs", "mts"), ("cjs", "cts")];

impl SoundOverlay {
    /// A layer that looks typed versions up with `settings`; `None` disables it.
    pub(crate) fn with_settings(settings: Option<SoundSettings>) -> Self {
        Self {
            layer: settings.map(|lookup_settings| {
                Arc::new(Layer {
                    settings: lookup_settings,
                    state: RwLock::new(LayerState::default()),
                })
            }),
        }
    }

    /// Run `read` against the layer state, or return `None` when disabled.
    ///
    /// A poisoned lock is tolerated: the state is only ever extended, and each
    /// extension is a single insert made after its filesystem work is done,
    /// so a panicking writer cannot leave it inconsistent.
    fn with_state<T>(&self, read: impl FnOnce(&LayerState) -> Option<T>) -> Option<T> {
        let layer = self.layer.as_ref()?;
        let state = layer.state.read().unwrap_or_else(PoisonError::into_inner);
        read(&state)
    }

    /// Look up the typed version of the package a bare `specifier` imported
    /// from `importer` names, once per installed package, and lay its files
    /// when it fits. Prints Bun's note or warning to stderr.
    ///
    /// Relative and absolute specifiers name source files, not packages, and
    /// a package that is not installed has nothing to lay over: both are left
    /// alone. The lookup and every filesystem probe run without the lock held;
    /// the outcome is recorded only once it is known, and only the caller that
    /// records it prints the note.
    #[expect(
        clippy::print_stderr,
        reason = "the note or warning is the build's report to the user, as in Bun"
    )]
    pub(crate) fn activate(&self, importer: &Path, specifier: &str) {
        if specifier.starts_with('.') || specifier.starts_with('/') {
            return;
        }
        let Some(layer) = &self.layer else {
            return;
        };
        let name = crate::manifest::bare_package_name(specifier);
        let from = importer.parent().unwrap_or_else(|| Path::new("."));
        let Some(installed) = installed_package_dir(from, &name) else {
            return;
        };
        let directory = installed.canonicalize().unwrap_or(installed);
        if self
            .with_state(|state| state.packages.get(&directory).map(|_| ()))
            .is_some()
        {
            return;
        }

        let (lookup, laid_files, note) = match typed_package(&layer.settings, &directory, &name) {
            None => (Lookup::Absent, Vec::new(), None),
            Some(found) if found.files.is_empty() => {
                (Lookup::NotUsed, Vec::new(), Some(found.message))
            }
            Some(found) => {
                let origin = Arc::new(TypedVersionOrigin {
                    name: found.name,
                    shown: found.shown,
                });
                let laid_files = found
                    .files
                    .into_iter()
                    .map(|file| {
                        let layer_only_directories = file
                            .path
                            .ancestors()
                            .skip(1)
                            .take_while(|directory_path| !directory_path.exists())
                            .map(Path::to_path_buf)
                            .collect::<Vec<_>>();
                        let source = LaidSource {
                            text: file.text,
                            origin: Arc::clone(&origin),
                        };
                        (file.path, source, layer_only_directories)
                    })
                    .collect();
                (Lookup::Laid, laid_files, Some(found.message))
            }
        };

        let mut state = layer.state.write().unwrap_or_else(PoisonError::into_inner);
        // Another clone may have recorded this package while the lock was free.
        if state.packages.contains_key(&directory) {
            return;
        }
        for (path, source, layer_only_directories) in laid_files {
            state.directories.extend(layer_only_directories);
            state.files.insert(path, source);
        }
        state.packages.insert(directory, lookup);
        drop(state);
        if let Some(message) = note {
            eprintln!("{message}");
        }
    }

    /// The laid text at `path` and the typed version it came from, if laid.
    pub(crate) fn laid(&self, path: &Path) -> Option<(String, Arc<TypedVersionOrigin>)> {
        self.with_state(|state| {
            state
                .files
                .get(path)
                .map(|source| (source.text.clone(), Arc::clone(&source.origin)))
        })
    }

    /// Whether `path` is a laid file.
    pub(crate) fn is_laid(&self, path: &Path) -> bool {
        self.with_state(|state| state.files.contains_key(path).then_some(()))
            .is_some()
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
        let stored = self.with_state(|state| state.stored(path));
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
        self.with_state(|state| state.stored(&canonical.join(rest)))
            .ok_or(error)
    }

    /// The layer's metadata for a laid file or layer-only directory.
    fn laid_metadata(&self, path: &Path) -> Option<FileMetadata> {
        self.with_state(|state| {
            if state.files.contains_key(path) {
                Some(FileMetadata::new(true, false, false))
            } else {
                state
                    .directories
                    .contains(path)
                    .then(|| FileMetadata::new(false, true, false))
            }
        })
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
