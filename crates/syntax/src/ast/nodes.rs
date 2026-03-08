//! Typed AST wrappers for every CST node kind.
//!
//! Each wrapper checks `node.kind()` on `cast()` and exposes child
//! accessors returning typed wrappers or tokens.

use kyokara_parser::SyntaxKind;
use std::marker::PhantomData;

use crate::ast::traits::{HasName, HasTypeParams, HasVisibility};
use crate::ast::{AstNode, support};
use crate::language::{KyokaraLanguage, SyntaxNode, SyntaxToken};

// ── Macro ──────────────────────────────────────────────────────────

macro_rules! define_ast_node {
    ($name:ident, $kind:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name {
            syntax: SyntaxNode,
        }

        impl AstNode for $name {
            fn can_cast(kind: SyntaxKind) -> bool {
                kind == SyntaxKind::$kind
            }

            fn cast(node: SyntaxNode) -> Option<Self> {
                if Self::can_cast(node.kind()) {
                    Some(Self { syntax: node })
                } else {
                    None
                }
            }

            fn syntax(&self) -> &SyntaxNode {
                &self.syntax
            }
        }
    };
}

// ── Top-level ──────────────────────────────────────────────────────

define_ast_node!(SourceFile, SourceFile);

impl SourceFile {
    pub fn module_decl(&self) -> Option<ModuleDecl> {
        support::child(&self.syntax)
    }

    pub fn imports(&self) -> impl Iterator<Item = ImportDecl> + '_ {
        support::children(&self.syntax)
    }

    pub fn items(&self) -> impl Iterator<Item = Item> + '_ {
        self.syntax.children().filter_map(Item::cast)
    }
}

/// An item — top-level definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Item {
    TypeDef(TypeDef),
    TraitDef(TraitDef),
    ImplDef(ImplDef),
    FnDef(FnDef),
    EffectDef(EffectDef),
    PropertyDef(PropertyDef),
    LetBinding(LetBinding),
}

impl Item {
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::TypeDef => TypeDef::cast(node).map(Item::TypeDef),
            SyntaxKind::TraitDef => TraitDef::cast(node).map(Item::TraitDef),
            SyntaxKind::ImplDef => ImplDef::cast(node).map(Item::ImplDef),
            SyntaxKind::FnDef => FnDef::cast(node).map(Item::FnDef),
            SyntaxKind::EffectDef => EffectDef::cast(node).map(Item::EffectDef),
            SyntaxKind::PropertyDef => PropertyDef::cast(node).map(Item::PropertyDef),
            SyntaxKind::LetBinding => LetBinding::cast(node).map(Item::LetBinding),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct ChildNodeIter<N> {
    children: Option<rowan::SyntaxNodeChildren<KyokaraLanguage>>,
    _marker: PhantomData<N>,
}

impl<N> ChildNodeIter<N> {
    fn from_parent(parent: Option<SyntaxNode>) -> Self {
        Self {
            children: parent.map(|node| node.children()),
            _marker: PhantomData,
        }
    }
}

impl<N: AstNode> Iterator for ChildNodeIter<N> {
    type Item = N;

    fn next(&mut self) -> Option<Self::Item> {
        let children = self.children.as_mut()?;
        for node in children {
            if let Some(cast) = N::cast(node) {
                return Some(cast);
            }
        }
        None
    }
}

define_ast_node!(ModuleDecl, ModuleDecl);

impl ModuleDecl {
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }
}

define_ast_node!(ImportDecl, ImportDecl);

impl ImportDecl {
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    pub fn alias(&self) -> Option<ImportAlias> {
        support::child(&self.syntax)
    }
}

define_ast_node!(ImportAlias, ImportAlias);

impl HasName for ImportAlias {}

define_ast_node!(Path, Path);

impl Path {
    /// All `Ident` tokens forming this path's segments.
    pub fn segments(&self) -> impl Iterator<Item = SyntaxToken> + '_ {
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::Ident)
    }
}

// ── Items ──────────────────────────────────────────────────────────

define_ast_node!(TypeDef, TypeDef);

