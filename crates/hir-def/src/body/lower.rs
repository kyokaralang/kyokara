//! Pass 2: CST → HIR body lowering.
//!
//! Walks typed AST expressions and patterns to produce HIR `Body`.
//! Desugars `|>` (pipeline) and `?` (propagation) here.

use la_arena::{Arena, ArenaMap};

use kyokara_diagnostics::Diagnostic;
use kyokara_intern::Interner;
use kyokara_span::{FileId, Span, TextRange};
use kyokara_syntax::SyntaxKind;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::{
    self, ArgList, BinaryExpr, BlockExpr, BlockItem, CallExpr, ElseBranch, FieldExpr, FnDef,
    IfExpr, LambdaExpr, LiteralExpr, MatchExpr, NamedArg, OldExpr, PathExpr, PipelineExpr,
    PropagateExpr, PropertyDef, RecordExpr, ReturnExpr, TypeExpr, UnaryExpr,
};
use kyokara_syntax::ast::traits::{HasName, HasTypeParams};

use crate::body::{Body, LocalBindingMeta, LocalBindingOrigin};
use crate::expr::{BinaryOp, CallArg, Expr, ExprIdx, Literal, PatIdx, Stmt, UnaryOp};
use crate::name::Name;
use crate::pat;
use crate::path;
use crate::resolver::ModuleScope;
use crate::scope::{ScopeDef, ScopeIdx, ScopeTree};
use crate::type_ref::TypeRef;

/// Result of body lowering.
pub struct BodyLowerResult {
    pub body: Body,
    pub diagnostics: Vec<Diagnostic>,
}

/// Lower a function body from CST to HIR.
pub fn lower_body(
    fn_def: &FnDef,
    module_scope: &ModuleScope,
    file_id: FileId,
    interner: &mut Interner,
) -> BodyLowerResult {
    let mut ctx = BodyLowerCtx {
        exprs: Arena::new(),
        pats: Arena::new(),
        scopes: ScopeTree::default(),
        pat_scopes: Vec::new(),
        expr_scopes: ArenaMap::default(),
        expr_source_map: ArenaMap::default(),
        pat_source_map: ArenaMap::default(),
        local_binding_meta: ArenaMap::default(),
        diagnostics: Vec::new(),
        file_id,
        interner,
        module_scope,
        current_scope: None,
        in_contract: false,
    };

    // Create root scope
    let root_scope = ctx.scopes.new_root();
    ctx.current_scope = Some(root_scope);

    // Register function parameters in scope
    if let Some(param_list) = fn_def.param_list() {
        for (i, param) in param_list.params().enumerate() {
            if let Some(tok) = param.name_token() {
                let name = Name::new(ctx.interner, tok.text());
                ctx.scopes.define(root_scope, name, ScopeDef::Param(i));
            }
        }
    }

    // Register type parameters in scope
    if let Some(tpl) = HasTypeParams::type_param_list(fn_def) {
        for tp in tpl.type_params() {
            if let Some(tok) = tp.name_token() {
                let name = Name::new(ctx.interner, tok.text());
                // Type params are registered as types
                // For now, just put them in scope so they're resolvable
                ctx.scopes.define(root_scope, name, ScopeDef::Param(0));
            }
        }
    }

    // Lower contract clauses (old() is valid here)
    ctx.in_contract = true;
    let requires = fn_def
        .requires_clause()
        .and_then(|rc| rc.expr())
        .map(|e| ctx.lower_expr(&e));

    let ensures = fn_def.ensures_clause().and_then(|ec| ec.expr()).map(|e| {
        // Introduce implicit `result` binding in ensures scope.
        ctx.push_scope();
        let result_name = Name::new(ctx.interner, "result");
        let result_pat = ctx.alloc_pat(pat::Pat::Bind { name: result_name });
        ctx.pat_source_map
            .insert(result_pat, e.syntax().text_range());
        ctx.register_local_binding(
            result_name,
            result_pat,
            e.syntax().text_range(),
            LocalBindingOrigin::ContractResult,
        );
        let idx = ctx.lower_expr(&e);
        ctx.pop_scope();
        idx
    });

    let invariant = fn_def
        .invariant_clause()
        .and_then(|ic| ic.expr())
        .map(|e| ctx.lower_expr(&e));
    ctx.in_contract = false;

    // Lower body
    let root = if let Some(body) = fn_def.body() {
        let range = body.syntax().text_range();
        let idx = ctx.lower_block(&body);
        ctx.expr_source_map.insert(idx, range);
        idx
    } else {
        ctx.alloc_expr(Expr::Missing)
    };

    BodyLowerResult {
        body: Body {
            exprs: ctx.exprs,
            pats: ctx.pats,
            root,
            requires,
            ensures,
            invariant,
            scopes: ctx.scopes,
            pat_scopes: ctx.pat_scopes,
            expr_scopes: ctx.expr_scopes,
            expr_source_map: ctx.expr_source_map,
            pat_source_map: ctx.pat_source_map,
            local_binding_meta: ctx.local_binding_meta,
        },
        diagnostics: ctx.diagnostics,
    }
}

