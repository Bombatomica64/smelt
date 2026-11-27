//! Python's builtin exception classes and their bases.
//!
//! Shared by the Python frontend, which resolves the chain an `except` clause
//! and a `raise` name, and the Rust backend, which writes the chain of a user
//! exception class whose topmost program base derives from a builtin class
//! (`docs/design/python-exceptions.md`).

/// Python's builtin exception classes, each with its direct base.
///
/// `BaseException` is the root; every other class names a class earlier in the
/// table, so a chain is found by walking bases.
pub const BUILTIN_EXCEPTIONS: &[(&str, &str)] = &[
    ("BaseException", ""),
    ("BaseExceptionGroup", "BaseException"),
    ("GeneratorExit", "BaseException"),
    ("KeyboardInterrupt", "BaseException"),
    ("SystemExit", "BaseException"),
    ("Exception", "BaseException"),
    ("ArithmeticError", "Exception"),
    ("FloatingPointError", "ArithmeticError"),
    ("OverflowError", "ArithmeticError"),
    ("ZeroDivisionError", "ArithmeticError"),
    ("AssertionError", "Exception"),
    ("AttributeError", "Exception"),
    ("BufferError", "Exception"),
    ("EOFError", "Exception"),
    ("ExceptionGroup", "BaseExceptionGroup"),
    ("ImportError", "Exception"),
    ("ModuleNotFoundError", "ImportError"),
    ("LookupError", "Exception"),
    ("IndexError", "LookupError"),
    ("KeyError", "LookupError"),
    ("MemoryError", "Exception"),
    ("NameError", "Exception"),
    ("UnboundLocalError", "NameError"),
    ("OSError", "Exception"),
    ("BlockingIOError", "OSError"),
    ("ChildProcessError", "OSError"),
    ("ConnectionError", "OSError"),
    ("BrokenPipeError", "ConnectionError"),
    ("ConnectionAbortedError", "ConnectionError"),
    ("ConnectionRefusedError", "ConnectionError"),
    ("ConnectionResetError", "ConnectionError"),
    ("FileExistsError", "OSError"),
    ("FileNotFoundError", "OSError"),
    ("InterruptedError", "OSError"),
    ("IsADirectoryError", "OSError"),
    ("NotADirectoryError", "OSError"),
    ("PermissionError", "OSError"),
    ("ProcessLookupError", "OSError"),
    ("TimeoutError", "OSError"),
    ("ReferenceError", "Exception"),
    ("RuntimeError", "Exception"),
    ("NotImplementedError", "RuntimeError"),
    ("PythonFinalizationError", "RuntimeError"),
    ("RecursionError", "RuntimeError"),
    ("StopAsyncIteration", "Exception"),
    ("StopIteration", "Exception"),
    ("SyntaxError", "Exception"),
    ("IndentationError", "SyntaxError"),
    ("TabError", "IndentationError"),
    ("SystemError", "Exception"),
    ("TypeError", "Exception"),
    ("ValueError", "Exception"),
    ("UnicodeError", "ValueError"),
    ("UnicodeDecodeError", "UnicodeError"),
    ("UnicodeEncodeError", "UnicodeError"),
    ("UnicodeTranslateError", "UnicodeError"),
    ("Warning", "Exception"),
    ("BytesWarning", "Warning"),
    ("DeprecationWarning", "Warning"),
    ("EncodingWarning", "Warning"),
    ("FutureWarning", "Warning"),
    ("ImportWarning", "Warning"),
    ("PendingDeprecationWarning", "Warning"),
    ("ResourceWarning", "Warning"),
    ("RuntimeWarning", "Warning"),
    ("SyntaxWarning", "Warning"),
    ("UnicodeWarning", "Warning"),
    ("UserWarning", "Warning"),
];

/// Builtin names that are the same class object as another builtin exception.
pub const BUILTIN_EXCEPTION_ALIASES: &[(&str, &str)] =
    &[("EnvironmentError", "OSError"), ("IOError", "OSError")];

/// The chain of the builtin exception class `name`: its own name, then each
/// base up to `BaseException`. An alias answers the chain of the class it is.
#[must_use]
pub fn builtin_exception_chain(name: &str) -> Option<Vec<&'static str>> {
    let class_name = BUILTIN_EXCEPTION_ALIASES
        .iter()
        .find_map(|(alias, class)| (*alias == name).then_some(*class))
        .unwrap_or(name);
    let mut chain = Vec::new();
    let mut current = class_name;
    while !current.is_empty() {
        let (class, base) = BUILTIN_EXCEPTIONS
            .iter()
            .find(|(class, _)| *class == current)?;
        chain.push(*class);
        current = base;
    }
    Some(chain)
}
