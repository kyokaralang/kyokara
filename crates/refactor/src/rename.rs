//! Rename symbol refactor.
//!
//! CST-based: walks the rowan syntax tree to find all `Ident` tokens
//! matching the old name, filters by syntactic context (parent node kind)
//! to determine if the token refers to the target symbol kind, and
//! produces text edits.

use kyokara_hir::{CheckResult, ProjectCheckResult};
use kyokara_intern::Interner;
use kyokara_parser::SyntaxKind;
use kyokara_span::FileId;
use kyokara_syntax::{SyntaxNode, SyntaxToken};
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::ImportMember;

use crate::{RefactorError, RefactorResult, SymbolKind, TextEdit};

/// Rename a symbol in a single-file check result.
pub fn rename_symbol(
    result: &CheckResult,
    file_id: FileId,
    old_name: &str,
    new_name: &str,
    kind: SymbolKind,
) -> Result<RefactorResult, RefactorError> {
    // 1. Validate the old name exists in the module scope.
    validate_old_name_exists(&result.interner, &result.module_scope, old_name, kind)?;

    // 2. Check the new name doesn't conflict.
    validate_new_name(&result.interner, &result.module_scope, new_name, kind)?;

    // 3. Check the new name is not a keyword.
    if SyntaxKind::is_reserved_keyword_text(new_name) {
        return Err(RefactorError::NewNameIsKeyword {
            name: new_name.to_string(),
        });
    }

    // 4. Build CST root and collect edits.
    let root = SyntaxNode::new_root(result.green.clone());
    let edits = collect_rename_edits(&root, file_id, old_name, new_name, kind);

    Ok(RefactorResult {
        description: format!("rename {kind:?} `{old_name}` → `{new_name}`"),
        edits,
        verified: false,
    })
}

/// Rename a symbol across all modules in a multi-file project.
///
/// When `target_file` is `Some`, only the symbol defined in that file
/// (and its cross-module usages) are renamed. When `None`, the rename
/// succeeds only if exactly one module defines the symbol; otherwise
/// an `AmbiguousRename` error is returned.
pub fn rename_symbol_project(
    result: &ProjectCheckResult,
    old_name: &str,
    new_name: &str,
    kind: SymbolKind,
    target_file: Option<&str>,
) -> Result<RefactorResult, RefactorError> {
    // 1. Find which modules locally define this symbol (have a definition
    //    site in their source CST, not just an imported name in scope).
    let defining_modules: Vec<(&kyokara_hir::ModulePath, &kyokara_hir::ModuleInfo)> = result
        .module_graph
        .iter()
        .filter(|(_, info)| {
            let parse = kyokara_syntax::parse(&info.source);
            let root = SyntaxNode::new_root(parse.green);
            is_locally_defined(&root, old_name, kind)
        })
        .collect();

    if defining_modules.is_empty() {
        return Err(RefactorError::SymbolNotFound {
            name: old_name.to_string(),
            kind,
        });
    }

    // 2. Disambiguate: pick the defining module.
    let (def_mod_path, _def_info) = if let Some(tf) = target_file {
        defining_modules
            .iter()
            .find(|(_, info)| info.path.display().to_string() == tf)
            .copied()
            .ok_or_else(|| RefactorError::SymbolNotFound {
                name: old_name.to_string(),
                kind,
            })?
    } else if defining_modules.len() == 1 {
        defining_modules[0]
    } else {
        let files: Vec<String> = defining_modules
            .iter()
            .map(|(_, info)| info.path.display().to_string())
            .collect();
        return Err(RefactorError::AmbiguousRename {
            name: old_name.to_string(),
            kind,
            files,
        });
    };

    // 3. Check the new name is not a keyword.
    if SyntaxKind::is_reserved_keyword_text(new_name) {
        return Err(RefactorError::NewNameIsKeyword {
            name: new_name.to_string(),
        });
    }

    // 4. Determine which modules should be edited:
    //    - The defining module itself
    //    - Modules that import from the defining module
    let importing_modules: Vec<&kyokara_hir::ModulePath> = result
        .module_graph
        .iter()
        .filter(|(mod_path, info)| {
            if *mod_path == def_mod_path {
                return false;
            }
            imports_from_module(&info.item_tree, def_mod_path)
        })
        .map(|(mod_path, _)| mod_path)
        .collect();

    // 5. Check for conflicts only in affected modules.
    let def_info = result
        .module_graph
        .get(def_mod_path)
        .ok_or(RefactorError::InternalError {
            message: "defining module not found in module graph".into(),
        })?;
    validate_new_name(&result.interner, &def_info.scope, new_name, kind)?;
    for imp_mod in &importing_modules {
        let info = result
            .module_graph
            .get(imp_mod)
            .ok_or(RefactorError::InternalError {
                message: "importing module not found in module graph".into(),
            })?;
        validate_new_name(&result.interner, &info.scope, new_name, kind)?;
    }

    // 6. Collect edits only in affected modules.
    let mut all_edits = Vec::new();

    // Defining module: rename definition + usages.
    {
        let parse = kyokara_syntax::parse(&def_info.source);
        let root = SyntaxNode::new_root(parse.green);
        let edits = collect_rename_edits(&root, def_info.file_id, old_name, new_name, kind);
        all_edits.extend(edits);
    }

    // Importing modules: rename usages only.
    for imp_mod in &importing_modules {
        let info = result
            .module_graph
            .get(imp_mod)
            .ok_or(RefactorError::InternalError {
                message: "importing module not found in module graph".into(),
            })?;
        let parse = kyokara_syntax::parse(&info.source);
        let root = SyntaxNode::new_root(parse.green);
        let edits = collect_rename_edits(&root, info.file_id, old_name, new_name, kind);
        all_edits.extend(edits);
    }

    Ok(RefactorResult {
        description: format!("rename {kind:?} `{old_name}` → `{new_name}` (project-wide)"),
        edits: all_edits,
        verified: false,
    })
}