/// Lower a property body from CST to HIR.
///
/// Similar to `lower_body` but for `PropertyDef` nodes: uses
/// `PropertyParamList` for parameter registration and lowers the
/// optional `where` clause as a `requires` precondition.
pub fn lower_property_body(
    prop_def: &PropertyDef,
    module_scope: &ModuleScope,
    file_id: FileId,
    interner: &mut Interner,
) -> BodyLowerResult {
    let mut ctx = BodyLowerCtx {
        exprs: Arena::new(),
        pats: Arena::new(),
        scopes: ScopeTree::default(),
        pat_scopes: Vec::new(),
        expr_scopes: ArenaMap::default(),
        expr_source_map: ArenaMap::default(),
        pat_source_map: ArenaMap::default(),
        local_binding_meta: ArenaMap::default(),
        diagnostics: Vec::new(),
        file_id,
        interner,
        module_scope,
        current_scope: None,
        in_contract: false,
    };

    // Create root scope.
    let root_scope = ctx.scopes.new_root();
    ctx.current_scope = Some(root_scope);

    // Register parameters in scope from PropertyParamList.
    if let Some(param_list) = prop_def.property_param_list() {
        for (i, param) in param_list.params().enumerate() {
            if let Some(tok) = param.name_token() {
                let name = Name::new(ctx.interner, tok.text());
                ctx.scopes.define(root_scope, name, ScopeDef::Param(i));
            }
        }
    }

    // Lower `where` clause as requires (precondition).
    let requires = if let Some(wc) = prop_def.where_clause() {
        wc.expr().map(|e| {
            ctx.in_contract = true;
            let range = e.syntax().text_range();
            let idx = ctx.lower_expr(&e);
            ctx.expr_source_map.insert(idx, range);
            ctx.in_contract = false;
            idx
        })
    } else {
        None
    };

    // Lower body.
    let root = if let Some(body) = prop_def.body() {
        let range = body.syntax().text_range();
        let idx = ctx.lower_block(&body);
        ctx.expr_source_map.insert(idx, range);
        idx
    } else {
        ctx.alloc_expr(Expr::Missing)
    };

    BodyLowerResult {
        body: Body {
            exprs: ctx.exprs,
            pats: ctx.pats,
            root,
            requires,
            ensures: None,
            invariant: None,
            scopes: ctx.scopes,
            pat_scopes: ctx.pat_scopes,
            expr_scopes: ctx.expr_scopes,
            expr_source_map: ctx.expr_source_map,
            pat_source_map: ctx.pat_source_map,
            local_binding_meta: ctx.local_binding_meta,
        },
        diagnostics: ctx.diagnostics,
    }
}

struct BodyLowerCtx<'a> {
    exprs: Arena<Expr>,
    pats: Arena<pat::Pat>,
    scopes: ScopeTree,
    pat_scopes: Vec<(PatIdx, ScopeIdx)>,
    expr_scopes: ArenaMap<ExprIdx, ScopeIdx>,
    expr_source_map: ArenaMap<ExprIdx, TextRange>,
    pat_source_map: ArenaMap<PatIdx, TextRange>,
    local_binding_meta: ArenaMap<PatIdx, LocalBindingMeta>,
    diagnostics: Vec<Diagnostic>,
    file_id: FileId,
    interner: &'a mut Interner,
    module_scope: &'a ModuleScope,
    current_scope: Option<ScopeIdx>,
    in_contract: bool,
}

