//! Emission of Python's `collections` containers (`deque`, `Counter`,
//! `defaultdict`, `OrderedDict`).
//!
//! Each container is a concrete generic runtime type (`smelt-runtime`'s
//! `py_collections.rs`): `deque[T]` renders as `SmeltPyDeque<T>`, `Counter[K]`
//! as `SmeltPyCounter<K>`, and the two mappings as `SmeltPyDefaultDict<K, V>`
//! and `SmeltPyOrderedDict<K, V>`. The frontend has already proved every
//! operand's type, so an operation is one method call on the receiver: the
//! variant names the method ([`PyCollectionMethod::rust_name`]), a key the
//! method only reads is passed by reference, and every other operand by value.
//! A fallible method returns `Result` and is rendered as a propagating call by
//! the fallible-op path; the rest are plain assignments.

use smelt_hir::{PyCollectionCtor, PyCollectionMethod, Symbol, Type, TypeId};
use smelt_mir::Operand;
use smelt_stdlib::StdlibClass;

use super::{EmitError, FunctionEmitter, RustExpr};

impl FunctionEmitter<'_> {
    /// The runtime type a `collections` class symbol renders as, if `name` is
    /// one of the four containers.
    pub(super) fn py_collection_rust_name(
        &self,
        name: Symbol,
    ) -> Result<Option<&'static str>, EmitError> {
        Ok(match self.stdlib_class_of_symbol(name)? {
            Some(StdlibClass::PyDeque) => Some("SmeltPyDeque"),
            Some(StdlibClass::PyCounter) => Some("SmeltPyCounter"),
            Some(StdlibClass::PyDefaultDict) => Some("SmeltPyDefaultDict"),
            Some(StdlibClass::PyOrderedDict) => Some("SmeltPyOrderedDict"),
            _ => None,
        })
    }

    /// The class and type arguments of a `collections` container type.
    fn py_collection_class(&self, ty: TypeId) -> Result<(StdlibClass, Vec<TypeId>), EmitError> {
        if let Some(Type::Class { name, args }) = self.mir.types.get(ty)
            && let Some(class) = self.stdlib_class_of_symbol(*name)?
            && matches!(
                class,
                StdlibClass::PyDeque
                    | StdlibClass::PyCounter
                    | StdlibClass::PyDefaultDict
                    | StdlibClass::PyOrderedDict
            )
        {
            return Ok((class, args.clone()));
        }
        Err(EmitError::new(
            "internal: a Python collection operation needs a deque, Counter, defaultdict or OrderedDict receiver",
        ))
    }

    /// Whether `ty` is a `collections` container whose `repr` the runtime
    /// writes: every type argument has a Python `repr` of its own.
    pub(super) fn py_collection_has_repr(&self, ty: TypeId) -> bool {
        self.py_collection_class(ty)
            .is_ok_and(|(_, args)| args.iter().all(|arg| self.type_has_py_repr(*arg)))
    }

    /// `<Container<..>>::new(..)`: a new container of the destination type.
    ///
    /// The type is spelled from the destination so an empty initial list
    /// (`deque()`) still names its element type. A `defaultdict` factory is
    /// wrapped in a `move` closure that calls it, which accepts a bare closure
    /// and a shared `Rc<dyn Fn>` alike.
    pub(super) fn py_collection_new_expr(
        &self,
        ctor: PyCollectionCtor,
        args: &[Operand],
        dest_ty: Option<TypeId>,
    ) -> Result<(RustExpr, TypeId), EmitError> {
        let ty = dest_ty.ok_or_else(|| {
            EmitError::new("internal: a Python collection constructor needs its destination type")
        })?;
        self.py_collection_class(ty)?;
        let container = self.rendered_type(ty)?;
        let mut rendered = Vec::with_capacity(args.len());
        for (index, arg) in args.iter().enumerate() {
            rendered.push(match (ctor, index) {
                (PyCollectionCtor::DefaultDict, 0) => {
                    if self.operand_closure_can_throw(arg)? {
                        return Err(EmitError::new(
                            "a defaultdict factory that can raise is not supported",
                        ));
                    }
                    format!(
                        "{{ let smelt_factory = {factory}; move || (smelt_factory)() }}",
                        factory = self.operand_expr(arg)?.delimited()
                    )
                }
                (PyCollectionCtor::Deque, 1) => self.py_collection_optional_int(arg)?,
                (PyCollectionCtor::DefaultDict, 1) => {
                    format!("&{}", self.operand_borrow_expr(arg)?.tight())
                }
                _ => self.operand_expr(arg)?.delimited().to_string(),
            });
        }
        Ok((
            RustExpr::atom(format!("<{container}>::{}({})", ctor.rust_name(), rendered.join(", "))),
            ty,
        ))
    }

    /// `receiver.method(args)` for one container method, and the type the
    /// call answers.
    pub(super) fn py_collection_op_expr(
        &self,
        method: PyCollectionMethod,
        receiver: &Operand,
        args: &[Operand],
    ) -> Result<(RustExpr, TypeId), EmitError> {
        let receiver_ty = self.operand_ty(receiver)?;
        let source_ty = self.py_collection_result_ty(method, receiver_ty)?;
        let receiver_text = self.operand_borrow_expr(receiver)?;
        let mut rendered = Vec::with_capacity(args.len());
        for (index, arg) in args.iter().enumerate() {
            rendered.push(if index == 0 && method.first_arg_by_ref() {
                format!("&{}", self.operand_borrow_expr(arg)?.tight())
            } else if method == PyCollectionMethod::MostCommon {
                self.py_collection_optional_int(arg)?
            } else {
                self.operand_expr(arg)?.delimited().to_string()
            });
        }
        Ok((
            receiver_text.postfix(format!(".{}({})", method.rust_name(), rendered.join(", "))),
            source_ty,
        ))
    }

    /// An optional machine-integer operand (a deque bound, a `most_common`
    /// count) as the runtime's `Option<i64>`: `None` for the `None` literal
    /// the frontend passes when the source gives none.
    fn py_collection_optional_int(&self, arg: &Operand) -> Result<String, EmitError> {
        Ok(match self.mir.types.get(self.operand_ty(arg)?) {
            Some(Type::None) => "None".to_owned(),
            Some(Type::Optional(..)) => self.operand_expr(arg)?.delimited().to_string(),
            _ => format!("Some({})", self.operand_expr(arg)?.delimited()),
        })
    }

    /// The type a container method answers, from the receiver's class and
    /// type arguments (the same table the frontend types the call by).
    fn py_collection_result_ty(
        &self,
        method: PyCollectionMethod,
        receiver_ty: TypeId,
    ) -> Result<TypeId, EmitError> {
        let (class, args) = self.py_collection_class(receiver_ty)?;
        let first = args.first().copied().ok_or_else(|| {
            EmitError::new("internal: a Python collection type has no type argument")
        })?;
        let py_int = self.type_id(Type::PyInt)?;
        // The mapped value: a `Counter`'s is its `int` count.
        let value = if class == StdlibClass::PyCounter {
            py_int
        } else {
            args.get(1).copied().unwrap_or(first)
        };
        Ok(match method {
            PyCollectionMethod::Append
            | PyCollectionMethod::AppendLeft
            | PyCollectionMethod::Extend
            | PyCollectionMethod::ExtendLeft
            | PyCollectionMethod::Rotate
            | PyCollectionMethod::Clear
            | PyCollectionMethod::SetIndex
            | PyCollectionMethod::Remove
            | PyCollectionMethod::Reverse
            | PyCollectionMethod::Set
            | PyCollectionMethod::CounterDelete
            | PyCollectionMethod::MappingDelete
            | PyCollectionMethod::Update
            | PyCollectionMethod::UpdateCounts
            | PyCollectionMethod::Subtract
            | PyCollectionMethod::SubtractCounts
            | PyCollectionMethod::MoveToEnd => self.type_id(Type::None)?,
            PyCollectionMethod::DequePop
            | PyCollectionMethod::PopLeft
            | PyCollectionMethod::GetIndex => first,
            PyCollectionMethod::Len => self.type_id(Type::Int)?,
            PyCollectionMethod::Contains | PyCollectionMethod::Equals => {
                self.type_id(Type::Bool)?
            }
            PyCollectionMethod::Count | PyCollectionMethod::Index | PyCollectionMethod::Total => {
                py_int
            }
            PyCollectionMethod::Copy | PyCollectionMethod::Plus | PyCollectionMethod::Minus => {
                receiver_ty
            }
            PyCollectionMethod::MaxLen => {
                self.type_id(Type::Optional(py_int, smelt_hir::Absence::Undefined))?
            }
            PyCollectionMethod::Items if class == StdlibClass::PyDeque => {
                self.type_id(Type::List(first))?
            }
            PyCollectionMethod::Items | PyCollectionMethod::MostCommon => {
                let pair = self.type_id(Type::Tuple(vec![first, value]))?;
                self.type_id(Type::List(pair))?
            }
            PyCollectionMethod::Keys | PyCollectionMethod::Elements => {
                self.type_id(Type::List(first))?
            }
            PyCollectionMethod::Values => self.type_id(Type::List(value))?,
            PyCollectionMethod::Get if class == StdlibClass::PyCounter => py_int,
            PyCollectionMethod::Get => {
                self.type_id(Type::Optional(value, smelt_hir::Absence::Undefined))?
            }
            PyCollectionMethod::GetOr
            | PyCollectionMethod::MappingPop
            | PyCollectionMethod::PopOr
            | PyCollectionMethod::SetDefault
            | PyCollectionMethod::GetOrInsert
            | PyCollectionMethod::GetItem => value,
            PyCollectionMethod::PopItem => self.type_id(Type::Tuple(vec![first, value]))?,
        })
    }
}
