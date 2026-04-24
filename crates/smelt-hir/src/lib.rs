use serde::{Deserialize, Serialize};

pub const CONSOLE_LOG_SYMBOL: &str = "console_log";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BodyId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExprId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StmtId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PatternId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    #[must_use]
    pub fn new(file: FileId, start: u32, end: u32) -> Self {
        Self { file, start, end }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Crate {
    pub modules: Vec<Module>,
    pub items: Vec<Item>,
    pub bodies: Vec<Body>,
    pub types: TypeInterner,
    pub symbols: SymbolInterner,
    pub names: OriginalNameTable,
}

impl Crate {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_module(&mut self, module: Module) -> ModuleId {
        let id = ModuleId(self.modules.len() as u32);
        self.modules.push(Module { id, ..module });
        id
    }

    pub fn push_item(&mut self, item: Item) -> ItemId {
        let id = ItemId(self.items.len() as u32);
        self.items.push(item);
        id
    }

    pub fn push_body(&mut self, body: Body) -> BodyId {
        let id = BodyId(self.bodies.len() as u32);
        self.bodies.push(Body { id, ..body });
        id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub id: ModuleId,
    pub name: String,
    pub source: SourceFile,
    pub imports: Vec<Import>,
    pub items: Vec<ItemId>,
    pub body: Option<BodyId>,
}

impl Module {
    #[must_use]
    pub fn new(name: impl Into<String>, source: SourceFile) -> Self {
        Self {
            id: ModuleId(u32::MAX),
            name: name.into(),
            source,
            imports: Vec::new(),
            items: Vec::new(),
            body: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: String,
    pub language: Language,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    TypeScript,
    Python,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub module: String,
    pub name: Symbol,
    pub alias: Option<Symbol>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Item {
    Function(Function),
    Class(Class),
    TypeAlias(TypeAlias),
    Const(ConstItem),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: Symbol,
    pub span: Span,
    pub params: Vec<Param>,
    pub return_ty: TypeId,
    pub is_async: bool,
    pub body: Option<BodyId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: Symbol,
    pub local: LocalId,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Class {
    pub name: Symbol,
    pub span: Span,
    pub fields: Vec<Field>,
    pub methods: Vec<ItemId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub name: Symbol,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeAlias {
    pub name: Symbol,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstItem {
    pub name: Symbol,
    pub ty: TypeId,
    pub value: ExprId,
    pub body: BodyId,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Body {
    pub id: BodyId,
    pub owner: Option<ItemId>,
    pub locals: Vec<LocalDecl>,
    pub params: Vec<LocalId>,
    pub exprs: Vec<Expr>,
    pub stmts: Vec<Stmt>,
    pub blocks: Vec<Block>,
    pub patterns: Vec<Pattern>,
    pub root: BlockId,
}

impl Body {
    #[must_use]
    pub fn new(owner: Option<ItemId>, span: Span) -> Self {
        Self {
            id: BodyId(u32::MAX),
            owner,
            locals: Vec::new(),
            params: Vec::new(),
            exprs: Vec::new(),
            stmts: Vec::new(),
            blocks: vec![Block {
                stmts: Vec::new(),
                tail: None,
                span,
            }],
            patterns: Vec::new(),
            root: BlockId(0),
        }
    }

    pub fn push_local(&mut self, local: LocalDecl) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(local);
        id
    }

    pub fn push_expr(&mut self, expr: Expr) -> ExprId {
        let id = ExprId(self.exprs.len() as u32);
        self.exprs.push(expr);
        id
    }

    pub fn push_stmt(&mut self, stmt: Stmt) -> StmtId {
        let id = StmtId(self.stmts.len() as u32);
        self.stmts.push(stmt);
        self.blocks[self.root.0 as usize].stmts.push(id);
        id
    }

    pub fn push_pattern(&mut self, pattern: Pattern) -> PatternId {
        let id = PatternId(self.patterns.len() as u32);
        self.patterns.push(pattern);
        id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDecl {
    pub name: Option<Symbol>,
    pub ty: TypeId,
    pub mutable: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Type {
    Bool,
    Int,
    Float,
    String,
    None,
    List(TypeId),
    Set(TypeId),
    Dict(TypeId, TypeId),
    Tuple(Vec<TypeId>),
    Optional(TypeId),
    Union(Vec<TypeId>),
    Class { name: Symbol, args: Vec<TypeId> },
    Function(FunctionType),
    Future(TypeId),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionType {
    pub params: Vec<TypeId>,
    pub return_ty: TypeId,
    pub is_async: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TypeInterner {
    types: Vec<Type>,
}

impl TypeInterner {
    pub fn intern(&mut self, ty: Type) -> TypeId {
        if let Some((idx, _)) = self
            .types
            .iter()
            .enumerate()
            .find(|(_, existing)| **existing == ty)
        {
            return TypeId(idx as u32);
        }
        let id = TypeId(self.types.len() as u32);
        self.types.push(ty);
        id
    }

    #[must_use]
    pub fn get(&self, id: TypeId) -> Option<&Type> {
        self.types.get(id.0 as usize)
    }

    #[must_use]
    pub fn all(&self) -> &[Type] {
        &self.types
    }
}

pub fn format_compact(krate: &Crate, modules: &[(String, ModuleId)]) -> String {
    let mut out = String::new();

    for (path, module_id) in modules {
        let module = &krate.modules[module_id.0 as usize];
        out.push_str(&format!("module {path} ({module_id:?})\n"));

        let Some(body_id) = module.body else {
            out.push_str("  <no body>\n\n");
            continue;
        };

        let body = &krate.bodies[body_id.0 as usize];
        out.push_str(&format!("  body {body_id:?}\n"));

        if !body.locals.is_empty() {
            out.push_str("  locals\n");
            for (idx, local) in body.locals.iter().enumerate() {
                let local_id = LocalId(idx as u32);
                let mutability = if local.mutable { "let" } else { "const" };
                let name = local
                    .name
                    .and_then(|symbol| {
                        krate
                            .names
                            .get(symbol)
                            .or_else(|| krate.symbols.get(symbol))
                    })
                    .unwrap_or("_");
                out.push_str(&format!(
                    "    {} {} {}: {}\n",
                    local_ref(local_id),
                    mutability,
                    name,
                    type_ref(krate, local.ty)
                ));
            }
        }

        if !body.exprs.is_empty() {
            out.push_str("  exprs\n");
            for (idx, expr) in body.exprs.iter().enumerate() {
                let expr_id = ExprId(idx as u32);
                out.push_str(&format!(
                    "    {}: {} = {}\n",
                    expr_ref(expr_id),
                    type_ref(krate, expr.ty),
                    expr_text(krate, expr)
                ));
            }
        }

        if !body.stmts.is_empty() {
            out.push_str("  stmts\n");
            for (idx, stmt) in body.stmts.iter().enumerate() {
                out.push_str(&format!("    s{}: {}\n", idx, stmt_text(krate, body, stmt)));
            }
        }

        out.push('\n');
    }

    out.push_str("interned types\n");
    for (idx, ty) in krate.types.all().iter().enumerate() {
        out.push_str(&format!("  t{} = {}\n", idx, type_text(krate, ty)));
    }

    out
}

fn stmt_text(krate: &Crate, body: &Body, stmt: &Stmt) -> String {
    match stmt {
        Stmt::Let { pat, ty, value } => {
            let value = value
                .map(|value| format!(" = {}", expr_ref(value)))
                .unwrap_or_default();
            format!(
                "let {}: {}{}",
                pattern_text(body, *pat),
                type_ref(krate, *ty),
                value
            )
        }
        Stmt::Expr(expr) => expr_ref(*expr),
        Stmt::Return(Some(expr)) => format!("return {}", expr_ref(*expr)),
        Stmt::Return(None) => "return".to_owned(),
        Stmt::Throw(expr) => format!("throw {}", expr_ref(*expr)),
        Stmt::Break => "break".to_owned(),
        Stmt::Continue => "continue".to_owned(),
    }
}

fn expr_text(krate: &Crate, expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Literal(literal) => literal_text(literal),
        ExprKind::Local(local) => local_ref(*local),
        ExprKind::Item(item) => item_ref(krate, *item),
        ExprKind::Call { callee, args } => {
            let args = args
                .iter()
                .map(|arg| expr_ref(*arg))
                .collect::<Vec<_>>()
                .join(", ");
            format!("call {}({})", expr_ref(*callee), args)
        }
        ExprKind::Method {
            receiver,
            method,
            args,
        } => {
            let method = krate.symbols.get(*method).unwrap_or("<unknown>");
            let args = args
                .iter()
                .map(|arg| expr_ref(*arg))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}.{}({})", expr_ref(*receiver), method, args)
        }
        ExprKind::Field { receiver, field } => {
            let field = krate.symbols.get(*field).unwrap_or("<unknown>");
            format!("{}.{}", expr_ref(*receiver), field)
        }
        ExprKind::Index { receiver, index } => {
            format!("{}[{}]", expr_ref(*receiver), expr_ref(*index))
        }
        ExprKind::BinOp { op, lhs, rhs } => {
            format!("{op:?} {}, {}", expr_ref(*lhs), expr_ref(*rhs))
        }
        ExprKind::UnaryOp { op, operand } => format!("{op:?} {}", expr_ref(*operand)),
        ExprKind::Block(block) => format!("block {block:?}"),
        ExprKind::Lambda { body, return_ty } => {
            format!("lambda {body:?} -> {}", type_ref(krate, *return_ty))
        }
        ExprKind::ListLit(items) => collection_text("[", "]", items),
        ExprKind::SetLit(items) => collection_text("set{", "}", items),
        ExprKind::DictLit(items) => {
            let items = items
                .iter()
                .map(|(key, value)| format!("{}: {}", expr_ref(*key), expr_ref(*value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{items}}}")
        }
        ExprKind::TupleLit(items) => collection_text("(", ")", items),
        ExprKind::New { class, args } => {
            let class = krate.symbols.get(*class).unwrap_or("<unknown>");
            let args = args
                .iter()
                .map(|arg| expr_ref(*arg))
                .collect::<Vec<_>>()
                .join(", ");
            format!("new {class}({args})")
        }
    }
}

fn collection_text(open: &str, close: &str, items: &[ExprId]) -> String {
    let items = items
        .iter()
        .map(|item| expr_ref(*item))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{open}{items}{close}")
}

fn pattern_text(body: &Body, pattern: PatternId) -> String {
    match &body.patterns[pattern.0 as usize] {
        Pattern::Wildcard => "_".to_owned(),
        Pattern::Binding(local) => local_ref(*local),
        Pattern::Tuple(items) => {
            let items = items
                .iter()
                .map(|item| pattern_text(body, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({items})")
        }
        Pattern::Literal(literal) => literal_text(literal),
    }
}

fn literal_text(literal: &Literal) -> String {
    match literal {
        Literal::Bool(value) => value.to_string(),
        Literal::Int(value) => value.to_string(),
        Literal::Float(value) => {
            if value.fract() == 0.0 {
                format!("{value:.1}")
            } else {
                value.to_string()
            }
        }
        Literal::String(value) => format!("\"{value}\""),
        Literal::None => "none".to_owned(),
    }
}

fn item_ref(krate: &Crate, item: ItemId) -> String {
    let Some(item_value) = krate.items.get(item.0 as usize) else {
        return format!("item{}", item.0);
    };

    let name = match item_value {
        Item::Function(function) => krate.symbols.get(function.name),
        Item::Class(class) => krate.symbols.get(class.name),
        Item::TypeAlias(alias) => krate.symbols.get(alias.name),
        Item::Const(item) => krate.symbols.get(item.name),
    }
    .unwrap_or("<unknown>");

    format!("@{}({})", item.0, name)
}

fn type_ref(krate: &Crate, ty: TypeId) -> String {
    let Some(ty_value) = krate.types.get(ty) else {
        return format!("t{}", ty.0);
    };
    type_text(krate, ty_value)
}

fn type_text(krate: &Crate, ty: &Type) -> String {
    match ty {
        Type::Bool => "Bool".to_owned(),
        Type::Int => "Int".to_owned(),
        Type::Float => "Float".to_owned(),
        Type::String => "String".to_owned(),
        Type::None => "None".to_owned(),
        Type::List(item) => format!("List<{}>", type_ref(krate, *item)),
        Type::Set(item) => format!("Set<{}>", type_ref(krate, *item)),
        Type::Dict(key, value) => {
            format!(
                "Dict<{}, {}>",
                type_ref(krate, *key),
                type_ref(krate, *value)
            )
        }
        Type::Tuple(items) => {
            let items = items
                .iter()
                .map(|item| type_ref(krate, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({items})")
        }
        Type::Optional(item) => format!("Optional<{}>", type_ref(krate, *item)),
        Type::Union(items) => items
            .iter()
            .map(|item| type_ref(krate, *item))
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Class { name, args } => {
            let name = krate.symbols.get(*name).unwrap_or("<unknown>");
            if args.is_empty() {
                name.to_owned()
            } else {
                let args = args
                    .iter()
                    .map(|arg| type_ref(krate, *arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{name}<{args}>")
            }
        }
        Type::Function(function) => {
            let params = function
                .params
                .iter()
                .map(|param| type_ref(krate, *param))
                .collect::<Vec<_>>()
                .join(", ");
            let async_prefix = if function.is_async { "async " } else { "" };
            format!(
                "{async_prefix}fn({params}) -> {}",
                type_ref(krate, function.return_ty)
            )
        }
        Type::Future(item) => format!("Future<{}>", type_ref(krate, *item)),
    }
}

fn local_ref(local: LocalId) -> String {
    format!("%{}", local.0)
}

fn expr_ref(expr: ExprId) -> String {
    format!("#{}", expr.0)
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SymbolInterner {
    symbols: Vec<String>,
}

impl SymbolInterner {
    pub fn intern(&mut self, value: &str) -> Symbol {
        if let Some((idx, _)) = self
            .symbols
            .iter()
            .enumerate()
            .find(|(_, existing)| existing.as_str() == value)
        {
            return Symbol(idx as u32);
        }
        let id = Symbol(self.symbols.len() as u32);
        self.symbols.push(value.to_owned());
        id
    }

    #[must_use]
    pub fn get(&self, symbol: Symbol) -> Option<&str> {
        self.symbols.get(symbol.0 as usize).map(String::as_str)
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct OriginalNameTable {
    names: Vec<Option<String>>,
}

impl OriginalNameTable {
    pub fn record(&mut self, symbol: Symbol, original: impl Into<String>) {
        let idx = symbol.0 as usize;
        if self.names.len() <= idx {
            self.names.resize_with(idx + 1, || None);
        }
        self.names[idx] = Some(original.into());
    }

    #[must_use]
    pub fn get(&self, symbol: Symbol) -> Option<&str> {
        self.names.get(symbol.0 as usize).and_then(Option::as_deref)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExprKind {
    Literal(Literal),
    Local(LocalId),
    Item(ItemId),
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    Method {
        receiver: ExprId,
        method: Symbol,
        args: Vec<ExprId>,
    },
    Field {
        receiver: ExprId,
        field: Symbol,
    },
    Index {
        receiver: ExprId,
        index: ExprId,
    },
    BinOp {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    UnaryOp {
        op: UnaryOp,
        operand: ExprId,
    },
    Block(BlockId),
    Lambda {
        body: BodyId,
        return_ty: TypeId,
    },
    ListLit(Vec<ExprId>),
    SetLit(Vec<ExprId>),
    DictLit(Vec<(ExprId, ExprId)>),
    TupleLit(Vec<ExprId>),
    New {
        class: Symbol,
        args: Vec<ExprId>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Literal {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub stmts: Vec<StmtId>,
    pub tail: Option<ExprId>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    Let {
        pat: PatternId,
        ty: TypeId,
        value: Option<ExprId>,
    },
    Expr(ExprId),
    Return(Option<ExprId>),
    Throw(ExprId),
    Break,
    Continue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Pattern {
    Wildcard,
    Binding(LocalId),
    Tuple(Vec<PatternId>),
    Literal(Literal),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub message: String,
}

#[must_use]
pub fn validate(krate: &Crate) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    for (body_idx, body) in krate.bodies.iter().enumerate() {
        for (expr_idx, expr) in body.exprs.iter().enumerate() {
            if krate.types.get(expr.ty).is_none() {
                errors.push(ValidationError {
                    message: format!(
                        "body {body_idx} expr {expr_idx} has unknown type {:?}",
                        expr.ty
                    ),
                });
            }
            if let ExprKind::Local(local) = expr.kind {
                if body.locals.get(local.0 as usize).is_none() {
                    errors.push(ValidationError {
                        message: format!(
                            "body {body_idx} expr {expr_idx} reads unknown local {:?}",
                            local
                        ),
                    });
                }
            }
        }
    }

    errors
}