impl BodyLowerCtx<'_> {
    fn alloc_expr(&mut self, expr: Expr) -> ExprIdx {
        let idx = self.exprs.alloc(expr);
        if let Some(scope) = self.current_scope {
            self.expr_scopes.insert(idx, scope);
        }
        idx
    }

    fn alloc_pat(&mut self, pat: pat::Pat) -> PatIdx {
        self.pats.alloc(pat)
    }

    fn push_scope(&mut self) -> ScopeIdx {
        let parent = self.current_scope.expect("push_scope without root");
        let new = self.scopes.new_child(parent);
        self.current_scope = Some(new);
        new
    }

    fn pop_scope(&mut self) {
        let current = self.current_scope.expect("pop_scope without scope");
        self.current_scope = self.scopes.scopes[current].parent;
    }

    fn node_span(&self, node: &kyokara_syntax::SyntaxNode) -> Span {
        Span {
            file: self.file_id,
            range: node.text_range(),
        }
    }

    fn register_local_binding(
        &mut self,
        name: Name,
        pat_idx: PatIdx,
        decl_range: TextRange,
        origin: LocalBindingOrigin,
    ) {
        if let Some(scope) = self.current_scope {
            self.scopes.define(scope, name, ScopeDef::Local(pat_idx));
            self.pat_scopes.push((pat_idx, scope));
            self.local_binding_meta.insert(
                pat_idx,
                LocalBindingMeta {
                    origin,
                    decl_range,
                    scope,
                },
            );
        }
    }

    fn validate_numeric_underscores(&mut self, text: &str, range: TextRange) {
        if text.contains('_') && (text.ends_with('_') || text.contains("__")) {
            let span = Span {
                file: self.file_id,
                range,
            };
            self.diagnostics.push(Diagnostic::error(
                format!("invalid underscore placement in numeric literal `{text}`"),
                span,
            ));
        }
    }

    // ── Escape decoding ────────────────────────────────────────────

    /// Decode escape sequences in a string literal interior (quotes already stripped).
    fn decode_string_escapes(&mut self, raw: &str, range: TextRange) -> String {
        let mut out = String::with_capacity(raw.len());
        let mut chars = raw.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some('\'') => out.push('\''),
                    Some('0') => out.push('\0'),
                    Some(other) => {
                        let span = Span {
                            file: self.file_id,
                            range,
                        };
                        self.diagnostics.push(Diagnostic::error(
                            format!("invalid escape sequence `\\{other}`"),
                            span,
                        ));
                        out.push('\\');
                        out.push(other);
                    }
                    None => {
                        out.push('\\');
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Decode a char literal interior (quotes already stripped).
    fn decode_char_escape(&mut self, raw: &str, range: TextRange) -> char {
        let mut chars = raw.chars();
        match chars.next() {
            Some('\\') => match chars.next() {
                Some('n') => '\n',
                Some('r') => '\r',
                Some('t') => '\t',
                Some('\\') => '\\',
                Some('"') => '"',
                Some('\'') => '\'',
                Some('0') => '\0',
                Some(other) => {
                    let span = Span {
                        file: self.file_id,
                        range,
                    };
                    self.diagnostics.push(Diagnostic::error(
                        format!("invalid escape sequence `\\{other}`"),
                        span,
                    ));
                    other
                }
                None => '\0',
            },
            Some(c) => c,
            None => '\0',
        }
    }

    // ── Expression lowering ────────────────────────────────────────

    fn lower_expr(&mut self, expr: &ExprCst) -> ExprIdx {
        let range = expr.syntax().text_range();
        let idx = self.lower_expr_inner(expr);
        self.expr_source_map.insert(idx, range);
        idx
    }

    fn lower_expr_inner(&mut self, expr: &ExprCst) -> ExprIdx {
        match expr {
            ExprCst::Literal(lit) => self.lower_literal(lit),
            ExprCst::Path(pe) => self.lower_path_expr(pe),
            ExprCst::Binary(be) => self.lower_binary(be),
            ExprCst::Unary(ue) => self.lower_unary(ue),
            ExprCst::Call(ce) => self.lower_call(ce),
            ExprCst::Field(fe) => self.lower_field(fe),
            ExprCst::Pipeline(pe) => self.lower_pipeline(pe),
            ExprCst::Propagate(pe) => self.lower_propagate(pe),
            ExprCst::Match(me) => self.lower_match(me),
            ExprCst::If(ie) => self.lower_if(ie),
            ExprCst::Block(be) => self.lower_block(be),
            ExprCst::Record(re) => self.lower_record(re),
            ExprCst::Return(re) => self.lower_return(re),
            ExprCst::Hole(_) => self.alloc_expr(Expr::Hole),
            ExprCst::Old(oe) => self.lower_old(oe),
            ExprCst::Paren(pe) => {
                // Paren expressions are transparent — lower the inner expr.
                pe.inner()
                    .map(|e| self.lower_expr(&e))
                    .unwrap_or_else(|| self.alloc_expr(Expr::Missing))
            }
            ExprCst::Lambda(le) => self.lower_lambda(le),
        }
    }

    fn lower_literal(&mut self, lit: &LiteralExpr) -> ExprIdx {
        let literal = lit
            .token()
            .map(|tok| match tok.kind() {
                SyntaxKind::IntLiteral => {
                    self.validate_numeric_underscores(tok.text(), tok.text_range());
                    let text = tok.text().replace('_', "");
                    match text.parse::<i64>() {
                        Ok(v) => Literal::Int(v),
                        Err(_) => {
                            let span = Span {
                                file: self.file_id,
                                range: tok.text_range(),
                            };
                            self.diagnostics.push(Diagnostic::error(
                                format!("integer literal `{}` is out of range", tok.text()),
                                span,
                            ));
                            Literal::Int(0)
                        }
                    }
                }
                SyntaxKind::FloatLiteral => {
                    self.validate_numeric_underscores(tok.text(), tok.text_range());
                    let text = tok.text().replace('_', "");
                    match text.parse::<f64>() {
                        Ok(v) => Literal::Float(v),
                        Err(_) => {
                            let span = Span {
                                file: self.file_id,
                                range: tok.text_range(),
                            };
                            self.diagnostics.push(Diagnostic::error(
                                format!("float literal `{}` is out of range", tok.text()),
                                span,
                            ));
                            Literal::Float(0.0)
                        }
                    }
                }
                SyntaxKind::StringLiteral => {
                    let text = tok.text();
                    let inner = &text[1..text.len().saturating_sub(1)];
                    Literal::String(self.decode_string_escapes(inner, tok.text_range()))
                }
                SyntaxKind::CharLiteral => {
                    let text = tok.text();
                    let inner = &text[1..text.len().saturating_sub(1)];
                    Literal::Char(self.decode_char_escape(inner, tok.text_range()))
                }
                SyntaxKind::TrueKw => Literal::Bool(true),
                SyntaxKind::FalseKw => Literal::Bool(false),
                _ => Literal::Bool(false),
            })
            .unwrap_or(Literal::Bool(false));
        self.alloc_expr(Expr::Literal(literal))
    }

    fn lower_path_expr(&mut self, pe: &PathExpr) -> ExprIdx {
        let path = pe
            .path()
            .map(|p| self.lower_path(&p))
            .unwrap_or_else(|| path::Path { segments: vec![] });

        // Check if single-segment path resolves
        if path.is_single()
            && let Some(name) = path.last()
            && self.resolve_name(name).is_none()
        {
            let span = self.node_span(pe.syntax());
            self.diagnostics.push(Diagnostic::error(
                format!("unresolved name `{}`", name.resolve(self.interner)),
                span,
            ));
        }

        self.alloc_expr(Expr::Path(path))
    }

    fn lower_binary(&mut self, be: &BinaryExpr) -> ExprIdx {
        let op = match be.op_token() {
            Some(tok) => match tok.kind() {
                SyntaxKind::Plus => BinaryOp::Add,
                SyntaxKind::Minus => BinaryOp::Sub,
                SyntaxKind::Star => BinaryOp::Mul,
                SyntaxKind::Slash => BinaryOp::Div,
                SyntaxKind::Percent => BinaryOp::Mod,
                SyntaxKind::EqEq => BinaryOp::Eq,
                SyntaxKind::BangEq => BinaryOp::NotEq,
                SyntaxKind::Lt => BinaryOp::Lt,
                SyntaxKind::Gt => BinaryOp::Gt,
                SyntaxKind::LtEq => BinaryOp::LtEq,
                SyntaxKind::GtEq => BinaryOp::GtEq,
                SyntaxKind::AmpAmp => BinaryOp::And,
                SyntaxKind::PipePipe => BinaryOp::Or,
                SyntaxKind::Amp => BinaryOp::BitAnd,
                SyntaxKind::Pipe => BinaryOp::BitOr,
                SyntaxKind::Caret => BinaryOp::BitXor,
                SyntaxKind::LtLt => BinaryOp::Shl,
                SyntaxKind::GtGt => BinaryOp::Shr,
                _ => {
                    self.diagnostics.push(Diagnostic::error(
                        format!("unsupported binary operator `{}`", tok.text()),
                        Span {
                            file: self.file_id,
                            range: tok.text_range(),
                        },
                    ));
                    return self.alloc_expr(Expr::Missing);
                }
            },
            None => {
                self.diagnostics.push(Diagnostic::error(
                    "missing binary operator".to_string(),
                    self.node_span(be.syntax()),
                ));
                return self.alloc_expr(Expr::Missing);
            }
        };

        let lhs = be
            .lhs()
            .map(|e| self.lower_expr(&e))
            .unwrap_or_else(|| self.alloc_expr(Expr::Missing));
        let rhs = be
            .rhs()
            .map(|e| self.lower_expr(&e))
            .unwrap_or_else(|| self.alloc_expr(Expr::Missing));

        self.alloc_expr(Expr::Binary { op, lhs, rhs })
    }

    fn lower_unary(&mut self, ue: &UnaryExpr) -> ExprIdx {
        let op = match ue.op_token() {
            Some(tok) => match tok.kind() {
                SyntaxKind::Bang => UnaryOp::Not,
                SyntaxKind::Minus => UnaryOp::Neg,
                SyntaxKind::Tilde => UnaryOp::BitNot,
                _ => {
                    self.diagnostics.push(Diagnostic::error(
                        format!("unsupported unary operator `{}`", tok.text()),
                        Span {
                            file: self.file_id,
                            range: tok.text_range(),
                        },
                    ));
                    return self.alloc_expr(Expr::Missing);
                }
            },
            None => {
                self.diagnostics.push(Diagnostic::error(
                    "missing unary operator".to_string(),
                    self.node_span(ue.syntax()),
                ));
                return self.alloc_expr(Expr::Missing);
            }
        };

        let operand = ue
            .operand()
            .map(|e| self.lower_expr(&e))
            .unwrap_or_else(|| self.alloc_expr(Expr::Missing));

        self.alloc_expr(Expr::Unary { op, operand })
    }

    fn lower_call(&mut self, ce: &CallExpr) -> ExprIdx {
        let callee = ce
            .callee()
            .map(|e| self.lower_expr(&e))
            .unwrap_or_else(|| self.alloc_expr(Expr::Missing));

        let args = ce
            .arg_list()
            .map(|al| self.lower_arg_list(&al))
            .unwrap_or_default();

        self.alloc_expr(Expr::Call { callee, args })
    }

    fn lower_arg_list(&mut self, al: &ArgList) -> Vec<CallArg> {
        let mut result = Vec::new();

        for child in al.syntax().children() {
            if let Some(named) = NamedArg::cast(child.clone()) {
                let name = named
                    .name_token()
                    .map(|tok| Name::new(self.interner, tok.text()))
                    .unwrap_or_else(|| Name::new(self.interner, "_"));
                let value = named
                    .value()
                    .map(|e| self.lower_expr(&e))
                    .unwrap_or_else(|| self.alloc_expr(Expr::Missing));
                result.push(CallArg::Named { name, value });
            } else if let Some(expr) = ExprCst::cast(child) {
                let idx = self.lower_expr(&expr);
                result.push(CallArg::Positional(idx));
            }
        }

        result
    }

    fn lower_field(&mut self, fe: &FieldExpr) -> ExprIdx {
        let base = fe
            .base()
            .map(|e| self.lower_expr(&e))
            .unwrap_or_else(|| self.alloc_expr(Expr::Missing));

        let field = fe
            .field_token()
            .map(|tok| Name::new(self.interner, tok.text()))
            .unwrap_or_else(|| Name::new(self.interner, "_"));

        self.alloc_expr(Expr::Field { base, field })
    }

    /// Pipeline desugaring: `x |> f(a, b)` → `Call { callee: f, args: [x, a, b] }`
    /// If RHS is not a call: `x |> f` → `Call { callee: f, args: [x] }`
    fn lower_pipeline(&mut self, pe: &PipelineExpr) -> ExprIdx {
        let lhs = pe
            .lhs()
            .map(|e| self.lower_expr(&e))
            .unwrap_or_else(|| self.alloc_expr(Expr::Missing));

        let rhs = pe.rhs();

        match rhs {
            Some(ExprCst::Call(ref call_expr)) => {
                // x |> f(a, b) → Call { callee: f, args: [x, a, b] }
                let callee = call_expr
                    .callee()
                    .map(|e| self.lower_expr(&e))
                    .unwrap_or_else(|| self.alloc_expr(Expr::Missing));

                let mut args = vec![CallArg::Positional(lhs)];
                if let Some(al) = call_expr.arg_list() {
                    args.extend(self.lower_arg_list(&al));
                }

                self.alloc_expr(Expr::Call { callee, args })
            }
            Some(ref rhs_expr) => {
                // x |> f → Call { callee: f, args: [x] }
                let callee = self.lower_expr(rhs_expr);
                let args = vec![CallArg::Positional(lhs)];
                self.alloc_expr(Expr::Call { callee, args })
            }
            None => self.alloc_expr(Expr::Missing),
        }
    }

    /// Propagation desugaring: `e?` →
    /// ```text
    /// Match { scrutinee: e, arms: [
    ///     MatchArm { pat: Constructor("Ok", [Bind("__v")]), body: Path("__v") },
    ///     MatchArm { pat: Constructor("Err", [Bind("__e")]), body: Return(Call("Err", [Path("__e")])) }
    /// ]}
    /// ```
    fn lower_propagate(&mut self, pe: &PropagateExpr) -> ExprIdx {
        let scrutinee = pe
            .inner()
            .map(|e| self.lower_expr(&e))
            .unwrap_or_else(|| self.alloc_expr(Expr::Missing));

        let ok_name = Name::new(self.interner, "Ok");
        let err_name = Name::new(self.interner, "Err");
        let v_name = Name::new(self.interner, "__v");
        let e_name = Name::new(self.interner, "__e");

        // Ok arm: Constructor("Ok", [Bind("__v")]) => Path("__v")
        let v_pat = self.alloc_pat(pat::Pat::Bind { name: v_name });
        let ok_pat = self.alloc_pat(pat::Pat::Constructor {
            path: path::Path::single(ok_name),
            args: vec![v_pat],
        });
        let ok_body = self.alloc_expr(Expr::Path(path::Path::single(v_name)));

        // Err arm: Constructor("Err", [Bind("__e")]) => Return(Call(Err, [Path("__e")]))
        let e_pat = self.alloc_pat(pat::Pat::Bind { name: e_name });
        let err_pat = self.alloc_pat(pat::Pat::Constructor {
            path: path::Path::single(err_name),
            args: vec![e_pat],
        });
        let e_path = self.alloc_expr(Expr::Path(path::Path::single(e_name)));
        let err_ctor = self.alloc_expr(Expr::Path(path::Path::single(err_name)));
        let err_call = self.alloc_expr(Expr::Call {
            callee: err_ctor,
            args: vec![CallArg::Positional(e_path)],
        });
        let err_body = self.alloc_expr(Expr::Return(Some(err_call)));

        let arms = vec![
            crate::expr::MatchArm {
                pat: ok_pat,
                body: ok_body,
            },
            crate::expr::MatchArm {
                pat: err_pat,
                body: err_body,
            },
        ];

        self.alloc_expr(Expr::Match { scrutinee, arms })
    }

    fn lower_match(&mut self, me: &MatchExpr) -> ExprIdx {
        let scrutinee = me
            .scrutinee()
            .map(|e| self.lower_expr(&e))
            .unwrap_or_else(|| self.alloc_expr(Expr::Missing));

        let arms = me
            .arm_list()
            .map(|al| {
                al.arms()
                    .map(|arm| {
                        self.push_scope();
                        let pat = arm
                            .pat()
                            .map(|p| self.lower_pat(&p, LocalBindingOrigin::MatchArmPattern))
                            .unwrap_or_else(|| self.alloc_pat(pat::Pat::Missing));
                        let body = arm
                            .body()
                            .map(|e| self.lower_expr(&e))
                            .unwrap_or_else(|| self.alloc_expr(Expr::Missing));
                        self.pop_scope();
                        crate::expr::MatchArm { pat, body }
                    })
                    .collect()
            })
            .unwrap_or_default();

        self.alloc_expr(Expr::Match { scrutinee, arms })
    }

    fn lower_if(&mut self, ie: &IfExpr) -> ExprIdx {
        let condition = ie
            .condition()
            .map(|e| self.lower_expr(&e))
            .unwrap_or_else(|| self.alloc_expr(Expr::Missing));

        let then_branch = ie
            .then_branch()
            .map(|b| self.lower_block(&b))
            .unwrap_or_else(|| self.alloc_expr(Expr::Missing));

        let else_branch = ie.else_branch().map(|eb| match eb {
            ElseBranch::Block(b) => self.lower_block(&b),
            ElseBranch::IfExpr(elif) => self.lower_if(&elif),
        });

        self.alloc_expr(Expr::If {
            condition,
            then_branch,
            else_branch,
        })
    }

    fn lower_block(&mut self, be: &BlockExpr) -> ExprIdx {
        self.push_scope();

        let mut stmts = Vec::new();
        let mut tail = None;

        let items: Vec<BlockItem> = be.stmts().collect();
        for (i, item) in items.iter().enumerate() {
            let is_last = i == items.len() - 1;
            match item {
                BlockItem::LetBinding(lb) => {
                    let pat = lb
                        .pat()
                        .map(|p| self.lower_pat(&p, LocalBindingOrigin::LetPattern))
                        .unwrap_or_else(|| self.alloc_pat(pat::Pat::Missing));
                    let ty = lb.type_expr().map(|te| self.lower_type_ref(&te));
                    let init = lb
                        .value()
                        .map(|e| self.lower_expr(&e))
                        .unwrap_or_else(|| self.alloc_expr(Expr::Missing));
                    stmts.push(Stmt::Let { pat, ty, init });
                }
                BlockItem::Expr(expr) => {
                    let idx = self.lower_expr(expr);
                    if is_last {
                        tail = Some(idx);
                    } else {
                        stmts.push(Stmt::Expr(idx));
                    }
                }
            }
        }

        self.pop_scope();
        self.alloc_expr(Expr::Block { stmts, tail })
    }

    fn lower_record(&mut self, re: &RecordExpr) -> ExprIdx {
        let path = re.path().map(|p| self.lower_path(&p));

        // Check for duplicate field names.
        if let Some(fl) = re.field_list() {
            let mut seen = std::collections::HashSet::new();
            for f in fl.fields() {
                if let Some(tok) = f.name_token() {
                    let name = tok.text();
                    if !seen.insert(name.to_string()) {
                        let span = self.node_span(f.syntax());
                        self.diagnostics.push(Diagnostic::error(
                            format!("duplicate field `{name}` in record literal"),
                            span,
                        ));
                    }
                }
            }
        }

        let fields = re
            .field_list()
            .map(|fl| {
                fl.fields()
                    .map(|f| {
                        let name = f
                            .name_token()
                            .map(|tok| Name::new(self.interner, tok.text()))
                            .unwrap_or_else(|| Name::new(self.interner, "_"));
                        let value = f
                            .value()
                            .map(|e| self.lower_expr(&e))
                            .unwrap_or_else(|| self.alloc_expr(Expr::Missing));
                        (name, value)
                    })
                    .collect()
            })
            .unwrap_or_default();

        self.alloc_expr(Expr::RecordLit { path, fields })
    }

    fn lower_return(&mut self, re: &ReturnExpr) -> ExprIdx {
        let value = re.value().map(|e| self.lower_expr(&e));
        self.alloc_expr(Expr::Return(value))
    }

    fn lower_old(&mut self, oe: &OldExpr) -> ExprIdx {
        if !self.in_contract {
            let span = self.node_span(oe.syntax());
            self.diagnostics.push(Diagnostic::error(
                "`old(...)` can only be used inside a contract clause (requires/ensures/invariant)",
                span,
            ));
        }
        let inner = oe
            .inner()
            .map(|e| self.lower_expr(&e))
            .unwrap_or_else(|| self.alloc_expr(Expr::Missing));
        self.alloc_expr(Expr::Old(inner))
    }

    fn lower_lambda(&mut self, le: &LambdaExpr) -> ExprIdx {
        self.push_scope();

        // Check for duplicate parameter names.
        if let Some(pl) = le.param_list() {
            let mut seen = std::collections::HashSet::new();
            for p in pl.params() {
                if let Some(tok) = p.name_token() {
                    let name = tok.text();
                    if !seen.insert(name.to_string()) {
                        let span = self.node_span(p.syntax());
                        self.diagnostics.push(Diagnostic::error(
                            format!("duplicate parameter `{name}`"),
                            span,
                        ));
                    }
                }
            }
        }

        let params: Vec<(PatIdx, Option<TypeRef>)> = le
            .param_list()
            .map(|pl| {
                pl.params()
                    .map(|p| {
                        let name = p
                            .name_token()
                            .map(|tok| Name::new(self.interner, tok.text()))
                            .unwrap_or_else(|| Name::new(self.interner, "_"));
                        let pat_idx = self.alloc_pat(pat::Pat::Bind { name });
                        self.pat_source_map.insert(pat_idx, p.syntax().text_range());

                        // Register in scope
                        self.register_local_binding(
                            name,
                            pat_idx,
                            p.syntax().text_range(),
                            LocalBindingOrigin::LambdaParam,
                        );

                        let ty = p.type_expr().map(|te| self.lower_type_ref(&te));
                        (pat_idx, ty)
                    })
                    .collect()
            })
            .unwrap_or_default();

        let body = le
            .body()
            .map(|e| self.lower_expr(&e))
            .unwrap_or_else(|| self.alloc_expr(Expr::Missing));

        self.pop_scope();
        self.alloc_expr(Expr::Lambda { params, body })
    }

    // ── Pattern lowering ───────────────────────────────────────────

    fn lower_pat(&mut self, pat_cst: &PatCst, origin: LocalBindingOrigin) -> PatIdx {
        let mut binders = std::collections::HashSet::new();
        self.lower_pat_with_binders(pat_cst, origin, &mut binders)
    }

    fn lower_pat_with_binders(
        &mut self,
        pat_cst: &PatCst,
        origin: LocalBindingOrigin,
        binders: &mut std::collections::HashSet<Name>,
    ) -> PatIdx {
        match pat_cst {
            PatCst::Ident(ip) => {
                let name = ip
                    .path()
                    .and_then(|p| p.segments().next())
                    .map(|tok| Name::new(self.interner, tok.text()))
                    .unwrap_or_else(|| Name::new(self.interner, "_"));

                // Check if this is a known constructor (capitalized) or a binding
                let is_constructor = name
                    .resolve(self.interner)
                    .starts_with(|c: char| c.is_uppercase());

                if is_constructor && self.module_scope.constructors.contains_key(&name) {
                    // Nullary constructor pattern
                    let pat_idx = self.alloc_pat(pat::Pat::Constructor {
                        path: path::Path::single(name),
                        args: vec![],
                    });
                    self.pat_source_map
                        .insert(pat_idx, ip.syntax().text_range());
                    pat_idx
                } else {
                    if is_constructor {
                        let span = self.node_span(ip.syntax());
                        self.diagnostics.push(Diagnostic::error(
                            format!(
                                "unknown constructor `{}` used as pattern (treating as binding)",
                                name.resolve(self.interner)
                            ),
                            span,
                        ));
                    }
                    // Binding pattern — introduces name into scope
                    let pat_idx = self.alloc_pat(pat::Pat::Bind { name });
                    self.pat_source_map
                        .insert(pat_idx, ip.syntax().text_range());
                    if !binders.insert(name) {
                        let span = self.node_span(ip.syntax());
                        self.diagnostics.push(Diagnostic::error(
                            format!(
                                "duplicate binding `{}` in pattern",
                                name.resolve(self.interner)
                            ),
                            span,
                        ));
                    }
                    self.register_local_binding(name, pat_idx, ip.syntax().text_range(), origin);
                    pat_idx
                }
            }
            PatCst::Constructor(cp) => {
                let path = cp
                    .path()
                    .map(|p| self.lower_path(&p))
                    .unwrap_or_else(|| path::Path { segments: vec![] });

                // No push_scope/pop_scope — sub-pattern bindings must stay in the
                // current (arm) scope so the arm body can resolve them.
                let args: Vec<PatIdx> = cp
                    .args()
                    .map(|a| self.lower_pat_with_binders(&a, origin, binders))
                    .collect();

                let pat_idx = self.alloc_pat(pat::Pat::Constructor { path, args });
                self.pat_source_map
                    .insert(pat_idx, cp.syntax().text_range());
                pat_idx
            }
            PatCst::Wildcard(wc) => {
                let pat_idx = self.alloc_pat(pat::Pat::Wildcard);
                self.pat_source_map
                    .insert(pat_idx, wc.syntax().text_range());
                pat_idx
            }
            PatCst::Literal(lp) => {
                let literal = lp
                    .token()
                    .map(|tok| match tok.kind() {
                        SyntaxKind::IntLiteral => {
                            self.validate_numeric_underscores(tok.text(), tok.text_range());
                            let text = tok.text().replace('_', "");
                            match text.parse::<i64>() {
                                Ok(v) => Literal::Int(v),
                                Err(_) => {
                                    let span = Span {
                                        file: self.file_id,
                                        range: tok.text_range(),
                                    };
                                    self.diagnostics.push(Diagnostic::error(
                                        format!("integer literal `{}` is out of range", tok.text()),
                                        span,
                                    ));
                                    Literal::Int(0)
                                }
                            }
                        }
                        SyntaxKind::FloatLiteral => {
                            self.validate_numeric_underscores(tok.text(), tok.text_range());
                            let text = tok.text().replace('_', "");
                            match text.parse::<f64>() {
                                Ok(v) => Literal::Float(v),
                                Err(_) => {
                                    let span = Span {
                                        file: self.file_id,
                                        range: tok.text_range(),
                                    };
                                    self.diagnostics.push(Diagnostic::error(
                                        format!("float literal `{}` is out of range", tok.text()),
                                        span,
                                    ));
                                    Literal::Float(0.0)
                                }
                            }
                        }
                        SyntaxKind::StringLiteral => {
                            let text = tok.text();
                            let inner = &text[1..text.len().saturating_sub(1)];
                            Literal::String(self.decode_string_escapes(inner, tok.text_range()))
                        }
                        SyntaxKind::CharLiteral => {
                            let text = tok.text();
                            let inner = &text[1..text.len().saturating_sub(1)];
                            Literal::Char(self.decode_char_escape(inner, tok.text_range()))
                        }
                        SyntaxKind::TrueKw => Literal::Bool(true),
                        SyntaxKind::FalseKw => Literal::Bool(false),
                        _ => Literal::Bool(false),
                    })
                    .unwrap_or(Literal::Bool(false));
                let pat_idx = self.alloc_pat(pat::Pat::Literal(literal));
                self.pat_source_map
                    .insert(pat_idx, lp.syntax().text_range());
                pat_idx
            }
            PatCst::Record(rp) => {
                let path = rp.path().map(|p| self.lower_path(&p));
                // Check for duplicate field names.
                {
                    let mut seen = std::collections::HashSet::new();
                    for tok in rp.field_names() {
                        let name = tok.text();
                        if !seen.insert(name.to_string()) {
                            let span = Span {
                                file: self.file_id,
                                range: tok.text_range(),
                            };
                            self.diagnostics.push(Diagnostic::error(
                                format!("duplicate field `{name}` in record pattern"),
                                span,
                            ));
                        }
                    }
                }
                let fields: Vec<Name> = rp
                    .field_names()
                    .map(|tok| Name::new(self.interner, tok.text()))
                    .collect();
                // Record pattern fields also introduce bindings
                for tok in rp.field_names() {
                    let field_name = Name::new(self.interner, tok.text());
                    let pat_idx = self.alloc_pat(pat::Pat::Bind { name: field_name });
                    self.pat_source_map.insert(pat_idx, tok.text_range());
                    self.register_local_binding(field_name, pat_idx, tok.text_range(), origin);
                }
                let pat_idx = self.alloc_pat(pat::Pat::Record { path, fields });
                self.pat_source_map
                    .insert(pat_idx, rp.syntax().text_range());
                pat_idx
            }
        }
    }

    // ── Helpers ────────────────────────────────────────────────────

    fn lower_path(&mut self, path: &kyokara_syntax::ast::nodes::Path) -> path::Path {
        let segments = path
            .segments()
            .map(|tok| Name::new(self.interner, tok.text()))
            .collect();
        path::Path { segments }
    }

    fn lower_type_ref(&mut self, ty: &TypeExpr) -> TypeRef {
        match ty {
            TypeExpr::NameType(nt) => {
                let path = nt
                    .path()
                    .map(|p| self.lower_path(&p))
                    .unwrap_or_else(|| path::Path { segments: vec![] });
                let args = nt
                    .type_arg_list()
                    .map(|tal| tal.type_args().map(|a| self.lower_type_ref(&a)).collect())
                    .unwrap_or_default();
                TypeRef::Path { path, args }
            }
            TypeExpr::FnType(ft) => {
                let all_types: Vec<TypeRef> =
                    ft.param_types().map(|t| self.lower_type_ref(&t)).collect();
                if all_types.is_empty() {
                    TypeRef::Fn {
                        params: vec![],
                        ret: Box::new(TypeRef::Error),
                    }
                } else {
                    let (params, ret) = all_types.split_at(all_types.len() - 1);
                    TypeRef::Fn {
                        params: params.to_vec(),
                        ret: Box::new(ret[0].clone()),
                    }
                }
            }
            TypeExpr::RecordType(rt) => {
                let fields = rt
                    .fields()
                    .map(|f| {
                        let fname = f
                            .name_token()
                            .map(|tok| Name::new(self.interner, tok.text()))
                            .unwrap_or_else(|| Name::new(self.interner, "_"));
                        let fty = f
                            .type_expr()
                            .map(|te| self.lower_type_ref(&te))
                            .unwrap_or(TypeRef::Error);
                        (fname, fty)
                    })
                    .collect();
                TypeRef::Record { fields }
            }
            TypeExpr::RefinedType(rt) => {
                let name = rt
                    .name_token()
                    .map(|tok| Name::new(self.interner, tok.text()))
                    .unwrap_or_else(|| Name::new(self.interner, "_"));
                let base = rt
                    .base_type()
                    .map(|t| self.lower_type_ref(&t))
                    .unwrap_or(TypeRef::Error);
                let predicate = rt
                    .predicate()
                    .map(|e| self.lower_expr(&e))
                    .unwrap_or_else(|| self.alloc_expr(Expr::Missing));
                TypeRef::Refined {
                    name,
                    base: Box::new(base),
                    predicate,
                }
            }
        }
    }

    fn resolve_name(&self, name: Name) -> Option<crate::resolver::ResolvedName> {
        let resolver =
            crate::resolver::Resolver::new(self.module_scope, &self.scopes, self.current_scope);
        resolver.resolve_name(name)
    }
}

// Type aliases for CST types to avoid confusion with HIR types.
type ExprCst = nodes::Expr;
type PatCst = nodes::Pat;
