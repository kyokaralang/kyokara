//! Pass 1: CST → ItemTree collection.
//!
//! Walks top-level CST items to build `ItemTree` + `ModuleScope`.

use kyokara_diagnostics::Diagnostic;
use kyokara_intern::Interner;
use kyokara_span::{FileId, Span};
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::*;
use kyokara_syntax::ast::traits::{HasName, HasTypeParams, HasVisibility};

use crate::item_tree::*;
use crate::name::Name;
use crate::path::Path;
use crate::resolver::ModuleScope;
use crate::type_ref::TypeRef;

/// Result of item tree collection.
pub struct ItemTreeResult {
    pub tree: ItemTree,
    pub module_scope: ModuleScope,
    pub diagnostics: Vec<Diagnostic>,
}

/// Collect all top-level items from a parsed source file.
pub fn collect_item_tree(
    file: &SourceFile,
    file_id: FileId,
    interner: &mut Interner,
) -> ItemTreeResult {
    let mut ctx = ItemTreeCtx {
        tree: ItemTree::default(),
        module_scope: ModuleScope::default(),
        diagnostics: Vec::new(),
        file_id,
        interner,
    };

    // Module declaration
    if let Some(module_decl) = file.module_decl()
        && let Some(path) = module_decl.path()
    {
        ctx.tree.module_name = Some(ctx.lower_path(&path));
    }

    // Imports
    for import in file.imports() {
        ctx.lower_import(&import);
    }

    // Items
    for item in file.items() {
        ctx.lower_item(item);
    }

    ItemTreeResult {
        tree: ctx.tree,
        module_scope: ctx.module_scope,
        diagnostics: ctx.diagnostics,
    }
}

struct ItemTreeCtx<'a> {
    tree: ItemTree,
    module_scope: ModuleScope,
    diagnostics: Vec<Diagnostic>,
    file_id: FileId,
    interner: &'a mut Interner,
}

