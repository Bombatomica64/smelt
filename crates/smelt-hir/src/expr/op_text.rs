//! The `dump-hir`/`dump-mir` spelling of each operation selector.
//!
//! `Display` is what lets an entry of the op table write `{op}` in its display
//! clause.

use std::fmt;

use super::{AbortSignalOp, AsyncOp, BlobOp, BoolFoldOp, CryptoOp, DictProjectionOp, EventEmitterOp, FormDataOp, HeadersOp, HttpServerOp, IncomingMessageOp, ListCallbackOp, ListProjectionOp, ListSearchOp, NumericExtremaOp, NumericPredicateOp, NumericRoundOp, NumericUnaryFuncOp, PrimitiveCastOp, PropertyLookup, RegexMatchOp, ErasedPatternMethod, RequestOp, ResponseOp, ServerResponseOp, SetBinaryOp, SetProjectionOp, SetRelationOp, SetRemoveOp, StringAffixOp, StringCaseOp, StringNormalizeForm, StringPadOp, StringPredicateOp, StringReplaceOp, StringSearchOp, StringSliceKind, StringTrimSide, TextDecoderOp, TextEncoderOp, UriTranscodeOp, UrlField, UrlSearchParamsOp};

impl NumericRoundOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Floor => "floor",
            Self::Ceil => "ceil",
            Self::Round => "round",
            Self::Trunc => "trunc",
        }
    }
}

impl fmt::Display for NumericRoundOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl NumericExtremaOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Min => "min",
            Self::Max => "max",
        }
    }
}

impl fmt::Display for NumericExtremaOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl NumericPredicateOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::IsFinite => "is_finite",
            Self::IsInteger => "is_integer",
            Self::IsSafeInteger => "is_safe_integer",
            Self::IsNaN => "is_nan",
        }
    }
}

impl fmt::Display for NumericPredicateOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl NumericUnaryFuncOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Sqrt => "sqrt",
            Self::Cbrt => "cbrt",
            Self::Sign => "sign",
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tan => "tan",
            Self::Asin => "asin",
            Self::Acos => "acos",
            Self::Atan => "atan",
            Self::Log => "log",
            Self::Log10 => "log10",
            Self::Log2 => "log2",
            Self::Exp => "exp",
            Self::Clz32 => "clz32",
        }
    }
}

impl fmt::Display for NumericUnaryFuncOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl PrimitiveCastOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::ToBool => "bool",
            Self::ToInt => "int",
            Self::ToFloat => "float",
            Self::ToInt32 => "int32",
            Self::ParseFloat => "parse_float",
            Self::FromCharCodes => "from_char_codes",
            Self::FromCodePoints => "from_code_points",
            Self::ToJsNumber => "js_number",
            Self::ToString => "string",
        }
    }
}

impl fmt::Display for PrimitiveCastOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl StringCaseOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Lower => "lower",
            Self::Upper => "upper",
        }
    }
}

impl fmt::Display for StringCaseOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl StringNormalizeForm {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Nfc => "nfc",
            Self::Nfd => "nfd",
            Self::Nfkc => "nfkc",
            Self::Nfkd => "nfkd",
        }
    }
}

impl fmt::Display for StringNormalizeForm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl UriTranscodeOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Encode => "uri_encode",
            Self::EncodeComponent => "uri_encode_component",
            Self::Decode => "uri_decode",
            Self::DecodeComponent => "uri_decode_component",
        }
    }
}

impl fmt::Display for UriTranscodeOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl StringTrimSide {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Both => "both",
            Self::Start => "start",
            Self::End => "end",
        }
    }
}

impl fmt::Display for StringTrimSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl StringAffixOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::StartsWith => "starts_with",
            Self::EndsWith => "ends_with",
        }
    }

    /// The spelling of the matching affix removal.
    #[must_use]
    pub const fn remove_text(self) -> &'static str {
        match self {
            Self::StartsWith => "remove_prefix",
            Self::EndsWith => "remove_suffix",
        }
    }
}

impl fmt::Display for StringAffixOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl StringSearchOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Find => "find",
            Self::RFind => "rfind",
            Self::Regex => "search",
        }
    }
}

