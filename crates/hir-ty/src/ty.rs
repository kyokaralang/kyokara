//! Resolved type representation.
//!
//! [`Ty`] is the fully-resolved counterpart of [`kyokara_hir_def::type_ref::TypeRef`].
//! Inference variables, built-in primitives, ADT references, structural records,
//! function types, and poison/never types all live here.

use kyokara_hir_def::item_tree::TypeItemIdx;
use kyokara_hir_def::name::Name;

/// Opaque identifier for a type inference variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyVarId(pub(crate) u32);

/// A fully-resolved type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// Inference variable — will be resolved during unification.
    Var(TyVarId),

    // ── Built-in primitives ──────────────────────────────────────
    Int,
    Float,
    String,
    Char,
    Bool,
    Unit,

    /// User-defined ADT or record type, potentially with type arguments.
    Adt {
        def: TypeItemIdx,
        args: Vec<Ty>,
    },

    /// Structural (anonymous) record type.
    Record {
        fields: Vec<(Name, Ty)>,
    },

    /// Function type: `(params) -> ret`.
    Fn {
        params: Vec<Ty>,
        ret: Box<Ty>,
    },

    /// Poison type — unifies with anything, prevents cascading errors.
    Error,

    /// Diverging expression type (e.g. `return`). Unifies with anything.
    Never,
}

impl Ty {
    /// Returns `true` if this type is a poison or never type that should
    /// silently unify with anything.
    pub fn is_poison(&self) -> bool {
        matches!(self, Ty::Error | Ty::Never)
    }

    /// Returns `true` if this type contains no inference variables.
    pub fn is_fully_resolved(&self) -> bool {
        match self {
            Ty::Var(_) => false,
            Ty::Int | Ty::Float | Ty::String | Ty::Char | Ty::Bool | Ty::Unit => true,
            Ty::Error | Ty::Never => true,
            Ty::Adt { args, .. } => args.iter().all(Ty::is_fully_resolved),
            Ty::Record { fields } => fields.iter().all(|(_, t)| t.is_fully_resolved()),
            Ty::Fn { params, ret } => {
                params.iter().all(Ty::is_fully_resolved) && ret.is_fully_resolved()
            }
        }
    }
}

/// Recognise built-in type names.
pub fn resolve_builtin(name: &str) -> Option<Ty> {
    match name {
        "Int" => Some(Ty::Int),
        "Float" => Some(Ty::Float),
        "String" => Some(Ty::String),
        "Char" => Some(Ty::Char),
        "Bool" => Some(Ty::Bool),
        "Unit" => Some(Ty::Unit),
        _ => None,
    }
}

/// Human-readable display of a type (for diagnostics).
///
/// Without an `ItemTree`, ADT types display as `<adt>`. Prefer
/// [`display_ty_with_tree`] when the item tree is available.
pub fn display_ty(ty: &Ty, interner: &kyokara_intern::Interner) -> std::string::String {
    display_ty_inner(ty, interner, None)
}

/// Human-readable display that resolves ADT names from the item tree.
pub fn display_ty_with_tree(
    ty: &Ty,
    interner: &kyokara_intern::Interner,
    tree: &kyokara_hir_def::item_tree::ItemTree,
) -> std::string::String {
    display_ty_inner(ty, interner, Some(tree))
}

fn display_ty_inner(
    ty: &Ty,
    interner: &kyokara_intern::Interner,
    tree: Option<&kyokara_hir_def::item_tree::ItemTree>,
) -> std::string::String {
    match ty {
        Ty::Var(v) => format!("?{}", v.0),
        Ty::Int => "Int".into(),
        Ty::Float => "Float".into(),
        Ty::String => "String".into(),
        Ty::Char => "Char".into(),
        Ty::Bool => "Bool".into(),
        Ty::Unit => "Unit".into(),
        Ty::Error => "<error>".into(),
        Ty::Never => "Never".into(),
        Ty::Adt { def, args } => {
            let name = tree
                .map(|t| t.types[*def].name.resolve(interner).to_owned())
                .unwrap_or_else(|| "<adt>".to_owned());
            if args.is_empty() {
                name
            } else {
                let args_str: Vec<_> = args
                    .iter()
                    .map(|a| display_ty_inner(a, interner, tree))
                    .collect();
                format!("{}<{}>", name, args_str.join(", "))
            }
        }
        Ty::Record { fields } => {
            let fs: Vec<_> = fields
                .iter()
                .map(|(n, t)| {
                    format!(
                        "{}: {}",
                        n.resolve(interner),
                        display_ty_inner(t, interner, tree)
                    )
                })
                .collect();
            format!("{{ {} }}", fs.join(", "))
        }
        Ty::Fn { params, ret } => {
            let ps: Vec<_> = params
                .iter()
                .map(|p| display_ty_inner(p, interner, tree))
                .collect();
            format!(
                "fn({}) -> {}",
                ps.join(", "),
                display_ty_inner(ret, interner, tree)
            )
        }
    }
}
