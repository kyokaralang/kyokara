//! Module-level item tree — signatures without bodies.
//!
//! The `ItemTree` is built by Pass 1 (walking CST top-level items) and
//! contains all items visible in a module's scope.

pub mod lower;

use kyokara_span::TextRange;
use la_arena::{Arena, Idx};

use crate::name::Name;
use crate::path::Path;
use crate::type_ref::TypeRef;

/// Index into the function arena.
pub type FnItemIdx = Idx<FnItem>;
/// Index into the type arena.
pub type TypeItemIdx = Idx<TypeItem>;
/// Index into the capability arena.
pub type CapItemIdx = Idx<CapItem>;
/// Index into the property arena.
pub type PropertyItemIdx = Idx<PropertyItem>;
/// Index into the let-binding arena.
pub type LetItemIdx = Idx<LetItem>;

/// All top-level items in a single module/file.
#[derive(Debug, Default, Clone)]
pub struct ItemTree {
    pub module_name: Option<Path>,
    pub imports: Vec<Import>,
    pub functions: Arena<FnItem>,
    pub types: Arena<TypeItem>,
    pub caps: Arena<CapItem>,
    pub properties: Arena<PropertyItem>,
    pub lets: Arena<LetItem>,
}

/// An import declaration.
#[derive(Debug, Clone)]
pub struct Import {
    pub path: Path,
    pub alias: Option<Name>,
}

/// A function item (signature only — body lowered in Pass 2).
#[derive(Debug, Clone)]
pub struct FnItem {
    pub name: Name,
    pub is_pub: bool,
    pub type_params: Vec<Name>,
    pub params: Vec<FnParam>,
    pub ret_type: Option<TypeRef>,
    pub with_caps: Vec<TypeRef>,
    pub pipe_caps: Vec<TypeRef>,
    pub has_body: bool,
    /// Source range of the CST `FnDef` node (for matching back to syntax).
    pub source_range: Option<TextRange>,
    /// For method definitions (`fn Type.method(self, ...)`), the receiver type name.
    /// `None` for regular (free) functions.
    pub receiver_type: Option<Name>,
}

/// A function parameter.
#[derive(Debug, Clone)]
pub struct FnParam {
    pub name: Name,
    pub ty: TypeRef,
}

/// A type definition.
#[derive(Debug, Clone)]
pub struct TypeItem {
    pub name: Name,
    pub is_pub: bool,
    pub type_params: Vec<Name>,
    pub kind: TypeDefKind,
}

/// The kind of type definition.
#[derive(Debug, Clone)]
pub enum TypeDefKind {
    /// Type alias: `type Foo = Bar`.
    Alias(TypeRef),
    /// Record: `type Foo = { x: Int, y: Int }`.
    Record { fields: Vec<(Name, TypeRef)> },
    /// ADT with variants: `type Foo = | A(Int) | B`.
    Adt { variants: Vec<VariantDef> },
}

/// A variant in an ADT definition.
#[derive(Debug, Clone)]
pub struct VariantDef {
    pub name: Name,
    pub fields: Vec<TypeRef>,
}

/// A capability definition.
#[derive(Debug, Clone)]
pub struct CapItem {
    pub name: Name,
    pub is_pub: bool,
    pub type_params: Vec<Name>,
    pub functions: Vec<FnItemIdx>,
}

/// Specifies which generator to use for a property parameter.
#[derive(Debug, Clone, PartialEq)]
pub enum GenSpec {
    /// `Gen.auto()` — type-driven generation.
    Auto,
    /// `Gen.int()` — unconstrained integer.
    Int,
    /// `Gen.int_range(min, max)` — bounded integer.
    IntRange { min: i64, max: i64 },
    /// `Gen.float()` — unconstrained float.
    Float,
    /// `Gen.float_range(min, max)` — bounded float.
    FloatRange { min: f64, max: f64 },
    /// `Gen.bool()` — random boolean.
    Bool,
    /// `Gen.string()` — random string.
    String,
    /// `Gen.char()` — random character.
    Char,
    /// `Gen.list(inner)` — list with inner generator.
    List(Box<GenSpec>),
    /// `Gen.map(key, val)` — map with key/val generators.
    Map(Box<GenSpec>, Box<GenSpec>),
    /// `Gen.option(inner)` — optional with inner generator.
    OptionOf(Box<GenSpec>),
    /// `Gen.result(ok, err)` — result with ok/err generators.
    ResultOf(Box<GenSpec>, Box<GenSpec>),
}

/// A property parameter with its generator spec.
#[derive(Debug, Clone)]
pub struct PropertyParamSpec {
    pub param: FnParam,
    pub gen_spec: GenSpec,
}

/// A property definition.
#[derive(Debug, Clone)]
pub struct PropertyItem {
    pub name: Name,
    pub params: Vec<PropertyParamSpec>,
    pub has_body: bool,
    /// Source range of the CST `PropertyDef` node (for matching back to syntax).
    pub source_range: Option<TextRange>,
    /// Link to the synthetic `FnItem` created for this property's body.
    pub fn_idx: Option<FnItemIdx>,
}

/// A top-level let binding.
#[derive(Debug, Clone)]
pub struct LetItem {
    pub name: Name,
    pub ty: Option<TypeRef>,
}
