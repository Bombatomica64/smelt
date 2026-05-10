//! Rule identities and source-shape metadata for standard-library mappings.

use crate::BackendDependency;

/// Source language that produced a standard-library call shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SourceLanguage {
    /// TypeScript or JavaScript input.
    TypeScript,
    /// Python input.
    Python,
}

/// Broad API namespace for a standard-library rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ApiNamespace {
    /// JSON parse/stringify APIs.
    Json,
    /// Regular-expression APIs.
    Regex,
    /// Random-number APIs.
    Random,
    /// HTTP client APIs.
    Http,
}

/// Receiver shape for a source API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReceiverKind {
    /// No receiver, such as `fetch(url)`.
    FreeFunction,
    /// Static namespace receiver, such as `JSON.parse`.
    Namespace,
    /// Instance receiver, such as `new RegExp(pattern).test(text)`.
    Instance,
}

/// Source API call shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ApiShape {
    /// A function or method call.
    Call,
    /// A constructor call.
    Constructor,
    /// A property access.
    Property,
}

/// Argument shape required by a supported rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ArgShape {
    /// Exactly one value argument.
    OneValue,
    /// Exactly one string argument.
    OneString,
    /// Exactly two string arguments.
    TwoStrings,
    /// Exactly two integer arguments.
    TwoInts,
    /// No arguments.
    None,
}

/// Return shape produced by a standard-library rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReturnShape {
    /// Boolean result.
    Bool,
    /// String result.
    String,
    /// Floating-point result.
    Float,
    /// Integer result.
    Int,
    /// Type is supplied by call-site context.
    Contextual,
}

/// Side-effect profile of a standard-library rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EffectKind {
    /// Pure deterministic mapping.
    Pure,
    /// Random-value generation.
    Random,
    /// HTTP or external IO.
    Io,
}

/// Stable identity for a recognized standard-library lowering rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum RuleId {
    /// TypeScript `JSON.stringify(value)`.
    TsJsonStringify,
    /// TypeScript `JSON.parse<T>(text)`.
    TsJsonParse,
    /// TypeScript `new RegExp(pattern).test(text)`.
    TsRegExpTest,
    /// TypeScript `Math.random()`.
    TsMathRandom,
    /// TypeScript `fetch(url)`.
    TsFetch,
    /// Python `json.dumps(value)`.
    PyJsonDumps,
    /// Python `json.loads(text)`.
    PyJsonLoads,
    /// Python `re.search(pattern, text)`.
    PyReSearch,
    /// Python `re.match(pattern, text)`.
    PyReMatch,
    /// Python `re.fullmatch(pattern, text)`.
    PyReFullMatch,
    /// Python `random.random()`.
    PyRandomRandom,
    /// Python `random.randint(start, end)`.
    PyRandomRandInt,
    /// Python `random.choice(values)`.
    PyRandomChoice,
    /// Python `requests.get(url)`.
    PyRequestsGet,
}

impl RuleId {
    /// Return the backend dependency required by this rule, when any.
    #[must_use]
    pub const fn backend_dependency(self) -> Option<BackendDependency> {
        match self {
            Self::TsJsonStringify | Self::TsJsonParse | Self::PyJsonDumps | Self::PyJsonLoads => {
                Some(BackendDependency::SerdeJson)
            }
            Self::TsRegExpTest | Self::PyReSearch | Self::PyReMatch | Self::PyReFullMatch => {
                Some(BackendDependency::Regex)
            }
            Self::TsMathRandom
            | Self::PyRandomRandom
            | Self::PyRandomRandInt
            | Self::PyRandomChoice => Some(BackendDependency::Rand),
            Self::TsFetch | Self::PyRequestsGet => Some(BackendDependency::Reqwest),
        }
    }

    /// Return a concise source API name for diagnostics.
    #[must_use]
    pub const fn source_api(self) -> &'static str {
        match self {
            Self::TsJsonStringify => "JSON.stringify",
            Self::TsJsonParse => "JSON.parse",
            Self::TsRegExpTest => "RegExp.test",
            Self::TsMathRandom => "Math.random",
            Self::TsFetch => "fetch",
            Self::PyJsonDumps => "json.dumps",
            Self::PyJsonLoads => "json.loads",
            Self::PyReSearch => "re.search",
            Self::PyReMatch => "re.match",
            Self::PyReFullMatch => "re.fullmatch",
            Self::PyRandomRandom => "random.random",
            Self::PyRandomRandInt => "random.randint",
            Self::PyRandomChoice => "random.choice",
            Self::PyRequestsGet => "requests.get",
        }
    }
}