impl fmt::Display for StringSearchOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl StringReplaceOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::First => "replace_first",
            Self::All => "replace_all",
        }
    }

    /// The `dump-mir` spelling of a regex replacement.
    #[must_use]
    pub const fn mir_regex_text(self) -> &'static str {
        match self {
            Self::First => "replace",
            Self::All => "replace_all",
        }
    }

    /// The `dump-hir` spelling of a regex replacement through a callback.
    #[must_use]
    pub const fn callback_text(self) -> &'static str {
        match self {
            Self::First => "replace_first_callback",
            Self::All => "replace_all_callback",
        }
    }

    /// The `dump-mir` spelling of a regex replacement through a callback.
    #[must_use]
    pub const fn mir_callback_text(self) -> &'static str {
        match self {
            Self::First => "replace_callback",
            Self::All => "replace_all_callback",
        }
    }
}

impl fmt::Display for StringReplaceOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl StringSliceKind {
    /// The `dump` spelling of a `StringSlice` with this kind.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Slice => "string_slice",
            Self::Substring => "string_substring",
            Self::Substr => "string_substr",
        }
    }
}

impl StringPadOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Start => "pad_start",
            Self::End => "pad_end",
        }
    }
}

impl fmt::Display for StringPadOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl StringPredicateOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::IsDigit => "isdigit",
            Self::IsAlpha => "isalpha",
            Self::IsAlnum => "isalnum",
        }
    }
}

impl fmt::Display for StringPredicateOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl ErasedPatternMethod {
    /// The method's JavaScript name, which is also its `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Match => "match",
            Self::Search => "search",
        }
    }
}

impl fmt::Display for ErasedPatternMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl RegexMatchOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Match => "match",
            Self::FullMatch => "fullmatch",
        }
    }
}

impl fmt::Display for RegexMatchOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl SetRelationOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::IsSubset => "issubset",
            Self::IsSuperset => "issuperset",
            Self::IsProperSubset => "isproper_subset",
            Self::IsProperSuperset => "isproper_superset",
        }
    }
}

impl fmt::Display for SetRelationOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl SetRemoveOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Discard => "discard",
            Self::Remove => "remove",
        }
    }
}

impl fmt::Display for SetRemoveOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl SetBinaryOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Union => "union",
            Self::Intersection => "intersection",
            Self::Difference => "difference",
            Self::SymmetricDifference => "symmetric_difference",
        }
    }
}

impl fmt::Display for SetBinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl SetProjectionOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Values => "values",
            Self::Entries => "entries",
        }
    }
}

impl fmt::Display for SetProjectionOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl TextEncoderOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Encode => "encode",
            Self::Encoding => "encoding",
        }
    }
}

impl fmt::Display for TextEncoderOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl TextDecoderOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Decode => "decode",
            Self::Encoding => "encoding",
        }
    }
}

impl fmt::Display for TextDecoderOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl UrlSearchParamsOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::GetAll => "get_all",
            Self::Has => "has",
            Self::Set => "set",
            Self::Append => "append",
            Self::Delete => "delete",
            Self::Sort => "sort",
            Self::ToText => "to_text",
            Self::Keys => "keys",
            Self::Values => "values",
            Self::Entries => "entries",
        }
    }
}

impl fmt::Display for UrlSearchParamsOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl AbortSignalOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Abort => "abort",
            Self::Timeout => "timeout",
        }
    }
}

impl fmt::Display for AbortSignalOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl CryptoOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::RandomUuid => "random_uuid",
            Self::GetRandomValues => "get_random_values",
            Self::Digest => "digest",
            Self::ImportKey => "import_key",
            Self::Sign => "sign",
            Self::Verify => "verify",
        }
    }
}

impl fmt::Display for CryptoOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl FormDataOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::GetAll => "get_all",
            Self::Has => "has",
            Self::Set => "set",
            Self::Append => "append",
            Self::Delete => "delete",
            Self::Keys => "keys",
            Self::Values => "values",
            Self::Entries => "entries",
            Self::ForEach => "for_each",
        }
    }
}

impl fmt::Display for FormDataOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl EventEmitterOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Once => "once",
            Self::Off => "off",
            Self::RemoveAll => "remove_all",
            Self::Emit => "emit",
            Self::ListenerCount => "listener_count",
        }
    }
}

impl fmt::Display for EventEmitterOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl HttpServerOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Listen => "listen",
            Self::Close => "close",
            Self::Address => "address",
        }
    }
}

impl fmt::Display for HttpServerOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl IncomingMessageOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Method => "method",
            Self::Url => "url",
            Self::Headers => "headers",
        }
    }
}

impl fmt::Display for IncomingMessageOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl ServerResponseOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::StatusCode => "status_code",
            Self::SetStatusCode => "set_status_code",
            Self::SetHeader => "set_header",
            Self::GetHeader => "get_header",
            Self::WriteHead => "write_head",
            Self::Write => "write",
            Self::End => "end",
        }
    }
}

