//! Complete TypeScript AST types, diagnostics, and span primitives.

pub use compact_str::CompactString;

/// Compact string type used throughout the AST. Stores strings ≤ 24 bytes inline
/// (no heap allocation). ~95% of JS/TS identifiers fit inline. Drop-in replacement
/// for `String` — implements `Deref<Target=str>`, `From<String>`, `From<&str>`, etc.
pub type AstString = CompactString;

pub type NodeId = u32;
pub type SymbolId = u32;
pub type TypeId = u32;

// ---------------------------------------------------------------------------
// Compile-time AST size assertions — catch regressions when node sizes grow.
// StmtKind/ExprKind target: 32 bytes (24-byte data + 8-byte discriminant).
// TypeNodeKind target: 40 bytes.
// ---------------------------------------------------------------------------
const _: () = assert!(std::mem::size_of::<StmtKind>() <= 32);
const _: () = assert!(std::mem::size_of::<ExprKind>() <= 32);
const _: () = assert!(std::mem::size_of::<TypeNodeKind>() <= 40);
const _: () = assert!(std::mem::size_of::<Stmt>() <= 40);
const _: () = assert!(std::mem::size_of::<Expr>() <= 40);

// ---------------------------------------------------------------------------
// Stable position-based node identity (Phase 1 primitive)
// ---------------------------------------------------------------------------

/// A stable, position-based AST node identifier.
///
/// Unlike the arena-index `NodeId` (a `u32`), `StableNodeId` is a `u64`
/// hash derived from (file_path, byte_offset, SyntaxKind). This allows nodes
/// to be identified across incremental rebuilds as long as they haven't moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StableNodeId(pub u64);

impl StableNodeId {
    /// Compute a stable node id from file path, byte offset, and syntax kind tag.
    pub fn new(file_path: &str, byte_offset: u32, syntax_kind: u32) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        file_path.hash(&mut hasher);
        byte_offset.hash(&mut hasher);
        syntax_kind.hash(&mut hasher);
        Self(hasher.finish())
    }
}

// ---------------------------------------------------------------------------
// Source positions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }
    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

// ---------------------------------------------------------------------------
// Comments
// ---------------------------------------------------------------------------

/// A comment extracted from the source text during scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    /// Byte offset of the start of the comment (the `/` of `//` or `/*`).
    pub pos: u32,
    /// Byte offset just past the end of the comment.
    pub end: u32,
    /// `true` for `/* ... */` comments, `false` for `// ...` comments.
    pub is_multiline: bool,
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCategory {
    Error,
    Warning,
    Suggestion,
    Message,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: u32,
    pub message: String,
    pub category: DiagnosticCategory,
    pub file_name: Option<String>,
    pub span: Option<Span>,
    /// Optional related diagnostic information (e.g. TS6203 "was also declared here").
    pub related: Option<Vec<RelatedDiagnostic>>,
}

/// A related diagnostic message attached to a primary diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedDiagnostic {
    pub code: u32,
    pub message: String,
    /// File name for the related location (may differ from the primary diagnostic).
    pub file_name: Option<String>,
    /// Span within the related file.
    pub span: Option<Span>,
}

// ---------------------------------------------------------------------------
// Modifier flags
// ---------------------------------------------------------------------------

pub type ModifierFlags = u32;
pub const MOD_NONE: u32 = 0;
pub const MOD_EXPORT: u32 = 1;
pub const MOD_DECLARE: u32 = 1 << 1;
pub const MOD_DEFAULT: u32 = 1 << 2;
pub const MOD_CONST: u32 = 1 << 3;
pub const MOD_ABSTRACT: u32 = 1 << 4;
pub const MOD_ASYNC: u32 = 1 << 5;
pub const MOD_STATIC: u32 = 1 << 6;
pub const MOD_READONLY: u32 = 1 << 7;
pub const MOD_PUBLIC: u32 = 1 << 8;
pub const MOD_PRIVATE: u32 = 1 << 9;
pub const MOD_PROTECTED: u32 = 1 << 10;
pub const MOD_OVERRIDE: u32 = 1 << 11;
pub const MOD_ACCESSOR: u32 = 1 << 12;
pub const MOD_IN: u32 = 1 << 13;
pub const MOD_OUT: u32 = 1 << 14;

// ---------------------------------------------------------------------------
// Variable declaration kind
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarKind {
    Var,
    Let,
    Const,
    Using,
    AwaitUsing,
}

// ---------------------------------------------------------------------------
// Operators
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Exp,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
    LogAnd,
    LogOr,
    NullCoal,
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    InstanceOf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    ExpAssign,
    BitAndAssign,
    BitOrAssign,
    BitXorAssign,
    ShlAssign,
    ShrAssign,
    UShrAssign,
    LogAndAssign,
    LogOrAssign,
    NullCoalAssign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Pos,
    Neg,
    BitNot,
    LogNot,
    Typeof,
    Void,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOp {
    PreInc,
    PreDec,
    PostInc,
    PostDec,
}

// ---------------------------------------------------------------------------
// Source file
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub file_name: String,
    pub text: String,
    pub statements: Vec<Stmt>,
    pub diagnostics: Vec<Diagnostic>,
    pub span: Span,
    /// All comments found in the source, sorted by position.
    pub comments: Vec<Comment>,
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Var(Box<VarStmt>),
    Expr(Box<Expr>),
    Return(Option<Box<Expr>>),
    If(IfStmt),
    While(WhileStmt),
    DoWhile(DoWhileStmt),
    For(Box<ForStmt>),
    ForIn(Box<ForInStmt>),
    ForOf(Box<ForOfStmt>),
    Switch(Box<SwitchStmt>),
    Try(Box<TryStmt>),
    Throw(Box<Expr>),
    Break(Option<String>),
    Continue(Option<String>),
    Block(Vec<Stmt>),
    Empty,
    FnDecl(Box<FnDecl>),
    ClassDecl(Box<ClassDecl>),
    InterfaceDecl(Box<InterfaceDecl>),
    TypeAlias(Box<TypeAliasDecl>),
    EnumDecl(Box<EnumDecl>),
    ModuleDecl(Box<ModuleDecl>),
    Import(Box<ImportDecl>),
    /// `import x = M.N;` → emitted as `var x = M.N;`
    /// The `ModifierFlags` holds `MOD_EXPORT` when `export import x = ...`.
    ImportEquals(Box<ImportEqualsDecl>),
    Export(Box<ExportDecl>),
    ExportAssign(Box<Expr>),
    Labeled(Box<LabeledStmt>),
    With(Box<WithStmt>),
    Debugger,
}

// ---------------------------------------------------------------------------
// Statement sub-types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct VarStmt {
    pub kind: VarKind,
    pub declarations: Vec<VarDeclarator>,
    pub modifiers: ModifierFlags,
}