/// Check if a module's CST contains a local definition of the given symbol.
fn is_locally_defined(root: &SyntaxNode, name: &str, kind: SymbolKind) -> bool {
    for element in root.descendants_with_tokens() {
        let Some(token) = element.into_token() else {
            continue;
        };
        if !token.kind().is_identifier_token() || token.text() != name {
            continue;
        }
        let Some(parent) = token.parent() else {
            continue;
        };
        let pk = parent.kind();
        let is_def = match kind {
            SymbolKind::Function => pk == SyntaxKind::FnDef,
            SymbolKind::Type => pk == SyntaxKind::TypeDef,
            SymbolKind::Capability => pk == SyntaxKind::EffectDef,
            SymbolKind::Variant => pk == SyntaxKind::Variant,
        };
        if is_def {
            return true;
        }
    }
    false
}

/// Check if a module imports from another module by comparing import paths
/// against the target module's path.
fn imports_from_module(
    item_tree: &kyokara_hir::ItemTree,
    target_mod_path: &kyokara_hir::ModulePath,
) -> bool {
    if target_mod_path.is_root() {
        return false; // Root module can't be imported
    }
    item_tree.imports.iter().any(|imp| {
        if imp.path.segments.is_empty() {
            return false;
        }
        if imp.path.segments.len() > 1 {
            imp.path.segments == target_mod_path.0
        } else {
            target_mod_path.last() == imp.path.last()
        }
    })
}

// ── Validation helpers ──────────────────────────────────────────────

fn validate_old_name_exists(
    interner: &Interner,
    scope: &kyokara_hir::ModuleScope,
    old_name: &str,
    kind: SymbolKind,
) -> Result<(), RefactorError> {
    if !name_exists_in_scope(interner, scope, old_name, kind) {
        return Err(RefactorError::SymbolNotFound {
            name: old_name.to_string(),
            kind,
        });
    }
    Ok(())
}

