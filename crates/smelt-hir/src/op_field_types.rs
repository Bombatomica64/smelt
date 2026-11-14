//! Type positions inside the op table's carried (`[T]`) fields.
//!
//! An op field of kind `(ty)` is a `TypeId` and every consumer that walks type
//! positions visits it directly. A carried `[T]` field is opaque to the table
//! grammar, so whether it holds a type has to be said by `T` itself: the MIR
//! type-position visitor calls [`OpFieldTypes`] on every carried field, which
//! means adding an op with a `[T]` field whose `T` has no impl here is a
//! compile error. The author then has to state, once, where `T` keeps its
//! types: `JsonParse.target` was a `[Option<TypeId>]` that
//! normalization and validation both skipped).

use crate::{Symbol, TypeId};

/// The type ids a carried op field holds.
pub trait OpFieldTypes {
    /// Visit every type id in `self`.
    fn visit_types(&self, visit: &mut dyn FnMut(TypeId));
    /// Rewrite every type id in `self`.
    fn map_types(&mut self, map: &mut dyn FnMut(TypeId) -> TypeId);
}

impl OpFieldTypes for TypeId {
    fn visit_types(&self, visit: &mut dyn FnMut(TypeId)) {
        visit(*self);
    }

    fn map_types(&mut self, map: &mut dyn FnMut(TypeId) -> TypeId) {
        *self = map(*self);
    }
}

impl<T: OpFieldTypes> OpFieldTypes for Option<T> {
    fn visit_types(&self, visit: &mut dyn FnMut(TypeId)) {
        if let Some(inner) = self {
            inner.visit_types(visit);
        }
    }

    fn map_types(&mut self, map: &mut dyn FnMut(TypeId) -> TypeId) {
        if let Some(inner) = self {
            inner.map_types(map);
        }
    }
}

impl<T: OpFieldTypes> OpFieldTypes for Vec<T> {
    fn visit_types(&self, visit: &mut dyn FnMut(TypeId)) {
        for inner in self {
            inner.visit_types(visit);
        }
    }

    fn map_types(&mut self, map: &mut dyn FnMut(TypeId) -> TypeId) {
        for inner in self {
            inner.map_types(map);
        }
    }
}

/// Declares that each listed carried-field type holds no type id.
///
/// Only list a type here after checking its definition: a type that does hold
/// a `TypeId` needs a real impl, or normalization and validation skip it.
macro_rules! holds_no_types {
    ($($ty:ty),* $(,)?) => {
        $(
            impl OpFieldTypes for $ty {
                fn visit_types(&self, _visit: &mut dyn FnMut(TypeId)) {}
                fn map_types(&mut self, _map: &mut dyn FnMut(TypeId) -> TypeId) {}
            }
        )*
    };
}

holds_no_types!(
    bool,
    usize,
    String,
    Symbol,
    crate::AbortSignalOp,
    crate::AsyncOp,
    crate::Base64Op,
    crate::BigIntOp,
    crate::BlobOp,
    crate::BoolFoldOp,
    crate::ByteArrayOp,
    crate::CryptoOp,
    crate::DatePart,
    crate::DictProjectionOp,
    crate::EventEmitterOp,
    crate::FormDataOp,
    crate::GeneratorResumeKind,
    crate::HeadersOp,
    crate::HostModuleOp,
    crate::HttpServerOp,
    crate::IncomingMessageOp,
    crate::ListCallbackOp,
    crate::ListProjectionOp,
    crate::ListSearchOp,
    crate::NumericExtremaOp,
    crate::NumericPredicateOp,
    crate::NumericRoundOp,
    crate::NumericUnaryFuncOp,
    crate::PrimitiveCastOp,
    crate::PropertyLookup,
    crate::PythonDivisionOp,
    crate::NumericFormatMethod,
    crate::RegexMatchOp,
    crate::ErasedPatternMethod,
    crate::RegexReplaceArg,
    crate::RequestOp,
    crate::ResponseOp,
    crate::ServerResponseOp,
    crate::SetBinaryOp,
    crate::SetProjectionOp,
    crate::SetRelationOp,
    crate::SetRemoveOp,
    crate::SqliteOp,
    crate::StringAffixOp,
    crate::StringCaseOp,
    crate::StringNormalizeForm,
    crate::StringPadOp,
    crate::StringPredicateOp,
    crate::StringReplaceOp,
    crate::StringSearchOp,
    crate::StringSliceKind,
    crate::StringTrimSide,
    crate::TextDecoderOp,
    crate::TextEncoderOp,
    crate::TypedArrayElement,
    crate::UnknownKind,
    crate::UnboundRead,
    crate::UriTranscodeOp,
    crate::UrlField,
    crate::UrlOp,
    crate::UrlSearchParamsOp,
);