impl HasName for TypeDef {}
impl HasTypeParams for TypeDef {}
impl HasVisibility for TypeDef {}

impl TypeDef {
    pub fn derive_clause(&self) -> Option<DeriveClause> {
        support::child(&self.syntax)
    }

    /// The type body — either a variant list, record field list, or a type expression.
    pub fn variant_list(&self) -> Option<VariantList> {
        support::child(&self.syntax)
    }

    /// If the body is a single type expression (type alias).
    pub fn type_expr(&self) -> Option<TypeExpr> {
        self.syntax.children().find_map(TypeExpr::cast)
    }

    /// If the body is a record field list.
    pub fn record_field_list(&self) -> Option<RecordFieldList> {
        support::child(&self.syntax)
    }
}

define_ast_node!(TraitDef, TraitDef);

impl HasName for TraitDef {}
impl HasTypeParams for TraitDef {}
impl HasVisibility for TraitDef {}

impl TraitDef {
    pub fn supertrait_list(&self) -> Option<SupertraitList> {
        support::child(&self.syntax)
    }

    pub fn method_sigs(&self) -> impl Iterator<Item = TraitMethodSig> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(ImplDef, ImplDef);

impl HasTypeParams for ImplDef {}

impl ImplDef {
    pub fn trait_ref(&self) -> Option<TraitRef> {
        support::child(&self.syntax)
    }

    pub fn self_type(&self) -> Option<TypeExpr> {
        self.syntax.children().find_map(TypeExpr::cast)
    }

    pub fn methods(&self) -> impl Iterator<Item = ImplMethodDef> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(FnDef, FnDef);

impl HasName for FnDef {}
impl HasTypeParams for FnDef {}
impl HasVisibility for FnDef {}

impl FnDef {
    pub fn param_list(&self) -> Option<ParamList> {
        support::child(&self.syntax)
    }

    pub fn return_type(&self) -> Option<ReturnType> {
        support::child(&self.syntax)
    }

    pub fn body(&self) -> Option<BlockExpr> {
        support::child(&self.syntax)
    }

    pub fn with_clause(&self) -> Option<WithClause> {
        support::child(&self.syntax)
    }

    pub fn contract_section(&self) -> Option<ContractSection> {
        support::child(&self.syntax)
    }

    pub fn requires_clauses(&self) -> impl Iterator<Item = RequiresClause> + '_ {
        ChildNodeIter::from_parent(self.contract_section().map(|c| c.syntax))
    }

    pub fn ensures_clauses(&self) -> impl Iterator<Item = EnsuresClause> + '_ {
        ChildNodeIter::from_parent(self.contract_section().map(|c| c.syntax))
    }

    pub fn invariant_clauses(&self) -> impl Iterator<Item = InvariantClause> + '_ {
        ChildNodeIter::from_parent(self.contract_section().map(|c| c.syntax))
    }

    pub fn requires_clause(&self) -> Option<RequiresClause> {
        self.requires_clauses().next()
    }

    pub fn ensures_clause(&self) -> Option<EnsuresClause> {
        self.ensures_clauses().next()
    }

    pub fn invariant_clause(&self) -> Option<InvariantClause> {
        self.invariant_clauses().next()
    }

    /// For method definitions (`fn Type.method(...)`), returns the receiver
    /// type name token (`Type`). Returns `None` for regular function defs.
    pub fn receiver_type_token(&self) -> Option<SyntaxToken> {
        // A method has a Dot token directly inside FnDef (not in a child node).
        let has_dot = self
            .syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .any(|tok| tok.kind() == SyntaxKind::Dot);
        if !has_dot {
            return None;
        }
        // The first Ident token is the receiver type name.
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|tok| tok.kind() == SyntaxKind::Ident)
    }

    /// For method definitions (`fn Type.method(...)`), returns the method
    /// name token. Returns `None` for regular function defs.
    pub fn method_name_token(&self) -> Option<SyntaxToken> {
        // Only valid for method defs (those with a Dot token).
        let has_dot = self
            .syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .any(|tok| tok.kind() == SyntaxKind::Dot);
        if !has_dot {
            return None;
        }
        // The second Ident token (after the dot) is the method name.
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::Ident)
            .nth(1)
    }
}

