//! Expression lowering: HIR `Expr` → KIR instructions.

use rustc_hash::FxHashSet;

use kyokara_hir_def::expr::{CallArg, Expr, ExprIdx, Literal, MatchArm, Stmt};
use kyokara_hir_def::item_tree::TypeDefKind;
use kyokara_hir_def::pat::Pat;
use kyokara_hir_def::path::Path;
use kyokara_hir_ty::ty::Ty;

use crate::block::{BranchTarget, SwitchCase, Terminator};
use crate::inst::{CallTarget, Constant, Inst};
use crate::value::ValueId;

use super::LoweringCtx;

impl<'a> LoweringCtx<'a> {
    /// Lower an HIR expression to a KIR value.
    pub(crate) fn lower_expr(&mut self, expr_idx: ExprIdx) -> ValueId {
        let expr = self.body.exprs[expr_idx].clone();
        let ty = self.expr_ty(expr_idx);

        match expr {
            Expr::Literal(lit) => self.lower_literal(lit, ty),
            Expr::Path(path) => self.lower_path(path, ty),
            Expr::Binary { op, lhs, rhs } => {
                let lv = self.lower_expr(lhs);
                let rv = self.lower_expr(rhs);
                self.builder.push_binary(op, lv, rv, ty)
            }
            Expr::Unary { op, operand } => {
                let v = self.lower_expr(operand);
                self.builder.push_unary(op, v, ty)
            }
            Expr::Call { callee, args } => self.lower_call(callee, args, ty),
            Expr::Field { base, field } => {
                let bv = self.lower_expr(base);
                self.builder.push_field_get(bv, field, ty)
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => self.lower_if(condition, then_branch, else_branch, ty),
            Expr::Match { scrutinee, arms } => self.lower_match(scrutinee, arms, ty),
            Expr::Block { stmts, tail } => self.lower_block(stmts, tail, ty),
            Expr::Return(val) => self.lower_return(val),
            Expr::RecordLit { path, fields } => self.lower_record_lit(path, fields, ty),
            Expr::Lambda { .. } => {
                let id = self.next_hole_id();
                self.builder.push_hole(id, vec![], ty)
            }
            Expr::Old(inner) => self.lower_expr_in_old_scope(inner),
            Expr::Hole => {
                let id = self.next_hole_id();
                self.builder.push_hole(id, vec![], ty)
            }
            Expr::Missing => self.builder.push_const(Constant::Unit, Ty::Error),
        }
    }

    // ── Literal ──────────────────────────────────────────────────

    fn lower_literal(&mut self, lit: Literal, ty: Ty) -> ValueId {
        let c = match lit {
            Literal::Int(v) => Constant::Int(v),
            Literal::Float(v) => Constant::Float(v),
            Literal::String(s) => Constant::String(s),
            Literal::Char(c) => Constant::Char(c),
            Literal::Bool(b) => Constant::Bool(b),
        };
        self.builder.push_const(c, ty)
    }

    // ── Path ─────────────────────────────────────────────────────

    fn lower_path(&mut self, path: Path, ty: Ty) -> ValueId {
        if path.segments.is_empty() {
            let id = self.next_hole_id();
            return self.builder.push_hole(id, vec![], ty);
        }

        let first = path.segments[0];

        // Local variable — chain field gets for multi-segment paths.
        if let Some(vid) = self.lookup_local(first) {
            return self.chain_field_gets(vid, &path.segments[1..], ty);
        }

        // ADT constructor (zero-arg produces a value directly).
        if let Some(&(type_idx, variant_idx)) = self.module_scope.constructors.get(&first) {
            let is_nullary = matches!(
                &self.item_tree.types[type_idx].kind,
                TypeDefKind::Adt { variants } if variants[variant_idx].fields.is_empty()
            );
            if is_nullary {
                let ctor_val = self
                    .builder
                    .push_adt_construct(type_idx, first, vec![], ty.clone());
                return self.chain_field_gets(ctor_val, &path.segments[1..], ty);
            }
            // Multi-field constructor as value — placeholder.
            let id = self.next_hole_id();
            return self.builder.push_hole(id, vec![], ty);
        }

        // Function reference — placeholder for first-class fn values.
        if self.module_scope.functions.contains_key(&first) {
            let id = self.next_hole_id();
            return self.builder.push_hole(id, vec![], ty);
        }

        // Unknown — emit hole.
        let id = self.next_hole_id();
        self.builder.push_hole(id, vec![], ty)
    }

    fn chain_field_gets(
        &mut self,
        mut val: ValueId,
        segments: &[kyokara_hir_def::name::Name],
        final_ty: Ty,
    ) -> ValueId {
        if segments.is_empty() {
            return val;
        }
        for (i, &seg) in segments.iter().enumerate() {
            let ty = if i == segments.len() - 1 {
                final_ty.clone()
            } else {
                Ty::Error // intermediate types not easily available
            };
            val = self.builder.push_field_get(val, seg, ty);
        }
        val
    }

    // ── Call ─────────────────────────────────────────────────────

    fn lower_call(&mut self, callee: ExprIdx, args: Vec<CallArg>, ty: Ty) -> ValueId {
        let callee_expr = self.body.exprs[callee].clone();

        // Simple path callee (common case).
        if let Expr::Path(ref path) = callee_expr
            && path.is_single()
        {
            let name = path.segments[0];
            let arg_vals = self.lower_call_args(&args);

            // 1. Local variable (indirect call) — locals shadow everything.
            if let Some(vid) = self.lookup_local(name) {
                return self
                    .builder
                    .push_call(CallTarget::Indirect(vid), arg_vals, ty);
            }

            // 2. Constructor call → AdtConstruct.
            if let Some(&(type_idx, _)) = self.module_scope.constructors.get(&name) {
                return self
                    .builder
                    .push_adt_construct(type_idx, name, arg_vals, ty);
            }

            // 3. Intrinsic (has entry in functions but no body).
            if self.intrinsics.contains(&name) {
                let name_str = name.resolve(self.interner).to_string();
                return self
                    .builder
                    .push_call(CallTarget::Intrinsic(name_str), arg_vals, ty);
            }

            // 4. Module-level function (direct call).
            if self.module_scope.functions.contains_key(&name) {
                return self
                    .builder
                    .push_call(CallTarget::Direct(name), arg_vals, ty);
            }

            // Fallback: treat as direct call (might be imported).
            return self
                .builder
                .push_call(CallTarget::Direct(name), arg_vals, ty);
        }

        // Complex callee expression.
        let callee_val = self.lower_expr(callee);
        let arg_vals = self.lower_call_args(&args);
        self.builder
            .push_call(CallTarget::Indirect(callee_val), arg_vals, ty)
    }

    fn lower_call_args(&mut self, args: &[CallArg]) -> Vec<ValueId> {
        args.iter()
            .map(|arg| {
                let idx = match arg {
                    CallArg::Positional(idx) => *idx,
                    CallArg::Named { value, .. } => *value,
                };
                self.lower_expr(idx)
            })
            .collect()
    }

    // ── If ───────────────────────────────────────────────────────

    fn lower_if(
        &mut self,
        condition: ExprIdx,
        then_branch: ExprIdx,
        else_branch: Option<ExprIdx>,
        ty: Ty,
    ) -> ValueId {
        let cond_val = self.lower_expr(condition);

        let then_blk = self.builder.new_block(Some(self.labels.then_));
        let else_blk = self.builder.new_block(Some(self.labels.else_));
        let merge_blk = self.builder.new_block(Some(self.labels.merge));

        self.builder.set_branch(
            cond_val,
            BranchTarget {
                block: then_blk,
                args: vec![],
            },
            BranchTarget {
                block: else_blk,
                args: vec![],
            },
        );

        // Then branch.
        self.builder.switch_to(then_blk);
        let then_val = self.lower_expr(then_branch);
        let then_term = self.block_has_terminator();
        if !then_term {
            self.builder.set_jump(BranchTarget {
                block: merge_blk,
                args: vec![then_val],
            });
        }

        // Else branch.
        self.builder.switch_to(else_blk);
        let else_val = match else_branch {
            Some(e) => self.lower_expr(e),
            None => self.builder.push_const(Constant::Unit, Ty::Unit),
        };
        let else_term = self.block_has_terminator();
        if !else_term {
            self.builder.set_jump(BranchTarget {
                block: merge_blk,
                args: vec![else_val],
            });
        }

        // Merge block.
        let result = self.builder.add_block_param(merge_blk, None, ty);
        self.builder.switch_to(merge_blk);
        if then_term && else_term {
            self.builder.set_unreachable();
        }
        result
    }

    // ── Match ────────────────────────────────────────────────────

    fn lower_match(&mut self, scrutinee: ExprIdx, arms: Vec<MatchArm>, ty: Ty) -> ValueId {
        let scr_ty = self.expr_ty(scrutinee);
        let scr_val = self.lower_expr(scrutinee);

        if self.is_adt_match(&scr_ty, &arms) {
            self.lower_match_adt(scr_val, &arms, ty)
        } else {
            self.lower_match_sequential(scr_val, &arms, ty)
        }
    }

    fn is_adt_match(&self, scr_ty: &Ty, arms: &[MatchArm]) -> bool {
        matches!(scr_ty, Ty::Adt { .. })
            && arms
                .iter()
                .any(|arm| matches!(&self.body.pats[arm.pat], Pat::Constructor { .. }))
    }

    fn lower_match_adt(&mut self, scr: ValueId, arms: &[MatchArm], ty: Ty) -> ValueId {
        let merge_blk = self.builder.new_block(Some(self.labels.merge));
        let switch_blk = self.builder.current_block().unwrap();

        // First pass: create case blocks, collect switch info.
        let mut cases = Vec::new();
        let mut default_target = None;
        let mut seen_variants = FxHashSet::default();

        struct ArmInfo {
            block: crate::block::BlockId,
            body: ExprIdx,
            pat_data: Pat,
        }
        let mut arm_infos = Vec::new();

        for arm in arms {
            // Once a catch-all arm is seen, all subsequent arms are
            // unreachable — stop building switch dispatch entries.
            if default_target.is_some() {
                break;
            }

            let pat = self.body.pats[arm.pat].clone();
            match &pat {
                Pat::Constructor { path, .. } => {
                    let ctor_name = path.last().unwrap();
                    // Skip duplicate constructor arms (first match wins).
                    if !seen_variants.insert(ctor_name) {
                        continue;
                    }
                    let case_blk = self.builder.new_block(Some(ctor_name));
                    cases.push(SwitchCase {
                        variant: ctor_name,
                        target: BranchTarget {
                            block: case_blk,
                            args: vec![],
                        },
                    });
                    arm_infos.push(ArmInfo {
                        block: case_blk,
                        body: arm.body,
                        pat_data: pat,
                    });
                }
                Pat::Wildcard | Pat::Bind { .. } => {
                    let default_blk = self.builder.new_block(Some(self.labels.default));
                    default_target = Some(BranchTarget {
                        block: default_blk,
                        args: vec![],
                    });
                    arm_infos.push(ArmInfo {
                        block: default_blk,
                        body: arm.body,
                        pat_data: pat,
                    });
                }
                _ => {}
            }
        }

        // Build a fallback target for nested pattern mismatch (default or unreachable).
        let has_default = default_target.is_some();
        let fallback_target = default_target.clone().unwrap_or_else(|| {
            let dead = self.builder.new_block(None);
            BranchTarget {
                block: dead,
                args: vec![],
            }
        });

        // Emit switch in the original block.
        self.builder.switch_to(switch_blk);
        self.builder.set_terminator(Terminator::Switch {
            scrutinee: scr,
            cases,
            default: default_target,
        });

        // Second pass: lower each arm's body.
        let mut all_terminated = true;
        for info in arm_infos {
            self.builder.switch_to(info.block);
            self.push_scope();

            match &info.pat_data {
                Pat::Constructor { args, .. } => {
                    // Extract fields and check nested literal subpatterns.
                    let mut field_vals = Vec::new();
                    for (i, sub_pat) in args.iter().enumerate() {
                        let field_ty = self.pat_ty(*sub_pat);
                        let field_val = self.builder.push_adt_field_get(scr, i as u32, field_ty);
                        field_vals.push((*sub_pat, field_val));
                    }

                    // Emit equality checks for nested literal subpatterns.
                    for &(sub_pat_idx, field_val) in &field_vals {
                        let sub_pat = self.body.pats[sub_pat_idx].clone();
                        if let Pat::Literal(lit) = sub_pat {
                            let lit_const = literal_to_constant(&lit);
                            let field_ty = self.builder.value_ty(field_val).clone();
                            let lit_val = self.builder.push_const(lit_const, field_ty);
                            let eq_val = self.builder.push_binary(
                                kyokara_hir_def::expr::BinaryOp::Eq,
                                field_val,
                                lit_val,
                                Ty::Bool,
                            );
                            // Branch: match → continue, mismatch → fallback.
                            let continue_blk = self.builder.new_block(None);
                            self.builder.set_branch(
                                eq_val,
                                BranchTarget {
                                    block: continue_blk,
                                    args: vec![],
                                },
                                fallback_target.clone(),
                            );
                            self.builder.switch_to(continue_blk);
                        } else {
                            self.bind_pattern(sub_pat_idx, field_val);
                        }
                    }
                }
                Pat::Bind { name } => {
                    self.define_local(*name, scr);
                }
                Pat::Wildcard => {}
                _ => {}
            }

            let body_val = self.lower_expr(info.body);
            if !self.block_has_terminator() {
                self.builder.set_jump(BranchTarget {
                    block: merge_blk,
                    args: vec![body_val],
                });
                all_terminated = false;
            }
            self.pop_scope();
        }

        // If we created a dead fallback block (no default arm), mark it unreachable.
        if !has_default {
            self.builder.switch_to(fallback_target.block);
            if !self.block_has_terminator() {
                self.builder.set_unreachable();
            }
        }

        let result = self.builder.add_block_param(merge_blk, None, ty);
        self.builder.switch_to(merge_blk);
        if all_terminated {
            self.builder.set_unreachable();
        }
        result
    }

    fn lower_match_sequential(&mut self, scr: ValueId, arms: &[MatchArm], ty: Ty) -> ValueId {
        let merge_blk = self.builder.new_block(Some(self.labels.merge));
        let mut all_terminated = true;

        for (i, arm) in arms.iter().enumerate() {
            let pat = self.body.pats[arm.pat].clone();
            let is_last = i == arms.len() - 1;

            match &pat {
                Pat::Literal(lit) => {
                    let lit_const = literal_to_constant(lit);
                    let scr_ty = self.builder_value_ty(scr);
                    let lit_val = self.builder.push_const(lit_const, scr_ty);
                    let eq_val = self.builder.push_binary(
                        kyokara_hir_def::expr::BinaryOp::Eq,
                        scr,
                        lit_val,
                        Ty::Bool,
                    );

                    let body_blk = self.builder.new_block(None);
                    let next_blk = if is_last {
                        // Last literal arm — create an unreachable fallthrough block.
                        let dead_blk = self.builder.new_block(None);
                        self.builder.set_branch(
                            eq_val,
                            BranchTarget {
                                block: body_blk,
                                args: vec![],
                            },
                            BranchTarget {
                                block: dead_blk,
                                args: vec![],
                            },
                        );
                        // Mark the fallthrough as unreachable.
                        self.builder.switch_to(dead_blk);
                        self.builder.set_unreachable();
                        None
                    } else {
                        let next_blk = self.builder.new_block(None);
                        self.builder.set_branch(
                            eq_val,
                            BranchTarget {
                                block: body_blk,
                                args: vec![],
                            },
                            BranchTarget {
                                block: next_blk,
                                args: vec![],
                            },
                        );
                        Some(next_blk)
                    };

                    // Lower the arm body.
                    self.builder.switch_to(body_blk);
                    self.push_scope();
                    let body_val = self.lower_expr(arm.body);
                    if !self.block_has_terminator() {
                        self.builder.set_jump(BranchTarget {
                            block: merge_blk,
                            args: vec![body_val],
                        });
                        all_terminated = false;
                    }
                    self.pop_scope();

                    // Continue in next_blk for subsequent arms.
                    if let Some(next) = next_blk {
                        self.builder.switch_to(next);
                    }
                }
                Pat::Wildcard => {
                    self.push_scope();
                    let body_val = self.lower_expr(arm.body);
                    if !self.block_has_terminator() {
                        self.builder.set_jump(BranchTarget {
                            block: merge_blk,
                            args: vec![body_val],
                        });
                        all_terminated = false;
                    }
                    self.pop_scope();
                    break; // catch-all: subsequent arms are unreachable
                }
                Pat::Bind { name } => {
                    self.push_scope();
                    self.define_local(*name, scr);
                    let body_val = self.lower_expr(arm.body);
                    if !self.block_has_terminator() {
                        self.builder.set_jump(BranchTarget {
                            block: merge_blk,
                            args: vec![body_val],
                        });
                        all_terminated = false;
                    }
                    self.pop_scope();
                    break; // catch-all: subsequent arms are unreachable
                }
                Pat::Record { .. } | Pat::Constructor { .. } => {
                    self.push_scope();
                    self.bind_pattern(arm.pat, scr);
                    let body_val = self.lower_expr(arm.body);
                    if !self.block_has_terminator() {
                        self.builder.set_jump(BranchTarget {
                            block: merge_blk,
                            args: vec![body_val],
                        });
                        all_terminated = false;
                    }
                    self.pop_scope();
                }
                _ => {}
            }
        }

        let result = self.builder.add_block_param(merge_blk, None, ty);
        self.builder.switch_to(merge_blk);
        if all_terminated {
            self.builder.set_unreachable();
        }
        result
    }

    /// Get the type of an already-allocated value.
    fn builder_value_ty(&self, vid: ValueId) -> Ty {
        self.builder.value_ty(vid).clone()
    }

    // ── Block ────────────────────────────────────────────────────

    fn lower_block(&mut self, stmts: Vec<Stmt>, tail: Option<ExprIdx>, _ty: Ty) -> ValueId {
        self.push_scope();

        for stmt in &stmts {
            if self.block_has_terminator() {
                break; // dead code after return
            }
            match stmt {
                Stmt::Let { pat, init, .. } => {
                    let init_val = self.lower_expr(*init);
                    self.bind_pattern(*pat, init_val);
                }
                Stmt::Expr(expr) => {
                    self.lower_expr(*expr);
                }
            }
        }

        let result = if self.block_has_terminator() {
            // Block already terminated (e.g. by a return statement).
            self.builder
                .alloc_value(Ty::Never, Inst::Const(Constant::Unit))
        } else if let Some(tail_expr) = tail {
            self.lower_expr(tail_expr)
        } else {
            self.builder.push_const(Constant::Unit, Ty::Unit)
        };

        self.pop_scope();
        result
    }

    // ── Return ───────────────────────────────────────────────────

    fn lower_return(&mut self, val: Option<ExprIdx>) -> ValueId {
        let ret_val = match val {
            Some(expr) => self.lower_expr(expr),
            None => self.builder.push_const(Constant::Unit, Ty::Unit),
        };

        // Emit ensures assertion before the return terminator.
        if let (Some(ens_expr), Some(rn)) = (self.ensures_expr, self.result_name) {
            // Temporarily clear ensures_expr to avoid re-entrant emission.
            self.ensures_expr = None;
            self.push_scope();
            self.define_local(rn, ret_val);
            let cond = self.lower_expr(ens_expr);
            let vid = self
                .builder
                .push_assert(cond, "ensures".to_string(), Ty::Unit);
            self.ensures_vids.push(vid);
            self.pop_scope();
            // Restore ensures_expr for subsequent return statements.
            self.ensures_expr = Some(ens_expr);
        }

        self.builder.set_return(ret_val);

        // Create dead block for any subsequent code.
        let dead_blk = self.builder.new_block(None);
        self.builder.switch_to(dead_blk);
        self.builder.set_unreachable();

        // Return a dummy value (not pushed to any block).
        self.builder
            .alloc_value(Ty::Never, Inst::Const(Constant::Unit))
    }

    // ── RecordLit ────────────────────────────────────────────────

    fn lower_record_lit(
        &mut self,
        path: Option<Path>,
        fields: Vec<(kyokara_hir_def::name::Name, ExprIdx)>,
        ty: Ty,
    ) -> ValueId {
        let field_vals: Vec<_> = fields
            .iter()
            .map(|(name, expr)| (*name, self.lower_expr(*expr)))
            .collect();

        // Named constructor → AdtConstruct.
        if let Some(path) = &path
            && let Some(ctor_name) = path.last()
            && let Some(&(type_idx, _)) = self.module_scope.constructors.get(&ctor_name)
        {
            let vals: Vec<_> = field_vals.into_iter().map(|(_, v)| v).collect();
            return self
                .builder
                .push_adt_construct(type_idx, ctor_name, vals, ty);
        }

        // Plain record literal.
        self.builder.push_record_create(field_vals, ty)
    }
}

fn literal_to_constant(lit: &kyokara_hir_def::expr::Literal) -> Constant {
    match lit {
        Literal::Int(v) => Constant::Int(*v),
        Literal::Float(v) => Constant::Float(*v),
        Literal::String(s) => Constant::String(s.clone()),
        Literal::Char(c) => Constant::Char(*c),
        Literal::Bool(b) => Constant::Bool(*b),
    }
}