#[derive(Debug, Clone)]
pub struct VarDeclarator {
    pub name: Pat,
    pub type_ann: Option<TypeNode>,
    pub init: Option<Box<Expr>>,
    /// Full-start offset matching TypeScript's `VariableDeclaration.pos`.
    /// This includes trivia after the declaration keyword or preceding comma.
    pub full_start: u32,
    /// Full-start offsets for the individual binding elements in a
    /// destructuring declaration. The associated spans remain the exact name
    /// ranges used by navigation and diagnostics.
    pub binding_name_full_starts: Vec<BindingNameFullStart>,
    pub span: Span,
    pub definite: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindingNameFullStart {
    pub name_span: Span,
    pub full_start: u32,
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub test: Box<Expr>,
    pub consequent: Box<Stmt>,
    pub alternate: Option<Box<Stmt>>,
}

#[derive(Debug, Clone)]
pub struct WhileStmt {
    pub test: Box<Expr>,
    pub body: Box<Stmt>,
}

#[derive(Debug, Clone)]
pub struct DoWhileStmt {
    pub body: Box<Stmt>,
    pub test: Box<Expr>,
}

#[derive(Debug, Clone)]
pub struct ForStmt {
    pub init: Option<ForInit>,
    pub test: Option<Box<Expr>>,
    pub update: Option<Box<Expr>>,
    pub body: Box<Stmt>,
}

#[derive(Debug, Clone)]
pub enum ForInit {
    Var(VarStmt),
    Expr(Box<Expr>),
}

#[derive(Debug, Clone)]
pub struct ForInStmt {
    pub left: ForInOfLeft,
    pub right: Box<Expr>,
    pub body: Box<Stmt>,
}

#[derive(Debug, Clone)]
pub struct ForOfStmt {
    pub is_await: bool,
    pub left: ForInOfLeft,
    pub right: Box<Expr>,
    pub body: Box<Stmt>,
}

#[derive(Debug, Clone)]
pub enum ForInOfLeft {
    Var(VarStmt),
    Pat(Pat),
    Expr(Box<Expr>),
}

#[derive(Debug, Clone)]
pub struct SwitchStmt {
    pub discriminant: Box<Expr>,
    pub cases: Vec<SwitchCase>,
}

#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub test: Option<Box<Expr>>,
    pub consequent: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TryStmt {
    pub block: Vec<Stmt>,
    pub handler: Option<CatchClause>,
    pub finalizer: Option<Vec<Stmt>>,
}

#[derive(Debug, Clone)]
pub struct CatchClause {
    pub param: Option<Pat>,
    pub param_type: Option<TypeNode>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct LabeledStmt {
    pub label: String,
    pub body: Box<Stmt>,
}

#[derive(Debug, Clone)]
pub struct WithStmt {
    pub object: Box<Expr>,
    pub body: Box<Stmt>,
}

#[derive(Debug, Clone)]
pub struct ImportEqualsDecl {
    pub name: String,
    pub module_ref: Box<Expr>,
    pub modifiers: ModifierFlags,
}

// ---------------------------------------------------------------------------
// Function / Class declarations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FnDecl {
    pub name: Option<String>,
    pub name_span: Option<Span>,
    pub type_params: Option<Vec<TypeParam>>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
    pub body: Option<Vec<Stmt>>,
    pub modifiers: ModifierFlags,
    pub is_generator: bool,
    pub is_async: bool,
    pub decorators: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: Pat,
    pub type_ann: Option<TypeNode>,
    pub initializer: Option<Box<Expr>>,
    pub dotdotdot: bool,
    pub optional: bool,
    pub modifiers: ModifierFlags,
    pub decorators: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ClassDecl {
    pub name: Option<String>,
    pub name_span: Option<Span>,
    pub type_params: Option<Vec<TypeParam>>,
    pub extends: Option<Box<Expr>>,
    pub extends_type_args: Option<Vec<TypeNode>>,
    pub implements: Vec<TypeNode>,
    pub members: Vec<ClassMember>,
    pub modifiers: ModifierFlags,
    pub decorators: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ClassMember {
    pub kind: ClassMemberKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ClassMemberKind {
    Property(ClassProp),
    Method(ClassMethod),
    Constructor(ClassConstructor),
    GetAccessor(ClassAccessor),
    SetAccessor(ClassAccessor),
    IndexSignature(IndexSignature),
    StaticBlock(Vec<Stmt>),
    SemicolonClassElement,
}

#[derive(Debug, Clone)]
pub struct ClassProp {
    pub name: PropName,
    pub type_ann: Option<TypeNode>,
    pub initializer: Option<Box<Expr>>,
    pub modifiers: ModifierFlags,
    pub optional: bool,
    pub definite: bool,
    pub decorators: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct ClassMethod {
    pub name: PropName,
    pub type_params: Option<Vec<TypeParam>>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
    pub body: Option<Vec<Stmt>>,
    pub modifiers: ModifierFlags,
    pub is_generator: bool,
    pub is_async: bool,
    pub optional: bool,
    pub decorators: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct ClassConstructor {
    /// The constructor keyword, excluding modifiers and leading trivia.
    pub keyword_span: Span,
    pub params: Vec<Param>,
    pub body: Option<Vec<Stmt>>,
    pub modifiers: ModifierFlags,
    pub decorators: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct ClassAccessor {
    pub name: PropName,
    pub type_params: Option<Vec<TypeParam>>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
    pub body: Option<Vec<Stmt>>,
    pub modifiers: ModifierFlags,
    pub decorators: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct IndexSignature {
    pub params: Vec<Param>,
    pub type_ann: Option<TypeNode>,
    pub modifiers: ModifierFlags,
}

#[derive(Debug, Clone)]
pub enum PropName {
    Ident(AstString, Span),
    String(AstString, Span),
    Number(AstString, Span),
    Computed(Box<Expr>, Span),
    Private(AstString, Span),
}

impl PropName {
    pub fn span(&self) -> Span {
        match self {
            Self::Ident(_, s)
            | Self::String(_, s)
            | Self::Number(_, s)
            | Self::Computed(_, s)
            | Self::Private(_, s) => *s,
        }
    }

    pub fn ident_name(&self) -> Option<&str> {
        match self {
            Self::Ident(name, _) => Some(name),
            Self::Private(name, _) => Some(name),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Interface / Type alias / Enum / Module
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct InterfaceDecl {
    pub name: String,
    pub name_span: Option<Span>,
    pub type_params: Option<Vec<TypeParam>>,
    pub extends: Vec<TypeNode>,
    pub members: Vec<TypeMember>,
    pub modifiers: ModifierFlags,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypeMember {
    pub kind: TypeMemberKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeMemberKind {
    PropertySig(PropertySig),
    MethodSig(MethodSig),
    CallSig(CallSig),
    ConstructSig(ConstructSig),
    IndexSig(IndexSignature),
    GetAccessorSig(AccessorSig),
    SetAccessorSig(AccessorSig),
}

#[derive(Debug, Clone)]
pub struct PropertySig {
    pub name: PropName,
    pub type_ann: Option<TypeNode>,
    pub optional: bool,
    pub readonly: bool,
}

#[derive(Debug, Clone)]
pub struct MethodSig {
    pub name: PropName,
    pub type_params: Option<Vec<TypeParam>>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub struct CallSig {
    pub type_params: Option<Vec<TypeParam>>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
}

#[derive(Debug, Clone)]
pub struct ConstructSig {
    pub type_params: Option<Vec<TypeParam>>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
}

#[derive(Debug, Clone)]
pub struct AccessorSig {
    pub name: PropName,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
}

#[derive(Debug, Clone)]
pub struct TypeAliasDecl {
    pub name: String,
    pub name_span: Option<Span>,
    pub type_params: Option<Vec<TypeParam>>,
    pub type_ann: TypeNode,
    pub modifiers: ModifierFlags,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub name_span: Option<Span>,
    pub members: Vec<EnumMember>,
    pub modifiers: ModifierFlags,
    pub is_const: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumMember {
    pub name: PropName,
    pub initializer: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ModuleDecl {
    pub name: ModuleName,
    pub name_span: Option<Span>,
    pub body: Option<ModuleBody>,
    pub modifiers: ModifierFlags,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ModuleName {
    Ident(String),
    String(String),
}

#[derive(Debug, Clone)]
pub enum ModuleBody {
    Block(Vec<Stmt>),
    Module(Box<ModuleDecl>),
}

// ---------------------------------------------------------------------------
// Import / Export
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub specifiers: ImportClause,
    pub source: String,
    pub type_only: bool,
    /// `import defer ...` — deferred import (TC39 proposal).
    pub defer: bool,
    /// `import "mod"` (no import clause) vs `import {} from "mod"` (empty clause).
    pub is_side_effect: bool,
    pub span: Span,
    /// Span of the module specifier string literal (including quotes).
    pub source_span: Span,
    /// Whether the import attributes contain a `resolution-mode` entry.
    pub has_resolution_mode: bool,
}

#[derive(Debug, Clone)]
pub enum ImportClause {
    Named {
        default: Option<String>,
        named: Vec<ImportSpecifier>,
        namespace: Option<String>,
    },
    Require(String),
}

#[derive(Debug, Clone)]
pub struct ImportSpecifier {
    pub local: String,
    pub imported: Option<String>,
    pub is_type: bool,
    pub imported_is_string: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ExportDecl {
    pub kind: ExportDeclKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExportDeclKind {
    Decl(Box<Stmt>),
    Named {
        specifiers: Vec<ExportSpecifier>,
        source: Option<String>,
        type_only: bool,
    },
    Default(Box<Expr>),
    DefaultDecl(Box<Stmt>),
    All {
        source: String,
        alias: Option<String>,
        alias_is_string: bool,
        type_only: bool,
    },
}

#[derive(Debug, Clone)]
pub struct ExportSpecifier {
    pub local: String,
    pub exported: Option<String>,
    pub is_type: bool,
    pub local_is_string: bool,
    pub exported_is_string: bool,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Ident(AstString),
    NumLit(AstString),
    BigIntLit(AstString),
    StrLit(AstString),
    BoolLit(bool),
    NullLit,
    RegexpLit(Box<RegexpLitExpr>),
    NoSubstTemplate(AstString),
    Template(Box<TemplateLit>),
    TaggedTemplate(Box<TaggedTemplateLit>),
    ArrayLit(Vec<Option<Box<Expr>>>),
    ObjectLit(Vec<ObjLitProp>),
    FnExpr(Box<FnDecl>),
    Arrow(Box<ArrowFn>),
    ClassExpr(Box<ClassDecl>),
    Call(Box<CallExpr>),
    New(Box<NewExpr>),
    Member(Box<MemberExpr>),
    ElemAccess(ElemAccessExpr),
    Cond(CondExpr),
    Binary(BinaryExpr),
    Unary(UnaryExpr),
    Update(UpdateExpr),
    Paren(Box<Expr>),
    TypeAssertion(Box<TypeAssertionExpr>),
    As(Box<AsExpr>),
    Satisfies(Box<SatisfiesExpr>),
    NonNull(Box<Expr>),
    /// Instantiation expression: `expr<TypeArgs>` (not a call).
    Instantiation(Box<InstantiationExpr>),
    Spread(Box<Expr>),
    Yield(bool, Option<Box<Expr>>),
    Await(Box<Expr>),
    Delete(Box<Expr>),
    Typeof(Box<Expr>),
    Void(Box<Expr>),
    This,
    Super,
    Assign(AssignExpr),
    MetaProp(Box<MetaPropExpr>),
    Comma(Vec<Box<Expr>>),
    Omitted,
    JsxElement(Box<JsxElement>),
    JsxSelfClosing(Box<JsxSelfClosingElement>),
    JsxFragment(Box<JsxFragment>),
}

impl ExprKind {
    /// If this is a type-erasing wrapper (As, Satisfies, TypeAssertion, NonNull, Instantiation),
    /// return a reference to the inner expression. Otherwise return None.
    pub fn type_layer_inner(&self) -> Option<&Expr> {
        match self {
            ExprKind::As(a) => Some(&a.expr),
            ExprKind::Satisfies(s) => Some(&s.expr),
            ExprKind::TypeAssertion(ta) => Some(&ta.expr),
            ExprKind::NonNull(e) => Some(e),
            ExprKind::Instantiation(inst) => Some(&inst.expr),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// JSX types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct JsxElement {
    pub name: Box<Expr>,
    pub opening_span: Span,
    pub closing: Option<JsxClosingElement>,
    pub type_args: Option<Vec<TypeNode>>,
    pub attributes: Vec<JsxAttribute>,
    pub children: Vec<JsxChild>,
    /// Recovery omitted this element's closing tag, at EOF or before its parent's close.
    pub closing_tag_missing: bool,
}

#[derive(Debug, Clone)]
pub struct JsxClosingElement {
    pub name: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct JsxSelfClosingElement {
    pub name: Box<Expr>,
    pub type_args: Option<Vec<TypeNode>>,
    pub attributes: Vec<JsxAttribute>,
}

#[derive(Debug, Clone)]
pub struct JsxFragment {
    pub children: Vec<JsxChild>,
}

#[derive(Debug, Clone)]
pub enum JsxAttribute {
    Normal {
        name: String,
        value: Option<Box<Expr>>,
        span: Span,
    },
    Spread(Box<Expr>, Span),
}

#[derive(Debug, Clone)]
pub enum JsxChild {
    Text(String, Span),
    Element(Box<Expr>),
    Expression(Option<Box<Expr>>, Span),
    Fragment(Box<JsxFragment>),
}

#[derive(Debug, Clone)]
pub struct TemplateLit {
    pub quasis: Vec<TemplateElement>,
    pub exprs: Vec<Box<Expr>>,
}

#[derive(Debug, Clone)]
pub struct TemplateElement {
    pub raw: String,
    pub cooked: Option<String>,
    pub tail: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TaggedTemplateLit {
    pub tag: Box<Expr>,
    pub quasi: TemplateLit,
    pub type_args: Option<Vec<TypeNode>>,
}

#[derive(Debug, Clone)]
pub enum ObjLitProp {
    Property(ObjProp),
    Shorthand(AstString, Span),
    /// Shorthand property with default initializer: `{ x = 1 }` in destructuring.
    ShorthandDefault(AstString, Box<Expr>, Span),
    Spread(Box<Expr>, Span),
    Method(ObjMethod),
    Get(ObjAccessor),
    Set(ObjAccessor),
}

#[derive(Debug, Clone)]
pub struct ObjProp {
    pub key: PropName,
    pub value: Box<Expr>,
    pub computed: bool,
    /// Parser stripped a `?` token from `{ x?: value }` syntax.
    pub question_token: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ObjMethod {
    pub name: PropName,
    pub type_params: Option<Vec<TypeParam>>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
    pub body: Vec<Stmt>,
    pub is_generator: bool,
    pub is_async: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ObjAccessor {
    pub name: PropName,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ArrowFn {
    pub type_params: Option<Vec<TypeParam>>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
    pub body: ArrowBody,
    pub is_async: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ArrowBody {
    Expr(Box<Expr>),
    Block(Vec<Stmt>),
}

#[derive(Debug, Clone)]
pub struct CallExpr {
    pub callee: Box<Expr>,
    pub type_args: Option<Vec<TypeNode>>,
    pub args: Vec<Box<Expr>>,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub struct NewExpr {
    pub callee: Box<Expr>,
    pub type_args: Option<Vec<TypeNode>>,
    pub args: Option<Vec<Box<Expr>>>,
}

#[derive(Debug, Clone)]
pub struct MemberExpr {
    pub object: Box<Expr>,
    pub property: AstString,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub struct ElemAccessExpr {
    pub object: Box<Expr>,
    pub index: Box<Expr>,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub struct CondExpr {
    pub test: Box<Expr>,
    pub consequent: Box<Expr>,
    pub alternate: Box<Expr>,
}

#[derive(Debug, Clone)]
pub struct BinaryExpr {
    pub left: Box<Expr>,
    pub op: BinaryOp,
    pub right: Box<Expr>,
}

#[derive(Debug, Clone)]
pub struct UnaryExpr {
    pub op: UnaryOp,
    pub argument: Box<Expr>,
}

#[derive(Debug, Clone)]
pub struct UpdateExpr {
    pub op: UpdateOp,
    pub argument: Box<Expr>,
}

#[derive(Debug, Clone)]
pub struct AssignExpr {
    pub left: Box<Expr>,
    pub op: AssignOp,
    pub right: Box<Expr>,
}

#[derive(Debug, Clone)]
pub struct RegexpLitExpr {
    pub pattern: AstString,
    pub flags: AstString,
}

#[derive(Debug, Clone)]
pub struct TypeAssertionExpr {
    pub type_node: TypeNode,
    pub expr: Box<Expr>,
}

#[derive(Debug, Clone)]
pub struct AsExpr {
    pub expr: Box<Expr>,
    pub type_node: TypeNode,
}

#[derive(Debug, Clone)]
pub struct SatisfiesExpr {
    pub expr: Box<Expr>,
    pub type_node: TypeNode,
}

#[derive(Debug, Clone)]
pub struct InstantiationExpr {
    pub expr: Box<Expr>,
    pub type_args: Vec<TypeNode>,
}

#[derive(Debug, Clone)]
pub struct MetaPropExpr {
    pub meta: AstString,
    pub property: AstString,
}

// ---------------------------------------------------------------------------
// Patterns (destructuring)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Pat {
    pub kind: PatKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PatKind {
    Ident(AstString),
    Array(Vec<Option<ArrayPatElem>>),
    Object(Vec<ObjPatProp>),
    Assign(Box<Pat>, Box<Expr>),
    Rest(Box<Pat>),
}

#[derive(Debug, Clone)]
pub enum ArrayPatElem {
    Pat(Pat),
    Rest(Pat),
}

#[derive(Debug, Clone)]
pub enum ObjPatProp {
    KeyValue(PropName, Pat),
    Shorthand(AstString, Span),
    Rest(Pat),
    ShorthandAssign(AstString, Box<Expr>, Span),
}

// ---------------------------------------------------------------------------
// Type annotations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TypeNode {
    pub kind: TypeNodeKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeNodeKind {
    Keyword(KeywordTypeKind),
    Reference(Box<TypeRef>),
    Array(Box<TypeNode>),
    Tuple(Vec<TupleElement>),
    Union(Vec<TypeNode>),
    Intersection(Vec<TypeNode>),
    Function(Box<FnTypeNode>),
    Constructor(Box<FnTypeNode>),
    TypeLit(Vec<TypeMember>),
    Conditional(Box<ConditionalTypeNode>),
    Mapped(Box<MappedTypeNode>),
    IndexedAccess(Box<TypeNode>, Box<TypeNode>),
    TypeQuery(Box<Expr>),
    Keyof(Box<TypeNode>),
    Unique(Box<TypeNode>),
    Readonly(Box<TypeNode>),
    Infer(String, Option<Box<TypeNode>>),
    TemplateLit(Box<TemplateLitTypeNode>),
    This,
    Paren(Box<TypeNode>),
    Rest(Box<TypeNode>),
    Optional(Box<TypeNode>),
    JSDocNullable(Option<Box<TypeNode>>),
    Literal(LiteralTypeKind),
    ImportType(Box<ImportTypeNode>),
    Predicate(Box<PredicateTypeNode>),
    TypeOperator(TypeOperatorKind, Box<TypeNode>),
    NamedTupleMember(Box<NamedTupleMember>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordTypeKind {
    Any,
    Unknown,
    Number,
    BigInt,
    String,
    Boolean,
    Void,
    Undefined,
    Null,
    Never,
    Object,
    Symbol,
    Intrinsic,
}

#[derive(Debug, Clone)]
pub struct TypeRef {
    pub name: Box<Expr>,
    pub type_args: Option<Vec<TypeNode>>,
}

#[derive(Debug, Clone)]
pub struct TupleElement {
    pub label: Option<String>,
    pub type_node: TypeNode,
    pub optional: bool,
    pub dotdotdot: bool,
}

#[derive(Debug, Clone)]
pub struct FnTypeNode {
    /// Only meaningful for `TypeNodeKind::Constructor`.
    pub is_abstract: bool,
    pub type_params: Option<Vec<TypeParam>>,
    pub params: Vec<Param>,
    pub return_type: Box<TypeNode>,
}

#[derive(Debug, Clone)]
pub struct ConditionalTypeNode {
    pub check: Box<TypeNode>,
    pub extends: Box<TypeNode>,
    pub true_type: Box<TypeNode>,
    pub false_type: Box<TypeNode>,
}

#[derive(Debug, Clone)]
pub struct MappedTypeNode {
    pub type_param: TypeParam,
    pub name_type: Option<Box<TypeNode>>,
    pub type_ann: Option<Box<TypeNode>>,
    pub readonly: Option<MappedModifier>,
    pub optional: Option<MappedModifier>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedModifier {
    Add,
    Remove,
    None,
}

#[derive(Debug, Clone)]
pub struct TemplateLitTypeNode {
    pub quasis: Vec<TemplateElement>,
    pub types: Vec<TypeNode>,
}

#[derive(Debug, Clone)]
pub enum LiteralTypeKind {
    Number(String),
    String(String),
    Boolean(bool),
    Null,
    BigInt(String),
    Minus(String),
}

#[derive(Debug, Clone)]
pub struct ImportTypeNode {
    pub argument: Box<TypeNode>,
    /// Span of the module specifier string literal (including quotes).
    pub argument_span: Span,
    pub qualifier: Option<Box<Expr>>,
    pub type_args: Option<Vec<TypeNode>>,
    pub is_typeof: bool,
    /// Whether the optional import-type attributes contain `resolution-mode`.
    pub has_resolution_mode: bool,
}

#[derive(Debug, Clone)]
pub struct PredicateTypeNode {
    pub param_name: String,
    pub type_ann: Option<Box<TypeNode>>,
    pub asserts: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeOperatorKind {
    Keyof,
    Unique,
    Readonly,
}

#[derive(Debug, Clone)]
pub struct NamedTupleMember {
    pub name: String,
    pub type_node: Box<TypeNode>,
    pub optional: bool,
    pub dotdotdot: bool,
}

// ---------------------------------------------------------------------------
// Type parameters
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TypeParam {
    pub name: String,
    /// Span of the name identifier alone (`span` also covers modifiers,
    /// constraint and default).
    pub name_span: Span,
    pub constraint: Option<Box<TypeNode>>,
    pub default: Option<Box<TypeNode>>,
    pub modifiers: ModifierFlags,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Compiler options (parsed from // @ directives)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct CompilerOptions {
    pub target: Option<ScriptTarget>,
    pub module: Option<ModuleKind>,
    pub strict: Option<bool>,
    pub no_implicit_any: Option<bool>,
    pub no_implicit_returns: Option<bool>,
    pub no_unused_locals: Option<bool>,
    pub no_unused_parameters: Option<bool>,
    pub strict_null_checks: Option<bool>,
    pub strict_function_types: Option<bool>,
    pub strict_bind_call_apply: Option<bool>,
    pub strict_property_initialization: Option<bool>,
    pub no_emit: Option<bool>,
    pub declaration: Option<bool>,
    pub source_map: Option<bool>,
    pub inline_source_map: Option<bool>,
    pub jsx: Option<JsxEmit>,
    pub jsx_factory: Option<String>,
    pub jsx_fragment_factory: Option<String>,
    pub jsx_import_source: Option<String>,
    pub lib: Vec<String>,
    /// `compilerOptions.types`. `None` = field unspecified → auto-discover
    /// every `@types/*` package found in `typeRoots`. `Some(vec![])` = user
    /// wrote `"types": []` → load nothing. `Some([..])` = load just those.
    /// This 3-state matters: `[]` is the canonical TypeScript escape hatch
    /// for projects that share a monorepo with stray @types they don't want.
    pub types: Option<Vec<String>>,
    /// `compilerOptions.typeRoots`. `None` → default `["./node_modules/@types"]`
    /// (resolved relative to the tsconfig dir, with ancestor walk-up).
    /// `Some([..])` → use only those roots.
    pub type_roots: Option<Vec<String>>,
    pub out_file: Option<String>,
    pub out_dir: Option<String>,
    pub root_dir: Option<String>,
    pub allow_js: Option<bool>,
    pub check_js: Option<bool>,
    pub es_module_interop: Option<bool>,
    pub allow_synthetic_default_imports: Option<bool>,
    pub allow_importing_ts_extensions: Option<bool>,
    pub skip_lib_check: Option<bool>,
    pub skip_default_lib_check: Option<bool>,
    pub no_lib: Option<bool>,
    pub exact_optional_property_types: Option<bool>,
    pub no_emit_on_error: Option<bool>,
    pub isolated_modules: Option<bool>,
    pub preserve_const_enums: Option<bool>,
    pub module_resolution: Option<String>,
    pub base_url: Option<String>,
    pub paths: Option<String>,
    pub filename: Option<String>,
    pub no_error_truncation: Option<bool>,
    pub experimental_decorators: Option<bool>,
    pub emit_decorator_metadata: Option<bool>,
    pub use_define_for_class_fields: Option<bool>,
    pub module_detection: Option<String>,
    pub verbatim_module_syntax: Option<bool>,
    pub no_check: Option<bool>,
    pub no_resolve: Option<bool>,
    pub down_level_iteration: Option<bool>,
    pub import_helpers: Option<bool>,
    pub emit_bom: Option<bool>,
    pub new_line: Option<String>,
    pub remove_comments: Option<bool>,
    pub no_emit_helpers: Option<bool>,
    pub always_strict: Option<bool>,
    pub pretty: Option<bool>,
    pub no_fallthrough_cases_in_switch: Option<bool>,
    pub allow_unreachable_code: Option<bool>,
    pub force_consistent_casing_in_file_names: Option<bool>,
    pub use_unknown_in_catch_variables: Option<bool>,
    pub no_property_access_from_index_signature: Option<bool>,
    pub no_unchecked_indexed_access: Option<bool>,
    pub resolve_json_module: Option<bool>,
    pub isolated_declarations: Option<bool>,
    pub emit_declaration_only: Option<bool>,
    pub imports_not_used_as_values: Option<ImportsNotUsedAsValues>,
    pub incremental: Option<bool>,
    pub composite: Option<bool>,
    pub ts_build_info_file: Option<String>,
    pub other: Vec<(String, String)>,
    /// PRESERVE MODE: Keep type annotations in the output.
    /// When true, type assertions (`as Type`), angle-bracket assertions (`<Type>x`),
    /// and type casts are preserved instead of being stripped.
    pub preserve_type_annotations: Option<bool>,
    /// PRESERVE MODE: Keep all comments in the output.
    /// When true, comments are not stripped regardless of `remove_comments` option.
    pub preserve_comments: Option<bool>,
    /// PRESERVE MODE: Try to maintain original source formatting.
    /// When true, attempts to preserve whitespace and indentation from source.
    pub preserve_whitespace: Option<bool>,
    /// FAST TRANSPILE MODE: skip the cosmetic source-text normalization that
    /// reproduces tsc's exact formatting (brace spacing, K&R/Allman conversion,
    /// whitespace/indent normalization, smeared-statement splitting, …) and copy
    /// no-transform statements verbatim. Output is semantically identical valid
    /// JS, just formatted as the source was. Intended for consumers that don't
    /// care about tsc-faithful formatting (e.g. the bext/PRISM transpile path,
    /// which feeds the JS straight to V8). Only meaningful with transpile-only.
    pub fast_emit: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportsNotUsedAsValues {
    Remove,
    Preserve,
    Error,
}

impl ImportsNotUsedAsValues {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "remove" => Some(Self::Remove),
            "preserve" => Some(Self::Preserve),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScriptTarget {
    ES3,
    ES5,
    ES2015,
    ES2016,
    ES2017,
    ES2018,
    ES2019,
    ES2020,
    ES2021,
    ES2022,
    ES2023,
    ES2024,
    ES2025,
    ESNext,
}

impl ScriptTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ES3 => "es3",
            Self::ES5 => "es5",
            Self::ES2015 => "es2015",
            Self::ES2016 => "es2016",
            Self::ES2017 => "es2017",
            Self::ES2018 => "es2018",
            Self::ES2019 => "es2019",
            Self::ES2020 => "es2020",
            Self::ES2021 => "es2021",
            Self::ES2022 => "es2022",
            Self::ES2023 => "es2023",
            Self::ES2024 => "es2024",
            Self::ES2025 => "es2025",
            Self::ESNext => "esnext",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "es3" => Some(Self::ES3),
            "es5" => Some(Self::ES5),
            "es6" | "es2015" => Some(Self::ES2015),
            "es2016" => Some(Self::ES2016),
            "es2017" => Some(Self::ES2017),
            "es2018" => Some(Self::ES2018),
            "es2019" => Some(Self::ES2019),
            "es2020" => Some(Self::ES2020),
            "es2021" => Some(Self::ES2021),
            "es2022" => Some(Self::ES2022),
            "es2023" => Some(Self::ES2023),
            "es2024" => Some(Self::ES2024),
            "es2025" => Some(Self::ES2025),
            "esnext" => Some(Self::ESNext),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKind {
    None,
    CommonJS,
    AMD,
    UMD,
    System,
    ES2015,
    ES2020,
    ES2022,
    ESNext,
    Node16,
    Node18,
    Node20,
    NodeNext,
    Preserve,
}

impl ModuleKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::CommonJS => "commonjs",
            Self::AMD => "amd",
            Self::UMD => "umd",
            Self::System => "system",
            Self::ES2015 => "es2015",
            Self::ES2020 => "es2020",
            Self::ES2022 => "es2022",
            Self::ESNext => "esnext",
            Self::Node16 => "node16",
            Self::Node18 => "node18",
            Self::Node20 => "node20",
            Self::NodeNext => "nodenext",
            Self::Preserve => "preserve",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "none" => Some(Self::None),
            "commonjs" => Some(Self::CommonJS),
            "amd" => Some(Self::AMD),
            "umd" => Some(Self::UMD),
            "system" => Some(Self::System),
            "es6" | "es2015" => Some(Self::ES2015),
            "es2020" => Some(Self::ES2020),
            "es2022" => Some(Self::ES2022),
            "esnext" => Some(Self::ESNext),
            "node16" => Some(Self::Node16),
            "node18" => Some(Self::Node18),
            "node20" => Some(Self::Node20),
            "nodenext" => Some(Self::NodeNext),
            "preserve" => Some(Self::Preserve),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsxEmit {
    None,
    Preserve,
    React,
    ReactNative,
    ReactJSX,
    ReactJSXDev,
}

impl JsxEmit {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "none" => Some(Self::None),
            "preserve" => Some(Self::Preserve),
            "react" => Some(Self::React),
            "react-native" | "reactnative" => Some(Self::ReactNative),
            "react-jsx" | "reactjsx" => Some(Self::ReactJSX),
            "react-jsxdev" | "reactjsxdev" => Some(Self::ReactJSXDev),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Test case structure
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TestCase {
    pub file_name: String,
    pub options: CompilerOptions,
    pub files: Vec<TestFile>,
}

#[derive(Debug, Clone)]
pub struct TestFile {
    pub name: String,
    pub content: String,
}

/// Parse a test case file, extracting compiler options from `// @` directives
/// and splitting multi-file tests by `// @filename:` directives.
pub fn parse_test_case(file_name: &str, source: &str) -> TestCase {
    // Strip BOM (U+FEFF) if present – it prevents directive detection.
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);

    // Some multi-file tests include descriptive comments/directives before the
    // first `@filename:` directive. Those lines are test metadata, not part of
    // a synthetic default source file.
    let has_any_filename_directive = source.lines().any(|line| {
        let trimmed = line.trim();
        if let Some(rest) = strip_directive_prefix(trimmed) {
            if let Some((key, _)) = rest.split_once(':') {
                return key.trim().eq_ignore_ascii_case("filename");
            }
        }
        false
    });

    let mut options = CompilerOptions::default();
    let mut files = Vec::new();
    let mut current_file_name = file_name
        .rsplit('/')
        .next()
        .unwrap_or(file_name)
        .to_string();
    let mut current_content = String::new();
    let mut has_filename_directive = false;
    let mut has_meaningful_content = false;
    let mut has_code_content = false;
    let mut current_is_explicit_file = false;

    for line in source.lines() {
        let trimmed = line.trim();
        // Match `// @key: value` directives with flexible whitespace:
        //   "// @key: value", "//@key: value", "//  @key: value"
        // Lines like `// @ts-check` (no colon) are NOT directives — treat as content.
        if let Some(rest) = strip_directive_prefix(trimmed) {
            if let Some((key, value)) = rest.split_once(':') {
                let key = key.trim().to_lowercase();
                let value = value.trim();
                // `@link` and `@symlink` are test metadata directives
                // (symlink configuration), not source code — always skip.
                if key == "link" || key == "symlink" {
                    continue;
                }
                if key == "filename" {
                    // Deduplicate consecutive identical @filename directives
                    // (e.g., `// @filename: file3.ts` appearing twice).
                    let new_name = value.to_string();
                    if new_name == current_file_name && !has_meaningful_content {
                        // Still mark that we saw a @filename directive so
                        // subsequent comments aren't treated as preamble.
                        has_filename_directive = true;
                        current_is_explicit_file = true;
                        continue;
                    }
                    // Push explicit @filename sections even when empty.
                    if has_meaningful_content || current_is_explicit_file {
                        files.push(TestFile {
                            name: current_file_name.clone(),
                            content: current_content.clone(),
                        });
                    }
                    current_file_name = new_name;
                    current_content.clear();
                    has_filename_directive = true;
                    has_meaningful_content = false;
                    has_code_content = false;
                    current_is_explicit_file = true;
                    continue;
                }
                // `@ts-` prefixed comments (`@ts-nocheck:`, `@ts-ignore:`,
                // `@ts-expect-error:`) are source-level pragmas, not test
                // options — always keep them as content.
                let is_ts_pragma = key.starts_with("ts-");
                // Option directives before real source content are always
                // parsed and stripped.  After content starts, only strip
                // known compiler options (e.g. `@strict`, `@removeComments`)
                // — keep unknown `// @...:` comments verbatim.
                if !is_ts_pragma && (!has_code_content || is_known_option(&key)) {
                    parse_option(&mut options, &key, value);
                    continue;
                }
            }
        }
        {
            // Track whether we've seen a non-empty source line for this file.
            // Whitespace-only lines are considered meaningful content.
            if !line.is_empty() {
                let trimmed = line.trim();
                let is_comment_only = trimmed.starts_with("//")
                    || trimmed.starts_with("/*")
                    || trimmed.starts_with('*')
                    || trimmed.starts_with("*/");
                let ignore_preamble_line = has_any_filename_directive
                    && !has_filename_directive
                    && (trimmed.is_empty() || is_comment_only);
                if !ignore_preamble_line {
                    has_meaningful_content = true;
                    if !is_comment_only {
                        has_code_content = true;
                    }
                }
            }
            // Only add lines if we're inside a file with content (or if no
            // @filename directive has been seen yet, allowing content before
            // the first @filename to accumulate for the default file).
            if has_meaningful_content || has_filename_directive {
                if !current_content.is_empty() {
                    current_content.push('\n');
                }
                current_content.push_str(line);
            }
        }
    }

    // Rust's `str::lines()` does not yield a trailing empty string when the
    // text ends with `\n`.  If the source ends with a newline, the last file's
    // content therefore loses the trailing `\n` that intermediate files get
    // from the blank line before the next `@filename:` directive.  Restore it
    // so the harness blank-line separator logic sees consistent behaviour.
    if source.ends_with('\n') && !current_content.is_empty() {
        current_content.push('\n');
    }

    // Push the last file.
    if has_meaningful_content || current_is_explicit_file {
        files.push(TestFile {
            name: current_file_name,
            content: current_content,
        });
    }

    if files.is_empty() {
        files.push(TestFile {
            name: file_name
                .rsplit('/')
                .next()
                .unwrap_or(file_name)
                .to_string(),
            content: String::new(),
        });
    }

    TestCase {
        file_name: file_name.to_string(),
        options,
        files,
    }
}

/// Strip the `// @` directive prefix from a line, handling variations
/// like `// @`, `//@`, and `//  @`.
fn strip_directive_prefix(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix("//")?;
    let rest = rest.trim_start();
    rest.strip_prefix('@')
}

fn parse_option(opts: &mut CompilerOptions, key: &str, value: &str) {
    // Some test directives use comma-separated multi-values like
    // `// @target: ES5, ES2015`. For target, prefer the highest non-ES5.
    // For boolean options, prefer true when multiple values are given.
    match key {
        "target" => {
            if value.contains(',') {
                // Multi-value target: pick the highest non-ES5 target.
                opts.target = value
                    .split(',')
                    .filter_map(|v| ScriptTarget::parse(v.trim()))
                    .find(|t| !matches!(t, ScriptTarget::ES3 | ScriptTarget::ES5))
                    .or_else(|| ScriptTarget::parse(value.split(',').next().unwrap_or("").trim()));
            } else {
                opts.target = ScriptTarget::parse(value);
            }
        }
        "module" => opts.module = ModuleKind::parse(value),
        "strict" => opts.strict = parse_bool(value),
        "noimplicitany" => opts.no_implicit_any = parse_bool(value),
        "noimplicitreturns" => opts.no_implicit_returns = parse_bool(value),
        "nounusedlocals" => opts.no_unused_locals = parse_bool(value),
        "nounusedparameters" => opts.no_unused_parameters = parse_bool(value),
        "strictnullchecks" => opts.strict_null_checks = parse_bool(value),
        "strictfunctiontypes" => opts.strict_function_types = parse_bool(value),
        "strictbindcallapply" => opts.strict_bind_call_apply = parse_bool(value),
        "noemit" => opts.no_emit = parse_bool(value),
        "declaration" => opts.declaration = parse_bool(value),
        "sourcemap" => opts.source_map = parse_bool(value),
        "jsx" => opts.jsx = JsxEmit::parse(value),
        "jsxfactory" => {
            // TypeScript validates jsxFactory as an identifier or dotted name.
            // Invalid values (containing spaces, operators, etc.) are ignored
            // and fall back to the default (React.createElement).
            let valid = !value.is_empty()
                && value.split('.').all(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        Some(c) if c.is_alphabetic() || c == '_' || c == '$' => {}
                        _ => return false,
                    }
                    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                });
            if valid {
                opts.jsx_factory = Some(value.to_string());
            }
            // Keep the raw value for option validation (TS5067).
            opts.other.push((key.to_string(), value.to_string()));
        }
        "jsximportsource" => opts.jsx_import_source = Some(value.to_string()),
        "jsxfragmentfactory" => {
            let valid = !value.is_empty()
                && value.split('.').all(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        Some(c) if c.is_alphabetic() || c == '_' || c == '$' => {}
                        _ => return false,
                    }
                    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                });
            if valid {
                opts.jsx_fragment_factory = Some(value.to_string());
            }
        }
        "lib" => opts.lib = value.split(',').map(|s| s.trim().to_string()).collect(),
        "types" => opts.types = Some(value.split(',').map(|s| s.trim().to_string()).collect()),
        "outfile" => opts.out_file = Some(value.to_string()),
        "outdir" => opts.out_dir = Some(value.to_string()),
        "rootdir" => opts.root_dir = Some(value.to_string()),
        "allowjs" => opts.allow_js = parse_bool(value),
        "checkjs" => opts.check_js = parse_bool(value),
        "esmoduleinterop" => opts.es_module_interop = parse_bool(value),
        "allowsyntheticdefaultimports" => opts.allow_synthetic_default_imports = parse_bool(value),
        "allowimportingtsextensions" => opts.allow_importing_ts_extensions = parse_bool(value),
        "skiplibcheck" => opts.skip_lib_check = parse_bool(value),
        "skipdefaultlibcheck" => opts.skip_default_lib_check = parse_bool(value),
        "nolib" => opts.no_lib = parse_bool(value),
        "noerrortruncation" => opts.no_error_truncation = parse_bool(value),
        "alwaysstrict" => opts.always_strict = parse_bool(value),
        "experimentaldecorators" => opts.experimental_decorators = parse_bool(value),
        "emitdecoratormetadata" => opts.emit_decorator_metadata = parse_bool(value),
        "usedefineforclassfields" => opts.use_define_for_class_fields = parse_bool(value),
        "moduledetection" => opts.module_detection = Some(value.to_string()),
        "verbatimmodulesyntax" => opts.verbatim_module_syntax = parse_bool(value),
        "nocheck" => opts.no_check = parse_bool(value),
        "noresolve" => opts.no_resolve = parse_bool(value),
        "downleveliteration" => opts.down_level_iteration = parse_bool(value),
        "importhelpers" => opts.import_helpers = parse_bool(value),
        "emitbom" => opts.emit_bom = parse_bool(value),
        "newline" => opts.new_line = Some(value.to_string()),
        "removecomments" => opts.remove_comments = parse_bool(value),
        "noemithelpers" => opts.no_emit_helpers = parse_bool(value),
        "pretty" => opts.pretty = parse_bool(value),
        "nofallthroughcasesinswitch" => opts.no_fallthrough_cases_in_switch = parse_bool(value),
        "allowunreachablecode" => opts.allow_unreachable_code = parse_bool(value),
        "useunknownincatchvariables" => opts.use_unknown_in_catch_variables = parse_bool(value),
        "resolvejsonmodule" => opts.resolve_json_module = parse_bool(value),
        "isolateddeclarations" => opts.isolated_declarations = parse_bool(value),
        "emitdeclarationonly" => opts.emit_declaration_only = parse_bool(value),
        "importsnotusedasvalues" => {
            opts.imports_not_used_as_values = ImportsNotUsedAsValues::parse(value)
        }
        "exactoptionalpropertytypes" => opts.exact_optional_property_types = parse_bool(value),
        "isolatedmodules" => opts.isolated_modules = parse_bool(value),
        "preserveconstenums" => opts.preserve_const_enums = parse_bool(value),
        "moduleresolution" => opts.module_resolution = Some(value.to_string()),
        "baseurl" => opts.base_url = Some(value.to_string()),
        "paths" => opts.paths = Some(value.to_string()),
        "noemitornerror" | "noemitorerror" => opts.no_emit_on_error = parse_bool(value),
        "incremental" => opts.incremental = parse_bool(value),
        "composite" => opts.composite = parse_bool(value),
        "tsbuildinfofile" => opts.ts_build_info_file = Some(value.to_string()),
        "strictpropertyinitialization" => opts.strict_property_initialization = parse_bool(value),
        "reactnamespace" => {
            // reactNamespace sets the JSX factory namespace (e.g. "factory" → "factory.createElement")
            // Only apply if jsxFactory wasn't explicitly set
            if opts.jsx_factory.is_none() {
                opts.jsx_factory = Some(format!("{}.createElement", value));
            }
            // Keep the raw value for option validation (TS5059).
            opts.other.push((key.to_string(), value.to_string()));
        }
        _ => opts.other.push((key.to_string(), value.to_string())),
    }
}

/// Returns `true` if `key` (already lowercased) is a known compiler option
/// that should be stripped from file content even when it appears after code.
fn is_known_option(key: &str) -> bool {
    matches!(
        key,
        "target"
            | "module"
            | "strict"
            | "noimplicitany"
            | "noimplicitreturns"
            | "nounusedlocals"
            | "nounusedparameters"
            | "strictnullchecks"
            | "strictfunctiontypes"
            | "noemit"
            | "declaration"
            | "sourcemap"
            | "jsx"
            | "jsxfactory"
            | "jsximportsource"
            | "jsxfragmentfactory"
            | "lib"
            | "types"
            | "outfile"
            | "outdir"
            | "rootdir"
            | "allowjs"
            | "checkjs"
            | "esmoduleinterop"
            | "allowsyntheticdefaultimports"
            | "skiplibcheck"
            | "skipdefaultlibcheck"
            | "nolib"
            | "noerrortruncation"
            | "alwaysstrict"
            | "experimentaldecorators"
            | "emitdecoratormetadata"
            | "usedefineforclassfields"
            | "moduledetection"
            | "verbatimmodulesyntax"
            | "nocheck"
            | "noresolve"
            | "downleveliteration"
            | "importhelpers"
            | "emitbom"
            | "newline"
            | "removecomments"
            | "noemithelpers"
            | "pretty"
            | "nofallthroughcasesinswitch"
            | "allowunreachablecode"
            | "useunknownincatchvariables"
            | "resolvejsonmodule"
            | "isolateddeclarations"
            | "emitdeclarationonly"
            | "importsnotusedasvalues"
            | "exactoptionalpropertytypes"
            | "isolatedmodules"
            | "preserveconstenums"
            | "moduleresolution"
            | "baseurl"
            | "paths"
            | "noemitornerror"
            | "noemitorerror"
            | "incremental"
            | "composite"
            | "tsbuildinfofile"
            | "reactnamespace"
            | "inlinesourcemap"
            | "inlinesources"
            | "maproot"
            | "sourceroot"
            | "declarationmap"
            | "fullemitpaths"
            | "currentdirectory"
            | "noimplicitreferences"
            | "erasablesyntaxonly"
            | "strictpropertyinitialization"
    )
}

fn parse_bool(s: &str) -> Option<bool> {
    // Handle comma-separated multi-values like "true, false" by
    // taking the first parseable value.
    if s.contains(',') {
        return s.split(',').find_map(parse_bool_single);
    }
    parse_bool_single(s)
}

fn parse_bool_single(s: &str) -> Option<bool> {
    match s.trim().to_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_standard_and_node_module_options_without_aliasing() {
        assert_eq!(ScriptTarget::parse("ES2025"), Some(ScriptTarget::ES2025));
        assert_ne!(ScriptTarget::parse("ES2025"), Some(ScriptTarget::ESNext));
        assert_eq!(ModuleKind::parse("Node18"), Some(ModuleKind::Node18));
        assert_eq!(ModuleKind::parse("Node20"), Some(ModuleKind::Node20));
        assert_ne!(ModuleKind::parse("Node18"), Some(ModuleKind::Node16));
        assert_ne!(ModuleKind::parse("Node20"), Some(ModuleKind::NodeNext));

        for target in [
            ScriptTarget::ES3,
            ScriptTarget::ES5,
            ScriptTarget::ES2015,
            ScriptTarget::ES2016,
            ScriptTarget::ES2017,
            ScriptTarget::ES2018,
            ScriptTarget::ES2019,
            ScriptTarget::ES2020,
            ScriptTarget::ES2021,
            ScriptTarget::ES2022,
            ScriptTarget::ES2023,
            ScriptTarget::ES2024,
            ScriptTarget::ES2025,
            ScriptTarget::ESNext,
        ] {
            assert_eq!(ScriptTarget::parse(target.as_str()), Some(target));
        }
        for module in [
            ModuleKind::None,
            ModuleKind::CommonJS,
            ModuleKind::AMD,
            ModuleKind::UMD,
            ModuleKind::System,
            ModuleKind::ES2015,
            ModuleKind::ES2020,
            ModuleKind::ES2022,
            ModuleKind::ESNext,
            ModuleKind::Node16,
            ModuleKind::Node18,
            ModuleKind::Node20,
            ModuleKind::NodeNext,
            ModuleKind::Preserve,
        ] {
            assert_eq!(ModuleKind::parse(module.as_str()), Some(module));
        }
    }

    #[test]
    fn parse_test_case_preserves_all_trailing_newlines() {
        let source = "const value = 1;\n\n\n";
        let test_case = parse_test_case("case.ts", source);
        assert_eq!(test_case.files[0].content, source);
    }
}