define_ast_node!(EffectDef, EffectDef);

impl HasName for EffectDef {}
impl HasTypeParams for EffectDef {}
impl HasVisibility for EffectDef {}

define_ast_node!(PropertyDef, PropertyDef);

impl HasName for PropertyDef {}

impl PropertyDef {
    pub fn property_param_list(&self) -> Option<PropertyParamList> {
        support::child(&self.syntax)
    }

    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(&self.syntax)
    }

    pub fn body(&self) -> Option<BlockExpr> {
        support::child(&self.syntax)
    }
}

define_ast_node!(LetBinding, LetBinding);

impl LetBinding {
    pub fn pat(&self) -> Option<Pat> {
        self.syntax.children().find_map(Pat::cast)
    }

    pub fn type_expr(&self) -> Option<TypeExpr> {
        self.syntax.children().find_map(TypeExpr::cast)
    }

    pub fn value(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

define_ast_node!(VarBinding, VarBinding);

impl HasName for VarBinding {}

impl VarBinding {
    pub fn type_expr(&self) -> Option<TypeExpr> {
        self.syntax.children().find_map(TypeExpr::cast)
    }

    pub fn value(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

// ── Type-def sub-nodes ─────────────────────────────────────────────

define_ast_node!(RecordFieldList, RecordFieldList);

impl RecordFieldList {
    pub fn fields(&self) -> impl Iterator<Item = RecordField> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(RecordField, RecordField);

impl HasName for RecordField {}

impl RecordField {
    pub fn type_expr(&self) -> Option<TypeExpr> {
        self.syntax.children().find_map(TypeExpr::cast)
    }
}

define_ast_node!(VariantList, VariantList);

impl VariantList {
    pub fn variants(&self) -> impl Iterator<Item = Variant> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(Variant, Variant);

impl HasName for Variant {}

impl Variant {
    pub fn field_list(&self) -> Option<VariantFieldList> {
        support::child(&self.syntax)
    }
}

define_ast_node!(VariantFieldList, VariantFieldList);

impl VariantFieldList {
    pub fn fields(&self) -> impl Iterator<Item = VariantField> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(VariantField, VariantField);

impl VariantField {
    pub fn type_expr(&self) -> Option<TypeExpr> {
        self.syntax.children().find_map(TypeExpr::cast)
    }
}

// ── Function sub-nodes ─────────────────────────────────────────────

define_ast_node!(ParamList, ParamList);

impl ParamList {
    pub fn params(&self) -> impl Iterator<Item = Param> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(Param, Param);

impl HasName for Param {}

impl Param {
    pub fn type_expr(&self) -> Option<TypeExpr> {
        self.syntax.children().find_map(TypeExpr::cast)
    }
}

define_ast_node!(ReturnType, ReturnType);

impl ReturnType {
    pub fn type_expr(&self) -> Option<TypeExpr> {
        self.syntax.children().find_map(TypeExpr::cast)
    }
}

define_ast_node!(WithClause, WithClause);

impl WithClause {
    pub fn types(&self) -> impl Iterator<Item = TypeExpr> + '_ {
        self.syntax.children().filter_map(TypeExpr::cast)
    }
}

define_ast_node!(ContractSection, ContractSection);

impl ContractSection {
    pub fn requires_clauses(&self) -> impl Iterator<Item = RequiresClause> + '_ {
        support::children(&self.syntax)
    }

    pub fn ensures_clauses(&self) -> impl Iterator<Item = EnsuresClause> + '_ {
        support::children(&self.syntax)
    }

    pub fn invariant_clauses(&self) -> impl Iterator<Item = InvariantClause> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(RequiresClause, RequiresClause);

impl RequiresClause {
    pub fn expr(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

define_ast_node!(EnsuresClause, EnsuresClause);

impl EnsuresClause {
    pub fn expr(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

define_ast_node!(InvariantClause, InvariantClause);

impl InvariantClause {
    pub fn expr(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

// ── Generics ───────────────────────────────────────────────────────

define_ast_node!(DeriveClause, DeriveClause);

impl DeriveClause {
    pub fn trait_refs(&self) -> impl Iterator<Item = TraitRef> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(TraitRef, TraitRef);

impl TraitRef {
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    pub fn type_arg_list(&self) -> Option<TypeArgList> {
        support::child(&self.syntax)
    }
}

define_ast_node!(SupertraitList, SupertraitList);

impl SupertraitList {
    pub fn trait_refs(&self) -> impl Iterator<Item = TraitRef> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(TraitMethodSig, TraitMethodSig);

impl HasName for TraitMethodSig {}
impl HasTypeParams for TraitMethodSig {}

impl TraitMethodSig {
    pub fn param_list(&self) -> Option<ParamList> {
        support::child(&self.syntax)
    }

    pub fn return_type(&self) -> Option<ReturnType> {
        support::child(&self.syntax)
    }
}

define_ast_node!(ImplMethodDef, ImplMethodDef);

impl HasName for ImplMethodDef {}
impl HasTypeParams for ImplMethodDef {}

impl ImplMethodDef {
    pub fn param_list(&self) -> Option<ParamList> {
        support::child(&self.syntax)
    }

    pub fn return_type(&self) -> Option<ReturnType> {
        support::child(&self.syntax)
    }

    pub fn body(&self) -> Option<BlockExpr> {
        support::child(&self.syntax)
    }

    pub fn with_clause(&self) -> Option<WithClause> {
        support::child(&self.syntax)
    }

    pub fn contract_section(&self) -> Option<ContractSection> {
        support::child(&self.syntax)
    }

    pub fn requires_clauses(&self) -> impl Iterator<Item = RequiresClause> + '_ {
        ChildNodeIter::from_parent(self.contract_section().map(|c| c.syntax))
    }

    pub fn ensures_clauses(&self) -> impl Iterator<Item = EnsuresClause> + '_ {
        ChildNodeIter::from_parent(self.contract_section().map(|c| c.syntax))
    }

    pub fn invariant_clauses(&self) -> impl Iterator<Item = InvariantClause> + '_ {
        ChildNodeIter::from_parent(self.contract_section().map(|c| c.syntax))
    }
}

define_ast_node!(TypeParamList, TypeParamList);

impl TypeParamList {
    pub fn type_params(&self) -> impl Iterator<Item = TypeParam> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(TypeParam, TypeParam);

impl HasName for TypeParam {}

impl TypeParam {
    pub fn bound_list(&self) -> Option<TypeParamBoundList> {
        support::child(&self.syntax)
    }
}

define_ast_node!(TypeParamBoundList, TypeParamBoundList);

impl TypeParamBoundList {
    pub fn trait_refs(&self) -> impl Iterator<Item = TraitRef> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(TypeArgList, TypeArgList);

impl TypeArgList {
    pub fn type_args(&self) -> impl Iterator<Item = TypeExpr> + '_ {
        self.syntax.children().filter_map(TypeExpr::cast)
    }
}

// ── Type expressions ───────────────────────────────────────────────

/// A type expression — dispatch enum.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeExpr {
    NameType(NameType),
    FnType(FnType),
    RecordType(RecordType),
    RefinedType(RefinedType),
}

impl TypeExpr {
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::NameType => NameType::cast(node).map(TypeExpr::NameType),
            SyntaxKind::FnType => FnType::cast(node).map(TypeExpr::FnType),
            SyntaxKind::RecordType => RecordType::cast(node).map(TypeExpr::RecordType),
            SyntaxKind::RefinedType => RefinedType::cast(node).map(TypeExpr::RefinedType),
            _ => None,
        }
    }

    pub fn syntax(&self) -> &SyntaxNode {
        match self {
            TypeExpr::NameType(n) => n.syntax(),
            TypeExpr::FnType(n) => n.syntax(),
            TypeExpr::RecordType(n) => n.syntax(),
            TypeExpr::RefinedType(n) => n.syntax(),
        }
    }
}

define_ast_node!(NameType, NameType);

impl NameType {
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    pub fn type_arg_list(&self) -> Option<TypeArgList> {
        support::child(&self.syntax)
    }
}

define_ast_node!(FnType, FnType);

impl FnType {
    /// Parameter types (all type expressions except the last which is the return type).
    pub fn param_types(&self) -> impl Iterator<Item = TypeExpr> + '_ {
        self.syntax.children().filter_map(TypeExpr::cast)
    }
}

define_ast_node!(RecordType, RecordType);

impl RecordType {
    pub fn fields(&self) -> impl Iterator<Item = RecordField> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(RefinedType, RefinedType);

impl RefinedType {
    pub fn name_token(&self) -> Option<SyntaxToken> {
        support::name_token(&self.syntax)
    }

    pub fn base_type(&self) -> Option<TypeExpr> {
        self.syntax.children().find_map(TypeExpr::cast)
    }

    pub fn predicate(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

// ── Expressions ────────────────────────────────────────────────────

/// An expression — dispatch enum.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    Literal(LiteralExpr),
    Path(PathExpr),
    Binary(BinaryExpr),
    Unary(UnaryExpr),
    Call(CallExpr),
    Field(FieldExpr),
    Index(IndexExpr),
    Pipeline(PipelineExpr),
    Propagate(PropagateExpr),
    Match(MatchExpr),
    If(IfExpr),
    Block(BlockExpr),
    Record(RecordExpr),
    Return(ReturnExpr),
    Hole(HoleExpr),
    Old(OldExpr),
    Paren(ParenExpr),
    Lambda(LambdaExpr),
}

impl Expr {
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::LiteralExpr => LiteralExpr::cast(node).map(Expr::Literal),
            SyntaxKind::PathExpr => PathExpr::cast(node).map(Expr::Path),
            SyntaxKind::BinaryExpr => BinaryExpr::cast(node).map(Expr::Binary),
            SyntaxKind::UnaryExpr => UnaryExpr::cast(node).map(Expr::Unary),
            SyntaxKind::CallExpr => CallExpr::cast(node).map(Expr::Call),
            SyntaxKind::FieldExpr => FieldExpr::cast(node).map(Expr::Field),
            SyntaxKind::IndexExpr => IndexExpr::cast(node).map(Expr::Index),
            SyntaxKind::PipelineExpr => PipelineExpr::cast(node).map(Expr::Pipeline),
            SyntaxKind::PropagateExpr => PropagateExpr::cast(node).map(Expr::Propagate),
            SyntaxKind::MatchExpr => MatchExpr::cast(node).map(Expr::Match),
            SyntaxKind::IfExpr => IfExpr::cast(node).map(Expr::If),
            SyntaxKind::BlockExpr => BlockExpr::cast(node).map(Expr::Block),
            SyntaxKind::RecordExpr => RecordExpr::cast(node).map(Expr::Record),
            SyntaxKind::ReturnExpr => ReturnExpr::cast(node).map(Expr::Return),
            SyntaxKind::HoleExpr => HoleExpr::cast(node).map(Expr::Hole),
            SyntaxKind::OldExpr => OldExpr::cast(node).map(Expr::Old),
            SyntaxKind::ParenExpr => ParenExpr::cast(node).map(Expr::Paren),
            SyntaxKind::LambdaExpr => LambdaExpr::cast(node).map(Expr::Lambda),
            _ => None,
        }
    }

    pub fn syntax(&self) -> &SyntaxNode {
        match self {
            Expr::Literal(n) => n.syntax(),
            Expr::Path(n) => n.syntax(),
            Expr::Binary(n) => n.syntax(),
            Expr::Unary(n) => n.syntax(),
            Expr::Call(n) => n.syntax(),
            Expr::Field(n) => n.syntax(),
            Expr::Index(n) => n.syntax(),
            Expr::Pipeline(n) => n.syntax(),
            Expr::Propagate(n) => n.syntax(),
            Expr::Match(n) => n.syntax(),
            Expr::If(n) => n.syntax(),
            Expr::Block(n) => n.syntax(),
            Expr::Record(n) => n.syntax(),
            Expr::Return(n) => n.syntax(),
            Expr::Hole(n) => n.syntax(),
            Expr::Old(n) => n.syntax(),
            Expr::Paren(n) => n.syntax(),
            Expr::Lambda(n) => n.syntax(),
        }
    }
}

define_ast_node!(LiteralExpr, LiteralExpr);

impl LiteralExpr {
    /// The literal token (IntLiteral, FloatLiteral, StringLiteral, CharLiteral, TrueKw, FalseKw).
    pub fn token(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .find_map(|it| it.into_token())
    }
}

define_ast_node!(PathExpr, PathExpr);

impl PathExpr {
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }
}

define_ast_node!(BinaryExpr, BinaryExpr);

impl BinaryExpr {
    pub fn lhs(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    pub fn rhs(&self) -> Option<Expr> {
        self.syntax.children().filter_map(Expr::cast).nth(1)
    }

    pub fn op_token(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|tok| tok.kind().is_binary_operator())
    }
}

define_ast_node!(UnaryExpr, UnaryExpr);

impl UnaryExpr {
    pub fn operand(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    pub fn op_token(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|tok| tok.kind().is_unary_prefix_operator())
    }
}

define_ast_node!(CallExpr, CallExpr);

impl CallExpr {
    /// The callee expression (first child expr).
    pub fn callee(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    pub fn arg_list(&self) -> Option<ArgList> {
        support::child(&self.syntax)
    }
}

define_ast_node!(ArgList, ArgList);

impl ArgList {
    /// Positional and named arguments as expressions.
    pub fn args(&self) -> impl Iterator<Item = Expr> + '_ {
        self.syntax.children().filter_map(Expr::cast)
    }

    pub fn named_args(&self) -> impl Iterator<Item = NamedArg> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(NamedArg, NamedArg);

impl HasName for NamedArg {}

impl NamedArg {
    pub fn value(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

define_ast_node!(FieldExpr, FieldExpr);

impl FieldExpr {
    pub fn base(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    pub fn field_token(&self) -> Option<SyntaxToken> {
        // The field name is the last Ident token (after the dot).
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::Ident)
            .last()
    }
}

define_ast_node!(IndexExpr, IndexExpr);

impl IndexExpr {
    pub fn base(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    pub fn index(&self) -> Option<Expr> {
        self.syntax.children().filter_map(Expr::cast).nth(1)
    }
}

define_ast_node!(PipelineExpr, PipelineExpr);

impl PipelineExpr {
    pub fn lhs(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    pub fn rhs(&self) -> Option<Expr> {
        self.syntax.children().filter_map(Expr::cast).nth(1)
    }
}

define_ast_node!(PropagateExpr, PropagateExpr);

impl PropagateExpr {
    pub fn inner(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

define_ast_node!(MatchExpr, MatchExpr);

impl MatchExpr {
    pub fn scrutinee(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    pub fn arm_list(&self) -> Option<MatchArmList> {
        support::child(&self.syntax)
    }
}

define_ast_node!(MatchArmList, MatchArmList);

impl MatchArmList {
    pub fn arms(&self) -> impl Iterator<Item = MatchArm> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(MatchArm, MatchArm);

impl MatchArm {
    pub fn pat(&self) -> Option<Pat> {
        self.syntax.children().find_map(Pat::cast)
    }

    pub fn body(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

define_ast_node!(IfExpr, IfExpr);

impl IfExpr {
    pub fn condition(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    pub fn then_branch(&self) -> Option<BlockExpr> {
        support::children::<BlockExpr>(&self.syntax).next()
    }

    pub fn else_branch(&self) -> Option<ElseBranch> {
        let mut seen_else = false;
        for child in self.syntax.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(tok) => {
                    if tok.kind() == SyntaxKind::ElseKw {
                        seen_else = true;
                    }
                }
                rowan::NodeOrToken::Node(node) if seen_else => {
                    if let Some(if_expr) = IfExpr::cast(node.clone()) {
                        return Some(ElseBranch::IfExpr(if_expr));
                    }
                    if let Some(block) = BlockExpr::cast(node) {
                        return Some(ElseBranch::Block(block));
                    }
                }
                rowan::NodeOrToken::Node(_) => {}
            }
        }
        None
    }
}

/// The else branch of an `if` expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ElseBranch {
    Block(BlockExpr),
    IfExpr(IfExpr),
}

define_ast_node!(WhileStmt, WhileStmt);

impl WhileStmt {
    pub fn condition(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    pub fn body(&self) -> Option<BlockExpr> {
        support::child(&self.syntax)
    }
}

define_ast_node!(ForStmt, ForStmt);

impl ForStmt {
    pub fn pat(&self) -> Option<Pat> {
        self.syntax.children().find_map(Pat::cast)
    }

    pub fn source(&self) -> Option<Expr> {
        let mut after_in = false;
        for elem in self.syntax.children_with_tokens() {
            if let Some(tok) = elem.as_token() {
                if tok.kind() == SyntaxKind::InKw {
                    after_in = true;
                    continue;
                }
                if after_in && tok.kind() == SyntaxKind::RParen {
                    break;
                }
                continue;
            }
            if !after_in {
                continue;
            }
            let node = elem.into_node()?;
            if let Some(expr) = Expr::cast(node) {
                return Some(expr);
            }
        }
        None
    }

    pub fn body(&self) -> Option<BlockExpr> {
        let mut after_head = false;
        for elem in self.syntax.children_with_tokens() {
            if let Some(tok) = elem.as_token() {
                if tok.kind() == SyntaxKind::RParen {
                    after_head = true;
                }
                continue;
            }
            if !after_head {
                continue;
            }
            let node = elem.into_node()?;
            if let Some(block) = BlockExpr::cast(node) {
                return Some(block);
            }
        }
        None
    }
}

define_ast_node!(BreakStmt, BreakStmt);
define_ast_node!(ContinueStmt, ContinueStmt);
define_ast_node!(AssignStmt, AssignStmt);

impl AssignStmt {
    pub fn target(&self) -> Option<Expr> {
        self.syntax.children().filter_map(Expr::cast).next()
    }

    pub fn value(&self) -> Option<Expr> {
        self.syntax.children().filter_map(Expr::cast).nth(1)
    }
}

define_ast_node!(BlockExpr, BlockExpr);

impl BlockExpr {
    /// Statements and expressions inside the block.
    pub fn stmts(&self) -> impl Iterator<Item = BlockItem> + '_ {
        self.syntax.children().filter_map(BlockItem::cast)
    }
}

/// An item inside a block — either a let binding or an expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BlockItem {
    LetBinding(LetBinding),
    VarBinding(VarBinding),
    WhileStmt(WhileStmt),
    ForStmt(ForStmt),
    AssignStmt(AssignStmt),
    BreakStmt(BreakStmt),
    ContinueStmt(ContinueStmt),
    Expr(Expr),
}

impl BlockItem {
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        if let Some(let_b) = LetBinding::cast(node.clone()) {
            return Some(BlockItem::LetBinding(let_b));
        }
        if let Some(var_b) = VarBinding::cast(node.clone()) {
            return Some(BlockItem::VarBinding(var_b));
        }
        if let Some(while_stmt) = WhileStmt::cast(node.clone()) {
            return Some(BlockItem::WhileStmt(while_stmt));
        }
        if let Some(for_stmt) = ForStmt::cast(node.clone()) {
            return Some(BlockItem::ForStmt(for_stmt));
        }
        if let Some(assign_stmt) = AssignStmt::cast(node.clone()) {
            return Some(BlockItem::AssignStmt(assign_stmt));
        }
        if let Some(break_stmt) = BreakStmt::cast(node.clone()) {
            return Some(BlockItem::BreakStmt(break_stmt));
        }
        if let Some(continue_stmt) = ContinueStmt::cast(node.clone()) {
            return Some(BlockItem::ContinueStmt(continue_stmt));
        }
        Expr::cast(node).map(BlockItem::Expr)
    }
}

define_ast_node!(RecordExpr, RecordExpr);

impl RecordExpr {
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    pub fn field_list(&self) -> Option<RecordExprFieldList> {
        support::child(&self.syntax)
    }
}

define_ast_node!(RecordExprFieldList, RecordExprFieldList);

impl RecordExprFieldList {
    pub fn fields(&self) -> impl Iterator<Item = RecordExprField> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(RecordExprField, RecordExprField);

impl HasName for RecordExprField {}

impl RecordExprField {
    pub fn value(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

define_ast_node!(ReturnExpr, ReturnExpr);

impl ReturnExpr {
    pub fn value(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

define_ast_node!(HoleExpr, HoleExpr);
define_ast_node!(OldExpr, OldExpr);

impl OldExpr {
    pub fn inner(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

define_ast_node!(ParenExpr, ParenExpr);

impl ParenExpr {
    pub fn inner(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

define_ast_node!(LambdaExpr, LambdaExpr);

impl LambdaExpr {
    pub fn param_list(&self) -> Option<ParamList> {
        support::child(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

// ── Patterns ───────────────────────────────────────────────────────

/// A pattern — dispatch enum.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Pat {
    Ident(IdentPat),
    Constructor(ConstructorPat),
    Wildcard(WildcardPat),
    Literal(LiteralPat),
    Record(RecordPat),
}

impl Pat {
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::IdentPat => IdentPat::cast(node).map(Pat::Ident),
            SyntaxKind::ConstructorPat => ConstructorPat::cast(node).map(Pat::Constructor),
            SyntaxKind::WildcardPat => WildcardPat::cast(node).map(Pat::Wildcard),
            SyntaxKind::LiteralPat => LiteralPat::cast(node).map(Pat::Literal),
            SyntaxKind::RecordPat => RecordPat::cast(node).map(Pat::Record),
            _ => None,
        }
    }

    pub fn syntax(&self) -> &SyntaxNode {
        match self {
            Pat::Ident(n) => n.syntax(),
            Pat::Constructor(n) => n.syntax(),
            Pat::Wildcard(n) => n.syntax(),
            Pat::Literal(n) => n.syntax(),
            Pat::Record(n) => n.syntax(),
        }
    }
}

define_ast_node!(IdentPat, IdentPat);

impl IdentPat {
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }
}

define_ast_node!(ConstructorPat, ConstructorPat);

impl ConstructorPat {
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    pub fn args(&self) -> impl Iterator<Item = Pat> + '_ {
        self.syntax.children().filter_map(Pat::cast)
    }
}

define_ast_node!(WildcardPat, WildcardPat);
define_ast_node!(LiteralPat, LiteralPat);

impl LiteralPat {
    pub fn token(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .find_map(|it| it.into_token())
    }
}

define_ast_node!(RecordPat, RecordPat);

impl RecordPat {
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    /// Field names in the record pattern.
    pub fn field_names(&self) -> impl Iterator<Item = SyntaxToken> + '_ {
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::Ident)
    }
}

define_ast_node!(PatList, PatList);

// ── Property ───────────────────────────────────────────────────────

define_ast_node!(PropertyParamList, PropertyParamList);

impl PropertyParamList {
    pub fn params(&self) -> impl Iterator<Item = PropertyParam> + '_ {
        support::children(&self.syntax)
    }
}

define_ast_node!(PropertyParam, PropertyParam);

impl HasName for PropertyParam {}

impl PropertyParam {
    pub fn type_expr(&self) -> Option<TypeExpr> {
        self.syntax.children().find_map(TypeExpr::cast)
    }

    pub fn generator(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

define_ast_node!(WhereClause, WhereClause);

impl WhereClause {
    pub fn expr(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

define_ast_node!(ForAllBinder, ForAllBinder);

impl HasName for ForAllBinder {}

impl ForAllBinder {
    pub fn type_expr(&self) -> Option<TypeExpr> {
        self.syntax.children().find_map(TypeExpr::cast)
    }
}

// ── Error ──────────────────────────────────────────────────────────

define_ast_node!(ErrorNode, ErrorNode);
