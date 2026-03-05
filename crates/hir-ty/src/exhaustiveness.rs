//! Flat exhaustiveness and redundancy checking for match expressions.
//!
//! v0.0: checks that all ADT constructors are covered (or a wildcard/bind
//! pattern is present). No nested pattern decomposition.

use kyokara_hir_def::expr::{ExprIdx, Literal, MatchArm};
use kyokara_hir_def::item_tree::{ItemTree, TypeDefKind, TypeItemIdx};
use kyokara_hir_def::pat::Pat;
use kyokara_intern::Interner;
use kyokara_stdx::{FxHashMap, FxHashSet};
use la_arena::{Arena, ArenaMap};

use crate::diagnostics::TyDiagnosticData;
use crate::infer::DiagLoc;
use crate::ty::Ty;
use crate::unify::UnificationTable;

pub(crate) struct AdtExhaustivenessInput<'a> {
    pub type_idx: TypeItemIdx,
    pub arms: &'a [MatchArm],
    pub pats: &'a Arena<Pat>,
    pub pat_types: &'a ArenaMap<la_arena::Idx<Pat>, Ty>,
    pub table: &'a UnificationTable,
    pub item_tree: &'a ItemTree,
    pub interner: &'a Interner,
    pub match_expr_idx: ExprIdx,
}

/// Check that a match on an ADT type is exhaustive and has no redundant arms.
pub(crate) fn check_exhaustiveness(
    input: AdtExhaustivenessInput<'_>,
    diags: &mut Vec<(TyDiagnosticData, DiagLoc)>,
) {
    let AdtExhaustivenessInput {
        type_idx,
        arms,
        pats,
        pat_types,
        table,
        item_tree,
        interner,
        match_expr_idx,
    } = input;

    let type_item = &item_tree.types[type_idx];
    let variants = match &type_item.kind {
        TypeDefKind::Adt { variants } => variants,
        _ => return, // Not an ADT — nothing to check.
    };

    let mut covered: FxHashSet<usize> = FxHashSet::default();
    let mut bool_coverage: FxHashMap<usize, BoolCoverage> = FxHashMap::default();
    let mut has_wildcard = false;
    let mut wildcard_seen_at: Option<usize> = None;

    for (arm_idx, arm) in arms.iter().enumerate() {
        let pat = &pats[arm.pat];
        match pat {
            Pat::Wildcard | Pat::Bind { .. } => {
                if has_wildcard || covered.len() == variants.len() {
                    diags.push((
                        TyDiagnosticData::RedundantMatchArm,
                        DiagLoc::Expr(match_expr_idx),
                    ));
                }
                if !has_wildcard {
                    has_wildcard = true;
                    wildcard_seen_at = Some(arm_idx);
                }
            }
            Pat::Constructor { path, args } => {
                if has_wildcard {
                    // Arms after a wildcard are redundant.
                    diags.push((
                        TyDiagnosticData::RedundantMatchArm,
                        DiagLoc::Expr(match_expr_idx),
                    ));
                    continue;
                }

                let Some(name) = path.last() else {
                    continue;
                };
                let Some(variant_idx) = variants
                    .iter()
                    .position(|v| v.name.resolve(interner) == name.resolve(interner))
                else {
                    continue;
                };
                if covered.contains(&variant_idx) {
                    diags.push((
                        TyDiagnosticData::RedundantMatchArm,
                        DiagLoc::Expr(match_expr_idx),
                    ));
                    continue;
                }

                // Conservative nested-pattern handling:
                // only constructor arms with irrefutable argument patterns
                // (wildcards/binds) are considered full variant coverage.
                // Refined subpatterns (e.g. Some(1), Some(Some(x))) are not
                // treated as covering the whole variant.
                let args_irrefutable = args
                    .iter()
                    .all(|arg| matches!(&pats[*arg], Pat::Wildcard | Pat::Bind { .. }));
                if !args_irrefutable {
                    if let Some((arity, assignments)) =
                        bool_assignments(args, pats, pat_types, table)
                    {
                        let Some(domain_size) = (1_usize).checked_shl(arity as u32) else {
                            continue;
                        };
                        let entry =
                            bool_coverage
                                .entry(variant_idx)
                                .or_insert_with(|| BoolCoverage {
                                    arity,
                                    seen: FxHashSet::default(),
                                });
                        if entry.arity != arity {
                            continue;
                        }
                        let mut added_any = false;
                        for assignment in assignments {
                            if entry.seen.insert(assignment) {
                                added_any = true;
                            }
                        }
                        if !added_any {
                            diags.push((
                                TyDiagnosticData::RedundantMatchArm,
                                DiagLoc::Expr(match_expr_idx),
                            ));
                        } else if entry.seen.len() == domain_size {
                            covered.insert(variant_idx);
                        }
                    }
                    continue;
                }
                if !covered.insert(variant_idx) {
                    diags.push((
                        TyDiagnosticData::RedundantMatchArm,
                        DiagLoc::Expr(match_expr_idx),
                    ));
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
            DiagLoc::Expr(match_expr_idx),
        ));
    }
}

#[derive(Debug)]
struct BoolCoverage {
    arity: usize,
    seen: FxHashSet<u64>,
}

fn bool_assignments(
    args: &[la_arena::Idx<Pat>],
    pats: &Arena<Pat>,
    pat_types: &ArenaMap<la_arena::Idx<Pat>, Ty>,
    table: &UnificationTable,
) -> Option<(usize, Vec<u64>)> {
    if args.is_empty() || args.len() > u64::BITS as usize {
        return None;
    }

    let mut assignments = vec![0_u64];
    for (idx, arg) in args.iter().enumerate() {
        let ty = pat_types.get(*arg)?;
        if !matches!(table.resolve_deep(ty), Ty::Bool) {
            return None;
        }

        match &pats[*arg] {
            Pat::Literal(Literal::Bool(value)) => {
                if *value {
                    for assignment in &mut assignments {
                        *assignment |= 1_u64 << idx;
                    }
                }
            }
            Pat::Wildcard | Pat::Bind { .. } => {
                let mut expanded = Vec::with_capacity(assignments.len() * 2);
                for assignment in assignments {
                    expanded.push(assignment);
                    expanded.push(assignment | (1_u64 << idx));
                }
                assignments = expanded;
            }
            _ => return None,
        }
    }

    Some((args.len(), assignments))
}

/// Check match exhaustiveness for non-ADT scrutinees.
///
/// Conservative rule:
/// - wildcard / bind arm means exhaustive
/// - `Bool` with both `true` and `false` literal arms means exhaustive
/// - otherwise emit `MissingMatchArms`
pub(crate) fn check_non_adt_exhaustiveness(
    scrutinee_ty: &Ty,
    arms: &[MatchArm],
    pats: &Arena<Pat>,
    diags: &mut Vec<(TyDiagnosticData, DiagLoc)>,
    match_expr_idx: ExprIdx,
) {
    if scrutinee_ty.is_poison() {
        return;
    }

    if arms.iter().any(|arm| {
        matches!(
            &pats[arm.pat],
            Pat::Wildcard | Pat::Bind { .. } | Pat::Record { .. }
        )
    }) {
        return;
    }

    if is_bool_literal_exhaustive(scrutinee_ty, arms, pats) {
        return;
    }

    diags.push((
        TyDiagnosticData::MissingMatchArms {
            missing: vec!["_".to_string()],
        },
        DiagLoc::Expr(match_expr_idx),
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
