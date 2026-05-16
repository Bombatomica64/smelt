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
    /// Date and datetime APIs.
    DateTime,
    /// URL parsing and field APIs.
    Url,
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
    /// Exactly one numeric argument.
    OneNumber,
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
    /// TypeScript `Date.now()`.
    TsDateNow,
    /// TypeScript `new Date(timestamp).toISOString()`.
    TsDateToIsoString,
    /// TypeScript `new URL(text).field`.
    TsUrlField,
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
    /// Python `datetime.datetime.now()` or `utcnow()`.
    PyDateTimeNow,
    /// Python `datetime.datetime.fromtimestamp(seconds)`.
    PyDateTimeFromTimestamp,
    /// Python `urllib.parse.urlparse(text).field`.
    PyUrlparseField,
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
            Self::TsDateNow
            | Self::TsDateToIsoString
            | Self::PyDateTimeNow
            | Self::PyDateTimeFromTimestamp => Some(BackendDependency::Chrono),
            Self::TsUrlField | Self::PyUrlparseField => Some(BackendDependency::Url),
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
            Self::TsDateNow => "Date.now",
            Self::TsDateToIsoString => "Date.toISOString",
            Self::TsUrlField => "URL field access",
            Self::PyJsonDumps => "json.dumps",
            Self::PyJsonLoads => "json.loads",
            Self::PyReSearch => "re.search",
            Self::PyReMatch => "re.match",
            Self::PyReFullMatch => "re.fullmatch",
            Self::PyRandomRandom => "random.random",
            Self::PyRandomRandInt => "random.randint",
            Self::PyRandomChoice => "random.choice",
            Self::PyRequestsGet => "requests.get",
            Self::PyDateTimeNow => "datetime.datetime.now",
            Self::PyDateTimeFromTimestamp => "datetime.datetime.fromtimestamp",
            Self::PyUrlparseField => "urllib.parse.urlparse field access",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert that newly registry-backed Chrono and URL rules expose stable
    /// dependency and source API metadata for frontend diagnostics/codegen.
    #[test]
    fn dependency_backed_date_and_url_rules_report_metadata() {
        let cases = [
            (RuleId::TsDateNow, BackendDependency::Chrono, "Date.now"),
            (
                RuleId::TsDateToIsoString,
                BackendDependency::Chrono,
                "Date.toISOString",
            ),
            (
                RuleId::PyDateTimeNow,
                BackendDependency::Chrono,
                "datetime.datetime.now",
            ),
            (
                RuleId::PyDateTimeFromTimestamp,
                BackendDependency::Chrono,
                "datetime.datetime.fromtimestamp",
            ),
            (
                RuleId::TsUrlField,
                BackendDependency::Url,
                "URL field access",
            ),
            (
                RuleId::PyUrlparseField,
                BackendDependency::Url,
                "urllib.parse.urlparse field access",
            ),
        ];

        for (rule, dependency, source_api) in cases {
            assert_eq!(rule.backend_dependency(), Some(dependency));
            assert_eq!(rule.source_api(), source_api);
        }
    }
}
