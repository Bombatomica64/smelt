//! Fetch init annotations: the ambient `RequestInit`/`ResponseInit`/`HeadersInit`
//! interfaces and the types of their fields.

use crate::lowering::ModuleBuilder;
use crate::lowering::support::source_spelling;
use smelt_hir::{Field, Type, Visibility};
use smelt_stdlib::FetchInitKeyKind;

impl ModuleBuilder<'_> {
    /// The declared field table for an ambient fetch init interface.
    ///
    /// One table, read two ways: [`Self::fetch_init_field_type`] answers a
    /// single key for a direct field read, and this answers the whole set so a
    /// utility type (`Omit`, `Required`, ...) can compose over it. Keeping them
    /// as one source is the point -- a second table would drift.
    ///
    /// Every key is optional, matching the interfaces themselves; `Required<..>`
    /// is what flips that, and it does so through the shared utility path.
    pub(in crate::lowering) fn ambient_fetch_init_fields(
        &mut self,
        name_text: &str,
    ) -> Option<Vec<Field>> {
        let bare = name_text.strip_prefix("globalThis.").unwrap_or(name_text);
        let keys = smelt_stdlib::fetch_init_keys(bare)?;
        let class = self.intern_type_name(bare);
        let mut fields = Vec::new();
        for (key, _) in keys {
            let field = self.intern_source_name(key);
            // The registry answers with the key's OPTIONAL type; a field table
            // carries the bare type plus an `optional` flag, so unwrap the one
            // to build the other.
            let Some(ty) = self.fetch_init_field_type(class, field) else {
                continue;
            };
            let inner = match self.ctx.krate.types.get(ty) {
                Some(&Type::Optional(inner)) => inner,
                _ => ty,
            };
            fields.push(Field {
                name: field,
                ty: inner,
                optional: true,
                visibility: Visibility::Public,
                span: self.span(0, 0),
            });
        }
        Some(fields)
    }

    /// Resolve a field on one of the ambient fetch **init** interfaces.
    ///
    /// `ResponseInit` and `RequestInit` are declared in
    /// lib.dom/undici, which Smelt does not import, so a value annotated with
    /// one is an opaque class with no declared fields and `init.status`
    /// resolved to `Unknown`. The field types are statically known all the
    /// same, so they are declared here — the same reason `SmeltMatch`'s and
    /// `URLSearchParams.size`'s entries exist in `builtin_class_field_type`.
    ///
    /// Every key is **optional** (`status?: number`), so each resolves to an
    /// `Optional<T>`; that is what makes the absent case fall back to the
    /// spec's default at the construction site rather than to a wrong value.
    ///
    /// A key outside the modeled set resolves to `None` here, which leaves the
    /// ordinary paths to reject the read — an unmodeled init key must not
    /// silently read as an erased value.
    pub(super) fn fetch_init_field_type(
        &mut self,
        class: smelt_hir::Symbol,
        field: smelt_hir::Symbol,
    ) -> Option<smelt_hir::TypeId> {
        let class_name = source_spelling(&self.ctx.krate, class)?
            .to_owned();
        // Source names are normalized to snake_case in the symbol table, with
        // the original spelling kept in `names`, so both are accepted: a key
        // written `statusText` interns as `status_text`, and which one arrives
        // here depends on the table the caller's symbol came from.
        let field_name = source_spelling(&self.ctx.krate, field)?
            .to_owned();
        // A site may qualify the ambient interface to escape a local one of the
        // same name (`globalThis.ResponseInit` where the module declares its
        // own `ResponseInit`). The qualified reference keeps its full path as a
        // distinct type, which is what makes the two tell apart at all, so the
        // registry recognizes both spellings of the ambient one.
        let class_name = class_name
            .strip_prefix("globalThis.")
            .unwrap_or(&class_name)
            .to_owned();
        let inner = match (class_name.as_str(), field_name.as_str()) {
            ("ResponseInit", "status") => self.ctx.krate.types.intern(Type::Float),
            ("ResponseInit", "statusText" | "status_text") | ("RequestInit", "method") => {
                self.ctx.krate.types.intern(Type::String)
            }
            // `body` is a `BodyInit`, whose modeled arm is a string; the other
            // arms are types Smelt does not model yet, and the construction
            // site names them rather than guessing.
            ("RequestInit", "body") => self.ctx.krate.types.intern(Type::String),
            // `headers` is a `HeadersInit`: a `Headers`, a record, or an array
            // of pairs. The modeled init type is the concrete `Headers`, which
            // is what the construction site's conversion accepts directly; the
            // other spellings reach it as literals, which keep their own types.
            ("ResponseInit" | "RequestInit", "headers") => self.headers_type(),
            ("RequestInit", "signal") => self.abort_signal_type(),
            // The STORED `RequestInit` members: WebIDL enumerations, which
            // surface as `string`, plus the one `boolean`. Read from the same
            // table the request itself stores them in, so the init surface and
            // the getter surface cannot disagree about which keys exist.
            ("RequestInit", field_name) => {
                let member = smelt_hir::RequestInitMember::from_property_name(field_name)
                    .or_else(|| {
                        smelt_hir::RequestInitMember::ALL
                            .into_iter()
                            .find(|member| member.field_name() == field_name)
                    })?;
                if member.is_boolean() {
                    self.ctx.krate.types.intern(Type::Bool)
                } else {
                    self.ctx.krate.types.intern(Type::String)
                }
            }
            _ => return None,
        };
        Some(self.ctx.krate.types.intern(Type::Optional(inner)))
    }

    /// The ambient `AbortSignal` class type.
    fn abort_signal_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("AbortSignal");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Materialize an ambient fetch INIT dictionary as a real interface.
    ///
    /// `ResponseInit` and `RequestInit` are lib.d.ts dictionaries: bags of
    /// optional keys with no methods and no identity. Round 4 modeled them as
    /// opaque classes, so a value of one arrived as an erased record and every
    /// key was read through a checked cast. That made `init.headers` a
    /// `SmeltUnknown` at a `new Headers(..)` site — the full Hono crate's stop
    /// — and worse, the cast tried to recover a `Headers` from a plain record
    /// literal and answered an EMPTY header list: a silent wrong value.
    ///
    /// They are declared here instead, as an interface with the spec's optional
    /// typed keys, so a parameter typed by one is a real generated struct: its
    /// fields read with their own types, an object literal at the call site
    /// converts into it through the ordinary record-to-struct adapter, and
    /// `headers` keeps the union WHATWG gives it (`HeadersInit`) rather than
    /// being narrowed to the `Headers` arm alone.
    ///
    /// A source declaration of the same name wins for the bare spelling: Hono declares its own
    /// `interface ResponseInit<T extends StatusCode>` and refers to the ambient
    /// one as `globalThis.ResponseInit` precisely to escape it. The answer comes
    /// from the module's facts, because a declaration later in the file must win
    /// over a reference earlier in it.
    pub(super) fn ambient_init_interface_type(&mut self, name_text: &str) -> Option<smelt_hir::TypeId> {
        let bare = name_text.strip_prefix("globalThis.").unwrap_or(name_text);
        let keys = smelt_stdlib::fetch_init_keys(bare)?;
        // A source declaration of the BARE name wins for the bare spelling —
        // Hono declares its own `interface ResponseInit<T extends StatusCode>`
        // and must keep it. The `globalThis.`-qualified spelling is the source
        // asking for the AMBIENT one by name, which is exactly why Hono writes
        // it at the site that forwards an init to the constructor, so a local
        // declaration cannot shadow that.
        if bare == name_text && self.source_declares_type(bare) {
            return None;
        }
        let name = self.intern_type_name(name_text);
        let ty = self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        });
        // Declared once per crate: a second reference finds the interface and
        // reuses it, which also keeps the emitted struct single.
        if self.find_interface(name).is_some() {
            return Some(ty);
        }
        let span = self.span(0, 0);
        let fields = keys
            .iter()
            .map(|(key, kind)| {
                let field_ty = match kind {
                    FetchInitKeyKind::Number => self.ctx.krate.types.intern(Type::Float),
                    FetchInitKeyKind::Text => self.ctx.krate.types.intern(Type::String),
                    FetchInitKeyKind::Boolean => self.ctx.krate.types.intern(Type::Bool),
                    FetchInitKeyKind::HeadersInit => self.headers_init_type(),
                    FetchInitKeyKind::AbortSignal => self.abort_signal_type(),
                };
                let optional_ty = self.ctx.krate.types.intern(Type::Optional(field_ty));
                Field {
                    name: self.intern_source_name(key),
                    ty: optional_ty,
                    visibility: Visibility::Public,
                    optional: true,
                    span,
                }
            })
            .collect::<Vec<_>>();
        let item = self.ctx.krate.upsert_interface(smelt_hir::Interface {
            name,
            span,
            type_params: Vec::new(),
            extends: Vec::new(),
            fields,
            methods: Vec::new(),
            decode_facts: Vec::new(),
        });
        self.interfaces.register_lowered(crate::lowering::state::interface_registry::LoweredInterface {
            name,
            name_text: name_text.to_owned(),
            item,
            extends: Vec::new(),
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            index_value_ty: None,
        });
        Some(ty)
    }

    /// The `HeadersInit` union: a pair list, a record, or a `Headers`.
    ///
    /// WHATWG spells the constructor's argument as a union and so does every
    /// init dictionary that carries headers, so the arms stay a union here. The
    /// construction site dispatches on them (see `headers_conversion_text`);
    /// narrowing this to the `Headers` arm alone is what made a record literal
    /// recover as an empty header list.
    fn headers_init_type(&mut self) -> smelt_hir::TypeId {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let pair_ty = self
            .ctx
            .krate
            .types
            .intern(Type::Tuple(vec![string_ty, string_ty]));
        let pair_list_ty = self.ctx.krate.types.intern(Type::List(pair_ty));
        let record_ty = self
            .ctx
            .krate
            .types
            .intern(Type::Dict(string_ty, string_ty));
        let headers_ty = self.headers_type();
        self.ctx
            .krate
            .types
            .intern(Type::Union(vec![pair_list_ty, record_ty, headers_ty]))
    }
}