impl ItemTreeCtx<'_> {
    fn lower_import(&mut self, import: &ImportDecl) {
        let path = import
            .path()
            .map(|p| self.lower_path(&p))
            .unwrap_or_else(|| Path { segments: vec![] });

        let alias = import
            .alias()
            .and_then(|a| a.name_token())
            .map(|tok| Name::new(self.interner, tok.text()));

        let import_idx = self.tree.imports.len();

        // The local name is the alias or the last path segment.
        let local_name = alias.or_else(|| path.last());
        if let Some(name) = local_name {
            if let std::collections::hash_map::Entry::Vacant(e) =
                self.module_scope.imports.entry(name)
            {
                e.insert(import_idx);
            } else {
                let span = self.node_span(import.syntax());
                self.diagnostics.push(Diagnostic::error(
                    format!("duplicate import `{}`", name.resolve(self.interner)),
                    span,
                ));
            }
        }

        self.tree.imports.push(Import { path, alias });
    }

    fn lower_item(&mut self, item: Item) {
        match item {
            Item::FnDef(f) => {
                self.lower_fn_def(&f, false);
            }
            Item::TypeDef(t) => self.lower_type_def(&t),
            Item::CapDef(c) => self.lower_cap_def(&c),
            Item::PropertyDef(p) => self.lower_property_def(&p),
            Item::LetBinding(l) => self.lower_let_binding(&l),
        }
    }

    fn lower_fn_def(&mut self, f: &FnDef, inside_cap: bool) -> FnItemIdx {
        let name = self.name_of(f);
        let type_params = self.collect_type_params(f);

        let params = f
            .param_list()
            .map(|pl| {
                pl.params()
                    .map(|p| {
                        let pname = p
                            .name_token()
                            .map(|tok| Name::new(self.interner, tok.text()))
                            .unwrap_or_else(|| Name::new(self.interner, "_"));
                        let ty = p
                            .type_expr()
                            .map(|t| self.lower_type_ref(&t))
                            .unwrap_or(TypeRef::Error);
                        FnParam { name: pname, ty }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Check for duplicate parameter names.
        if let Some(pl) = f.param_list() {
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

        let ret_type = f
            .return_type()
            .and_then(|rt| rt.type_expr())
            .map(|t| self.lower_type_ref(&t));

        let with_caps = f
            .with_clause()
            .map(|wc| wc.types().map(|t| self.lower_type_ref(&t)).collect())
            .unwrap_or_default();

        let pipe_caps = f
            .pipe_clause()
            .map(|pc| pc.types().map(|t| self.lower_type_ref(&t)).collect())
            .unwrap_or_default();

        let has_body = f.body().is_some();
        let is_pub = f.is_pub();

        let idx = self.tree.functions.alloc(FnItem {
            name,
            is_pub,
            type_params,
            params,
            ret_type,
            with_caps,
            pipe_caps,
            has_body,
        });

        if !inside_cap {
            self.register_fn(name, idx, f.syntax());
        }

        idx
    }

    fn lower_type_def(&mut self, t: &TypeDef) {
        let name = self.name_of(t);
        let type_params = self.collect_type_params(t);

        let kind = if let Some(vl) = t.variant_list() {
            let mut variants = Vec::new();
            for (vi, variant) in vl.variants().enumerate() {
                let vname = variant
                    .name_token()
                    .map(|tok| Name::new(self.interner, tok.text()))
                    .unwrap_or_else(|| Name::new(self.interner, "_"));
                let fields = variant
                    .field_list()
                    .map(|fl| {
                        fl.fields()
                            .filter_map(|f| f.type_expr())
                            .map(|t| self.lower_type_ref(&t))
                            .collect()
                    })
                    .unwrap_or_default();
                variants.push(VariantDef {
                    name: vname,
                    fields,
                });
                // We'll register constructors after alloc.
                // Store (vname, vi) temporarily.
                let _ = (vname, vi);
            }

            TypeDefKind::Adt { variants }
        } else if let Some(rfl) = t.record_field_list() {
            self.check_duplicate_record_fields(rfl.fields());
            let fields = rfl
                .fields()
                .map(|f| {
                    let fname = f
                        .name_token()
                        .map(|tok| Name::new(self.interner, tok.text()))
                        .unwrap_or_else(|| Name::new(self.interner, "_"));
                    let ty = f
                        .type_expr()
                        .map(|te| self.lower_type_ref(&te))
                        .unwrap_or(TypeRef::Error);
                    (fname, ty)
                })
                .collect();
            TypeDefKind::Record { fields }
        } else if let Some(te) = t.type_expr() {
            // The type_expr on TypeDef is the alias body.
            // But we need to check: if it's a RecordType node,
            // it was parsed as a type expr, not a RecordFieldList child.
            TypeDefKind::Alias(self.lower_type_ref(&te))
        } else {
            TypeDefKind::Alias(TypeRef::Error)
        };

        let is_pub = t.is_pub();
        let idx = self.tree.types.alloc(TypeItem {
            name,
            is_pub,
            type_params,
            kind: kind.clone(),
        });

        // Register type in module scope
        if let std::collections::hash_map::Entry::Vacant(e) = self.module_scope.types.entry(name) {
            e.insert(idx);
        } else {
            let span = self.node_span(t.syntax());
            self.diagnostics.push(Diagnostic::error(
                format!("duplicate type `{}`", name.resolve(self.interner)),
                span,
            ));
        }

        // Register constructors for ADTs
        if let TypeDefKind::Adt { ref variants } = kind {
            for (vi, variant) in variants.iter().enumerate() {
                if let std::collections::hash_map::Entry::Vacant(e) =
                    self.module_scope.constructors.entry(variant.name)
                {
                    e.insert((idx, vi));
                } else {
                    let span = self.node_span(t.syntax());
                    self.diagnostics.push(Diagnostic::error(
                        format!(
                            "duplicate constructor `{}`",
                            variant.name.resolve(self.interner)
                        ),
                        span,
                    ));
                }
            }
        }
    }

    fn lower_cap_def(&mut self, c: &CapDef) {
        let name = self.name_of(c);
        let type_params = self.collect_type_params(c);

        let functions: Vec<FnItemIdx> =
            c.functions().map(|f| self.lower_fn_def(&f, true)).collect();

        let is_pub = c.is_pub();
        let idx = self.tree.caps.alloc(CapItem {
            name,
            is_pub,
            type_params,
            functions,
        });

        if let std::collections::hash_map::Entry::Vacant(e) = self.module_scope.caps.entry(name) {
            e.insert(idx);
        } else {
            let span = self.node_span(c.syntax());
            self.diagnostics.push(Diagnostic::error(
                format!("duplicate capability `{}`", name.resolve(self.interner)),
                span,
            ));
        }
    }

    fn lower_property_def(&mut self, p: &PropertyDef) {
        let name = self.name_of(p);

        let params = p
            .param_list()
            .map(|pl| {
                pl.params()
                    .map(|param| {
                        let pname = param
                            .name_token()
                            .map(|tok| Name::new(self.interner, tok.text()))
                            .unwrap_or_else(|| Name::new(self.interner, "_"));
                        let ty = param
                            .type_expr()
                            .map(|t| self.lower_type_ref(&t))
                            .unwrap_or(TypeRef::Error);
                        FnParam { name: pname, ty }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let has_body = p.body().is_some();

        self.tree.properties.alloc(PropertyItem {
            name,
            params,
            has_body,
        });
    }

    fn lower_let_binding(&mut self, l: &LetBinding) {
        // For top-level lets, extract the name from the pattern.
        let name = l
            .pat()
            .and_then(|p| match p {
                Pat::Ident(ip) => ip
                    .path()
                    .and_then(|path| path.segments().next())
                    .map(|tok| Name::new(self.interner, tok.text())),
                _ => None,
            })
            .unwrap_or_else(|| Name::new(self.interner, "_"));

        let ty = l.type_expr().map(|te| self.lower_type_ref(&te));

        self.tree.lets.alloc(LetItem { name, ty });
    }

    // ── Helpers ────────────────────────────────────────────────────

    fn name_of(&mut self, node: &impl HasName) -> Name {
        node.name_token()
            .map(|tok| Name::new(self.interner, tok.text()))
            .unwrap_or_else(|| Name::new(self.interner, "_"))
    }

    fn collect_type_params(&mut self, node: &impl HasTypeParams) -> Vec<Name> {
        node.type_param_list()
            .map(|tpl| {
                tpl.type_params()
                    .filter_map(|tp| tp.name_token())
                    .map(|tok| Name::new(self.interner, tok.text()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn register_fn(&mut self, name: Name, idx: FnItemIdx, syntax: &kyokara_syntax::SyntaxNode) {
        if let std::collections::hash_map::Entry::Vacant(e) =
            self.module_scope.functions.entry(name)
        {
            e.insert(idx);
        } else {
            let span = self.node_span(syntax);
            self.diagnostics.push(Diagnostic::error(
                format!("duplicate function `{}`", name.resolve(self.interner)),
                span,
            ));
        }
    }

    fn node_span(&self, node: &kyokara_syntax::SyntaxNode) -> Span {
        Span {
            file: self.file_id,
            range: node.text_range(),
        }
    }

    /// Emit diagnostics for duplicate field names in a record field list.
    fn check_duplicate_record_fields<I>(&mut self, fields: I)
    where
        I: Iterator<Item = RecordField>,
    {
        let mut seen = std::collections::HashSet::new();
        for f in fields {
            if let Some(tok) = f.name_token() {
                let name = tok.text();
                if !seen.insert(name.to_string()) {
                    let span = self.node_span(f.syntax());
                    self.diagnostics.push(Diagnostic::error(
                        format!("duplicate field `{name}` in record type"),
                        span,
                    ));
                }
            }
        }
    }

    /// Lower a CST type expression to a `TypeRef`.
    fn lower_type_ref(&mut self, ty: &TypeExpr) -> TypeRef {
        match ty {
            TypeExpr::NameType(nt) => {
                let path = nt
                    .path()
                    .map(|p| self.lower_path(&p))
                    .unwrap_or_else(|| Path { segments: vec![] });
                let args = nt
                    .type_arg_list()
                    .map(|tal| tal.type_args().map(|a| self.lower_type_ref(&a)).collect())
                    .unwrap_or_default();
                TypeRef::Path { path, args }
            }
            TypeExpr::FnType(ft) => {
                let all_types: Vec<TypeRef> =
                    ft.param_types().map(|t| self.lower_type_ref(&t)).collect();
                // Last type is the return type (fn(A, B) -> C parsed as 3 child types).
                // Actually, the FnType node structure: the params are inside parens,
                // then `-> RetType`. All child type exprs include params + return.
                // We need to split: all but last are params, last is return.
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
                self.check_duplicate_record_fields(rt.fields());
                let fields = rt
                    .fields()
                    .map(|f| {
                        let fname = f
                            .name_token()
                            .map(|tok| Name::new(self.interner, tok.text()))
                            .unwrap_or_else(|| Name::new(self.interner, "_"));
                        let ty = f
                            .type_expr()
                            .map(|te| self.lower_type_ref(&te))
                            .unwrap_or(TypeRef::Error);
                        (fname, ty)
                    })
                    .collect();
                TypeRef::Record { fields }
            }
            TypeExpr::RefinedType(_rt) => {
                // Refined types reference expressions, which require body lowering.
                // For the item tree (signatures only), we represent them as Error
                // and handle them properly during body lowering.
                // TODO: properly lower refined types when body lowering is available.
                TypeRef::Error
            }
        }
    }

    fn lower_path(&mut self, path: &kyokara_syntax::ast::nodes::Path) -> Path {
        let segments = path
            .segments()
            .map(|tok| Name::new(self.interner, tok.text()))
            .collect();
        Path { segments }
    }
}
