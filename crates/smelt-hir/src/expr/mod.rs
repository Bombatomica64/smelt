//! Expression and literal representation for HIR.

mod call;
mod control_flow;
mod core;
/// Split expression kind variants.
mod kinds;
mod literals;
/// Structural child-expression remapping over `ExprKind`.
mod map;
mod ops;
mod types;

/// `dump` spellings of the operation selectors.
mod op_text;

pub use call::{CaptureMode, ClosureCapture, ClosureExpr};
pub use control_flow::AsyncOp;
pub use core::Expr;
pub use kinds::{ExprKind, GeneratorResumeKind, ListSpliceItem, PropertyLookup};
pub use literals::{Literal, UnknownKind};
pub use ops::{
    AbortSignalOp, Base64Op, BigIntOp, BlobOp, BoolFoldOp, ByteArrayOp, CryptoOp, DatePart,
    DictProjectionOp, ErasedPatternMethod, EventEmitterOp, FormDataOp, HashOp, HeadersOp,
    HostModuleOp, HttpServerOp, IncomingMessageOp, ListCallbackOp, ListProjectionOp, ListSearchOp,
    NumericExtremaOp, NumericFormatMethod, NumericPredicateOp, NumericRoundOp, NumericUnaryFuncOp,
    PathOp, PrimitiveCastOp, PythonDivisionOp, RegexMatchOp, RegexReplaceArg, RequestInitMember,
    RequestOp, ResponseOp, ServerResponseOp, SetBinaryOp, SetProjectionOp, SetRelationOp,
    SetRemoveOp, SqliteOp, StringAffixOp, StringCaseOp, StringNormalizeForm, StringPadOp,
    StringPredicateOp, StringReplaceOp, StringSearchOp, StringSliceKind, StringTrimSide,
    TextDecoderOp, TextEncoderOp, UnboundRead, UriTranscodeOp, UrlField, UrlOp, UrlSearchParamsOp,
};
pub use types::{BinOp, UnaryOp, bin_op_text};
