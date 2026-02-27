//! Flat exhaustiveness and redundancy checking for match expressions.
//!
//! v0.0: checks that all ADT constructors are covered (or a wildcard/bind
//! pattern is present). No nested pattern decomposition.

use kyokara_hir_def::expr::{ExprIdx, Literal, MatchArm};
use kyokara_hir_def::item_tree::{ItemTree, TypeDefKind, TypeItemIdx};
use kyokara_hir_def::pat::Pat;
use kyokara_intern::Interner;
use kyokara_stdx::FxHashSet;
use la_arena::Arena;

use crate::diagnostics::TyDiagnosticData;
use crate::ty::Ty;

/// Check that a match on an ADT type is exhaustive and has no redundant arms.
pub fn check_exhaustiveness(
    type_idx: TypeItemIdx,
    arms: &[MatchArm],
    pats: &Arena<Pat>,
    item_tree: &ItemTree,
    interner: &Interner,
    diags: &mut Vec<(TyDiagnosticData, ExprIdx)>,
    match_expr_idx: ExprIdx,
) {
    let type_item = &item_tree.types[type_idx];
    let variants = match &type_item.kind {
        TypeDefKind::Adt { variants } => variants,
        _ => return, // Not an ADT — nothing to check.
    };

    let mut covered: FxHashSet<usize> = FxHashSet::default();
    let mut has_wildcard = false;
    let mut wildcard_seen_at: Option<usize> = None;

    for (arm_idx, arm) in arms.iter().enumerate() {
        let pat = &pats[arm.pat];
        match pat {
            Pat::Wildcard | Pat::Bind { .. } => {
                if has_wildcard || covered.len() == variants.len() {
                    diags.push((TyDiagnosticData::RedundantMatchArm, match_expr_idx));
                }
                if !has_wildcard {
                    has_wildcard = true;
                    wildcard_seen_at = Some(arm_idx);
                }
            }
            Pat::Constructor { path, .. } => {
                if has_wildcard {
                    // Arms after a wildcard are redundant.
                    diags.push((TyDiagnosticData::RedundantMatchArm, match_expr_idx));
                    continue;
                }
                // Find which variant this constructor matches.
                if let Some(name) = path.last()
                    && let Some(variant_idx) = variants
                        .iter()
                        .position(|v| v.name.resolve(interner) == name.resolve(interner))
                    && !covered.insert(variant_idx)
                {
                    diags.push((TyDiagnosticData::RedundantMatchArm, match_expr_idx));
                }
            }
            Pat::Literal(_) => {
                // Literal patterns against an ADT — can't contribute to exhaustiveness.
            }
            Pat::Record { .. } | Pat::Missing => {}
        }
    }

    // Check exhaustiveness: if no wildcard, all variants must be covered.
    let _ = wildcard_seen_at;
    if !has_wildcard && covered.len() < variants.len() {
        let missing: Vec<String> = variants
            .iter()
            .enumerate()
            .filter(|(i, _)| !covered.contains(i))
            .map(|(_, v)| v.name.resolve(interner).to_owned())
            .collect();
        diags.push((
            TyDiagnosticData::MissingMatchArms { missing },
            match_expr_idx,
        ));
    }
}

/// Check match exhaustiveness for non-ADT scrutinees.
///
/// Conservative rule:
/// - wildcard / bind arm means exhaustive
/// - `Bool` with both `true` and `false` literal arms means exhaustive
/// - otherwise emit `MissingMatchArms`
pub fn check_non_adt_exhaustiveness(
    scrutinee_ty: &Ty,
    arms: &[MatchArm],
    pats: &Arena<Pat>,
    diags: &mut Vec<(TyDiagnosticData, ExprIdx)>,
    match_expr_idx: ExprIdx,
) {
    if scrutinee_ty.is_poison() {
        return;
    }

    if arms
        .iter()
        .any(|arm| matches!(&pats[arm.pat], Pat::Wildcard | Pat::Bind { .. }))
    {
        return;
    }

    if is_bool_literal_exhaustive(scrutinee_ty, arms, pats) {
        return;
    }

    diags.push((
        TyDiagnosticData::MissingMatchArms {
            missing: vec!["_".to_string()],
        },
        match_expr_idx,
    ));
}

fn is_bool_literal_exhaustive(scrutinee_ty: &Ty, arms: &[MatchArm], pats: &Arena<Pat>) -> bool {
    if !matches!(scrutinee_ty, Ty::Bool) {
        return false;
    }

    let mut has_true = false;
    let mut has_false = false;
    for arm in arms {
        if let Pat::Literal(Literal::Bool(v)) = &pats[arm.pat] {
            if *v {
                has_true = true;
            } else {
                has_false = true;
            }
        }
    }

    has_true && has_false
}