fn name_exists_in_scope(
    interner: &Interner,
    scope: &kyokara_hir::ModuleScope,
    name: &str,
    kind: SymbolKind,
) -> bool {
    match kind {
        SymbolKind::Function => scope.functions.keys().any(|n| n.resolve(interner) == name),
        SymbolKind::Type => scope.types.keys().any(|n| n.resolve(interner) == name),
        SymbolKind::Capability => scope.effects.keys().any(|n| n.resolve(interner) == name),
        SymbolKind::Variant => {
            scope.constructors.keys().any(|n| n.resolve(interner) == name)
                || scope
                    .type_variants
                    .keys()
                    .any(|(_, variant_name)| variant_name.resolve(interner) == name)
        }
    }
}

fn validate_new_name(
    interner: &Interner,
    scope: &kyokara_hir::ModuleScope,
    new_name: &str,
    _kind: SymbolKind,
) -> Result<(), RefactorError> {
    // Check if the new name conflicts with any existing symbol.
    if scope
        .functions
        .keys()
        .any(|n| n.resolve(interner) == new_name)
    {
        return Err(RefactorError::NameConflict {
            name: new_name.to_string(),
            existing_kind: SymbolKind::Function,
        });
    }
    if scope.types.keys().any(|n| n.resolve(interner) == new_name) {
        return Err(RefactorError::NameConflict {
            name: new_name.to_string(),
            existing_kind: SymbolKind::Type,
        });
    }
    if scope
        .effects
        .keys()
        .any(|n| n.resolve(interner) == new_name)
    {
        return Err(RefactorError::NameConflict {
            name: new_name.to_string(),
            existing_kind: SymbolKind::Capability,
        });
    }
    if scope
        .constructors
        .keys()
        .any(|n| n.resolve(interner) == new_name)
    {
        return Err(RefactorError::NameConflict {
            name: new_name.to_string(),
            existing_kind: SymbolKind::Variant,
        });
    }
    if scope
        .type_variants
        .keys()
        .any(|(_, variant_name)| variant_name.resolve(interner) == new_name)
    {
        return Err(RefactorError::NameConflict {
            name: new_name.to_string(),
            existing_kind: SymbolKind::Variant,
        });
    }

    Ok(())
}

// ── CST walking ─────────────────────────────────────────────────────

/// Collect text edits for renaming `old_name` → `new_name` in a single CST.
fn collect_rename_edits(
    root: &SyntaxNode,
    file_id: FileId,
    old_name: &str,
    new_name: &str,
    kind: SymbolKind,
) -> Vec<TextEdit> {
    let mut edits = Vec::new();

    for element in root.descendants_with_tokens() {
        let Some(token) = element.into_token() else {
            continue;
        };
        if !token.kind().is_identifier_token() || token.text() != old_name {
            continue;
        }

        let Some(parent) = token.parent() else {
            continue;
        };

        if parent.kind() == SyntaxKind::ImportMember
            && is_import_member_name_token(&token, &parent)
        {
            edits.push(TextEdit {
                file_id,
                range: token.text_range(),
                new_text: new_name.to_string(),
            });
            continue;
        }

        if should_rename_token(&parent, kind) {
            // For function renames, skip PathExpr usages that are locally
            // shadowed by a local binding or Param with the same name.
            if kind == SymbolKind::Function && is_locally_shadowed(&token, old_name) {
                continue;
            }
            edits.push(TextEdit {
                file_id,
                range: token.text_range(),
                new_text: new_name.to_string(),
            });
        }
    }

    // Sort descending by start offset for correct application order.
    edits.sort_by(|a, b| b.range.start().cmp(&a.range.start()));
    edits
}

fn is_import_member_name_token(token: &SyntaxToken, parent: &SyntaxNode) -> bool {
    let Some(member) = ImportMember::cast(parent.clone()) else {
        return false;
    };
    member
        .name_token()
        .as_ref()
        .is_some_and(|name_tok| name_tok.text_range() == token.text_range())
}