impl fmt::Display for ServerResponseOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl RequestOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Url => "url",
            Self::Method => "method",
            Self::Headers => "headers",
            Self::BodyUsed => "body_used",
            Self::Body => "body",
            Self::Text => "text",
            Self::Json => "json",
            Self::FormData => "form_data",
            Self::ArrayBuffer => "array_buffer",
            Self::Bytes => "bytes",
            Self::Clone => "clone",
            Self::Signal => "signal",
            Self::Init(member) => member.field_name(),
        }
    }
}

impl fmt::Display for RequestOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl ResponseOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Ok => "ok",
            Self::StatusText => "status_text",
            Self::Headers => "headers",
            Self::BodyUsed => "body_used",
            Self::Body => "body",
            Self::Text => "text",
            Self::Json => "json",
            Self::FormData => "form_data",
            Self::ArrayBuffer => "array_buffer",
            Self::Bytes => "bytes",
            Self::Clone => "clone",
        }
    }
}

impl fmt::Display for ResponseOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl HeadersOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Has => "has",
            Self::Set => "set",
            Self::Append => "append",
            Self::Delete => "delete",
            Self::Keys => "keys",
            Self::Values => "values",
            Self::Entries => "entries",
            Self::GetSetCookie => "get_set_cookie",
        }
    }
}

impl fmt::Display for HeadersOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl ListSearchOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Find => "find",
            Self::RFind => "rfind",
        }
    }
}

impl fmt::Display for ListSearchOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl ListCallbackOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Filter => "filter",
            Self::Find => "find_callback",
            Self::FindIndex => "find_index",
            Self::FindLast => "find_last_callback",
            Self::FindLastIndex => "find_last_index",
            Self::Some => "some",
            Self::Every => "every",
            Self::ForEach => "for_each",
            Self::FlatMap => "flat_map",
        }
    }
}

impl fmt::Display for ListCallbackOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl ListProjectionOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Keys => "keys",
            Self::Values => "values",
            Self::Entries => "entries",
        }
    }
}

impl fmt::Display for ListProjectionOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl BoolFoldOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Any => "any",
        }
    }
}

impl fmt::Display for BoolFoldOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl PropertyLookup {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::PrototypeChain => "chain",
        }
    }
}

impl fmt::Display for PropertyLookup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl DictProjectionOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::FromEntries => "from_entries",
            Self::Keys => "keys",
            Self::OwnKeys => "own_keys",
            Self::ForInKeys => "for_in_keys",
            Self::Symbols => "symbols",
            Self::Values => "values",
            Self::Entries => "entries",
        }
    }
}

impl fmt::Display for DictProjectionOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl UrlField {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Href => "href",
            Self::Protocol => "protocol",
            Self::Host => "host",
            Self::Origin => "origin",
            Self::Hostname => "hostname",
            Self::Username => "username",
            Self::Password => "password",
            Self::Port => "port",
            Self::Pathname => "pathname",
            Self::Search => "search",
            Self::Hash => "hash",
        }
    }
}

impl fmt::Display for UrlField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl BlobOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Size => "size",
            Self::Type => "type",
            Self::Name => "name",
            Self::LastModified => "last_modified",
            Self::Text => "text",
            Self::ArrayBuffer => "array_buffer",
            Self::Bytes => "bytes",
            Self::Slice => "slice",
        }
    }
}

impl fmt::Display for BlobOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl AsyncOp {
    /// The operation's `dump` spelling.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::All => "async_all",
            Self::Race => "async_race",
            Self::AllSettled => "async_all_settled",
            Self::Sleep => "async_sleep",
            Self::ExitDrain => "async_exit_drain",
            Self::SetTimeout => "async_set_timeout",
            Self::ClearTimeout => "async_clear_timeout",
            Self::SetInterval => "async_set_interval",
            Self::ClearInterval => "async_clear_interval",
            Self::Promise => "async_promise",
            Self::Then => "async_then",
            Self::Catch => "async_catch",
            Self::Finally => "async_finally",
            Self::SpawnLocal => "async_spawn_local",
            Self::CreateTask => "async_create_task",
            Self::WaitFor => "async_wait_for",
            Self::Resolve => "async_resolve",
            Self::Reject => "async_reject",
            Self::HttpGetText => "async_http_get_text",
            Self::HttpFetch => "async_http_fetch",
        }
    }
}

impl fmt::Display for AsyncOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}