/// Determine if an ident token with the given parent should be renamed
/// for the target symbol kind.
pub fn should_rename_token(parent: &SyntaxNode, kind: SymbolKind) -> bool {
    let parent_kind = parent.kind();

    // Definition sites: ident is a direct child of a definition node.
    match kind {
        SymbolKind::Function if parent_kind == SyntaxKind::FnDef => return true,
        SymbolKind::Type if parent_kind == SyntaxKind::TypeDef => return true,
        SymbolKind::Capability if parent_kind == SyntaxKind::EffectDef => return true,
        SymbolKind::Variant if parent_kind == SyntaxKind::Variant => return true,
        _ => {}
    }

    // Usage sites: ident is inside a Path node. Check the Path's parent.
    if parent_kind == SyntaxKind::Path {
        let Some(grandparent) = parent.parent() else {
            return false;
        };
        return is_usage_site(grandparent.kind(), kind, &grandparent);
    }

    false
}

/// Check if a Path's parent node kind is a valid usage site for the
/// target symbol kind.
pub fn is_usage_site(gp_kind: SyntaxKind, kind: SymbolKind, grandparent: &SyntaxNode) -> bool {
    match kind {
        SymbolKind::Function => matches!(gp_kind, SyntaxKind::PathExpr | SyntaxKind::CallExpr),
        SymbolKind::Type => matches!(
            gp_kind,
            SyntaxKind::NameType | SyntaxKind::RecordExpr | SyntaxKind::RecordPat
        ),
        SymbolKind::Capability => {
            // Capabilities appear as NameType inside WithClause.
            if gp_kind == SyntaxKind::NameType
                && let Some(ggp) = grandparent.parent()
            {
                return ggp.kind() == SyntaxKind::WithClause;
            }
            // Also match EffectDef name position (handled above as definition site).
            false
        }
        SymbolKind::Variant => {
            if matches!(gp_kind, SyntaxKind::ConstructorPat | SyntaxKind::PathExpr) {
                return true;
            }
            // Zero-arg variant in match pattern: IdentPat > Path > Ident
            // But skip if the IdentPat is inside a local binding.
            if gp_kind == SyntaxKind::IdentPat {
                if let Some(ggp) = grandparent.parent() {
                    return !matches!(ggp.kind(), SyntaxKind::LetBinding | SyntaxKind::VarBinding);
                }
                return true;
            }
            false
        }
    }
}

/// Check if a token usage is locally shadowed by a local binding or `Param`
/// with the same name in the enclosing function body.
///
/// This prevents function renames from touching local variables that shadow
/// the function name.
fn is_locally_shadowed(token: &SyntaxToken, name: &str) -> bool {
    // Walk up from the token's parent to find the enclosing FnDef.
    let fn_def = token
        .parent_ancestors()
        .find(|n| n.kind() == SyntaxKind::FnDef);
    let Some(fn_def) = fn_def else {
        return false;
    };

    // Check if the token is at the definition site (direct child of FnDef).
    // Definition-site tokens should never be considered "shadowed".
    if let Some(parent) = token.parent()
        && parent.kind() == SyntaxKind::FnDef
    {
        return false;
    }

    let usage_offset = token.text_range().start();

    // Look for Param or local-binding nodes within this FnDef that bind the same name.
    for node in fn_def.descendants() {
        match node.kind() {
            // Params shadow the entire function body regardless of position.
            SyntaxKind::Param => {
                for child in node.children_with_tokens() {
                    if let Some(t) = child.into_token()
                        && t.kind().is_identifier_token()
                    {
                        if t.text() == name {
                            return true;
                        }
                        break; // Only check the first Ident
                    }
                }
            }
            // Local bindings only shadow usages that appear AFTER the binding.
            SyntaxKind::LetBinding | SyntaxKind::VarBinding => {
                if node.text_range().start() > usage_offset {
                    continue; // Binding is after usage — doesn't shadow it.
                }
                for child in node.children() {
                    if child.kind() == SyntaxKind::IdentPat {
                        for element in child.descendants_with_tokens() {
                            if let Some(t) = element.into_token()
                                && t.kind().is_identifier_token()
                                && t.text() == name
                            {
                                return true;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    false
}
