//! CST → Doc IR conversion.
//!
//! Dispatches on `SyntaxKind` to produce a `Doc` for every node in the
//! tree. Unknown or error nodes fall back to verbatim text output.

use kyokara_parser::SyntaxKind;
use kyokara_syntax::{SyntaxNode, SyntaxToken};

use crate::comments;
use crate::doc::Doc;

/// The indentation width (2 spaces).
const INDENT: i32 = 2;

/// Format a syntax node into a Doc.
pub fn format_node(node: &SyntaxNode) -> Doc {
    format_node_inner(node)
}

/// Format a syntax token into a Doc (just its text).
fn format_token(tok: &SyntaxToken) -> Doc {
    Doc::text(tok.text().to_string())
}

/// Inner dispatch — produces the Doc for the node itself, without
/// attached comments (those are handled by the caller).
fn format_node_inner(node: &SyntaxNode) -> Doc {
    match node.kind() {
        SyntaxKind::SourceFile => format_source_file(node),
        SyntaxKind::ModuleDecl => format_module_decl(node),
        SyntaxKind::ImportDecl => format_import_decl(node),
        SyntaxKind::Path => format_path(node),
        SyntaxKind::ImportAlias => format_import_alias(node),

        // Items
        SyntaxKind::TypeDef => format_type_def(node),
        SyntaxKind::FnDef => format_fn_def(node),
        SyntaxKind::EffectDef => format_cap_def(node),
        SyntaxKind::PropertyDef => format_property_def(node),
        SyntaxKind::LetBinding => format_let_binding(node),

        // Type-def sub-nodes
        SyntaxKind::RecordFieldList => format_record_field_list(node),
        SyntaxKind::RecordField => format_record_field(node),
        SyntaxKind::VariantList => format_variant_list(node),
        SyntaxKind::Variant => format_variant(node),
        SyntaxKind::VariantFieldList => format_variant_field_list(node),
        SyntaxKind::VariantField => format_variant_field(node),

        // Function sub-nodes
        SyntaxKind::ParamList => format_param_list(node),
        SyntaxKind::Param => format_param(node),
        SyntaxKind::ReturnType => format_return_type(node),
        SyntaxKind::WithClause => format_with_clause(node),
        SyntaxKind::PipeClause => format_pipe_clause(node),
        SyntaxKind::ContractSection => format_contract_section(node),
        SyntaxKind::RequiresClause => format_requires_clause(node),
        SyntaxKind::EnsuresClause => format_ensures_clause(node),
        SyntaxKind::InvariantClause => format_invariant_clause(node),

        // Generics
        SyntaxKind::TypeParamList => format_type_param_list(node),
        SyntaxKind::TypeParam => format_type_param(node),
        SyntaxKind::TypeArgList => format_type_arg_list(node),

        // Type expressions
        SyntaxKind::NameType => format_name_type(node),
        SyntaxKind::FnType => format_fn_type(node),
        SyntaxKind::RecordType => format_record_type(node),
        SyntaxKind::RefinedType => format_refined_type(node),

        // Expressions
        SyntaxKind::LiteralExpr => format_literal_expr(node),
        SyntaxKind::IdentExpr => format_ident_expr(node),
        SyntaxKind::PathExpr => format_path_expr(node),
        SyntaxKind::BinaryExpr => format_binary_expr(node),
        SyntaxKind::UnaryExpr => format_unary_expr(node),
        SyntaxKind::CallExpr => format_call_expr(node),
        SyntaxKind::NamedArg => format_named_arg(node),
        SyntaxKind::ArgList => format_arg_list(node),
        SyntaxKind::FieldExpr => format_field_expr(node),
        SyntaxKind::PipelineExpr => format_pipeline_expr(node),
        SyntaxKind::PropagateExpr => format_propagate_expr(node),
        SyntaxKind::MatchExpr => format_match_expr(node),
        SyntaxKind::MatchArm => format_match_arm(node),
        SyntaxKind::MatchArmList => format_match_arm_list(node),
        SyntaxKind::IfExpr => format_if_expr(node),
        SyntaxKind::BlockExpr => format_block_expr(node),
        SyntaxKind::RecordExpr => format_record_expr(node),
        SyntaxKind::RecordExprField => format_record_expr_field(node),
        SyntaxKind::RecordExprFieldList => format_record_expr_field_list(node),
        SyntaxKind::ReturnExpr => format_return_expr(node),
        SyntaxKind::HoleExpr => Doc::text("_"),
        SyntaxKind::OldExpr => format_old_expr(node),
        SyntaxKind::ParenExpr => format_paren_expr(node),
        SyntaxKind::LambdaExpr => format_lambda_expr(node),

        // Patterns
        SyntaxKind::IdentPat => format_ident_pat(node),
        SyntaxKind::ConstructorPat => format_constructor_pat(node),
        SyntaxKind::WildcardPat => Doc::text("_"),
        SyntaxKind::LiteralPat => format_literal_pat(node),
        SyntaxKind::RecordPat => format_record_pat(node),
        SyntaxKind::PatList => format_pat_list(node),

        // Property
        SyntaxKind::PropertyParamList => format_property_param_list(node),
        SyntaxKind::PropertyParam => format_property_param(node),
        SyntaxKind::WhereClause => format_where_clause(node),
        SyntaxKind::ForAllBinder => format_for_all_binder(node),

        // Error recovery — verbatim fallback
        SyntaxKind::ErrorNode | SyntaxKind::Error => verbatim(node),

        // Anything else — verbatim
        _ => verbatim(node),
    }
}

/// Verbatim fallback: emit the node's original text unchanged.
fn verbatim(node: &SyntaxNode) -> Doc {
    Doc::text(node.text().to_string())
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Find the first token of the given kind in a node's direct children.
fn find_token(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|it| it.into_token())
        .find(|tok| tok.kind() == kind)
}

/// Find the first identifier token in a node's direct children.
fn find_ident(node: &SyntaxNode) -> Option<SyntaxToken> {
    find_token(node, SyntaxKind::Ident)
}

/// Find the first child node of the given kind.
fn find_child_node(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.children().find(|c| c.kind() == kind)
}

/// Collect all child nodes of the given kind.
fn child_nodes(node: &SyntaxNode, kind: SyntaxKind) -> Vec<SyntaxNode> {
    node.children().filter(|c| c.kind() == kind).collect()
}

/// Collect non-trivia tokens from direct children.
fn non_trivia_tokens(node: &SyntaxNode) -> Vec<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|it| it.into_token())
        .filter(|tok| !tok.kind().is_trivia())
        .collect()
}

/// Complex lambda bodies are clearer in forced multiline layout.
fn lambda_body_prefers_multiline(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IfExpr | SyntaxKind::MatchExpr | SyntaxKind::BlockExpr
    )
}

// ── Source file ─────────────────────────────────────────────────────

fn format_source_file(node: &SyntaxNode) -> Doc {
    // Single pass: categorize all children with their attached comments.
    let is_top_level_item = |c: &SyntaxNode| {
        matches!(
            c.kind(),
            SyntaxKind::ModuleDecl
                | SyntaxKind::ImportDecl
                | SyntaxKind::TypeDef
                | SyntaxKind::FnDef
                | SyntaxKind::EffectDef
                | SyntaxKind::PropertyDef
                | SyntaxKind::LetBinding
                | SyntaxKind::ErrorNode
        )
    };

    let all_children =
        comments::format_children_with_comments(node, &is_top_level_item, format_node);

    // Separate into module, imports, and items based on original node kinds.
    let child_nodes_iter: Vec<SyntaxNode> =
        node.children().filter(|c| is_top_level_item(c)).collect();

    let mut module_docs = Vec::new();
    let mut import_docs = Vec::new();
    let mut import_sort_keys = Vec::new();
    let mut item_docs = Vec::new();

    let mut all_iter = all_children.into_iter();
    for child_node in child_nodes_iter {
        if let Some(child_doc) = all_iter.next() {
            match child_node.kind() {
                SyntaxKind::ModuleDecl => module_docs.push(child_doc),
                SyntaxKind::ImportDecl => {
                    import_sort_keys.push(import_sort_key(&child_node));
                    import_docs.push(child_doc);
                }
                _ => item_docs.push(child_doc),
            }
        }
    }
    // Standalone trailing comments (after the last item).
    for trailing_doc in all_iter {
        item_docs.push(trailing_doc);
    }

    // Sort imports alphabetically.
    if import_docs.len() > 1 {
        let mut indexed: Vec<(usize, &str)> = import_sort_keys
            .iter()
            .enumerate()
            .map(|(i, k)| (i, k.as_str()))
            .collect();
        indexed.sort_by(|a, b| a.1.cmp(b.1));
        import_docs = indexed
            .iter()
            .map(|(i, _)| import_docs[*i].clone())
            .collect();
    }

    let mut parts = Vec::new();
    parts.extend(module_docs);

    if !import_docs.is_empty() {
        if !parts.is_empty() {
            parts.push(Doc::HardLine);
            parts.push(Doc::HardLine);
        }
        parts.push(Doc::join(import_docs, Doc::HardLine));
    }

    if !item_docs.is_empty() {
        if !parts.is_empty() {
            parts.push(Doc::HardLine);
            parts.push(Doc::HardLine);
        }
        parts.push(Doc::join(
            item_docs,
            Doc::concat(vec![Doc::HardLine, Doc::HardLine]),
        ));
    }

    if !parts.is_empty() {
        parts.push(Doc::HardLine);
    }

    Doc::concat(parts)
}

/// Sort key for imports: the path text (e.g. "Std.IO").
fn import_sort_key(node: &SyntaxNode) -> String {
    if let Some(path) = find_child_node(node, SyntaxKind::Path) {
        path.text().to_string()
    } else {
        String::new()
    }
}

// ── Module / Import ─────────────────────────────────────────────────

fn format_module_decl(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("module"), Doc::text(" ")];
    if let Some(path) = find_child_node(node, SyntaxKind::Path) {
        parts.push(format_node(&path));
    }
    Doc::concat(parts)
}

fn format_import_decl(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("import"), Doc::text(" ")];
    if let Some(path) = find_child_node(node, SyntaxKind::Path) {
        parts.push(format_node(&path));
    }
    if let Some(alias) = find_child_node(node, SyntaxKind::ImportAlias) {
        parts.push(Doc::text(" "));
        parts.push(format_node(&alias));
    }
    Doc::concat(parts)
}

fn format_import_alias(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("as"), Doc::text(" ")];
    if let Some(ident) = find_ident(node) {
        parts.push(format_token(&ident));
    }
    Doc::concat(parts)
}

fn format_path(node: &SyntaxNode) -> Doc {
    let segments: Vec<Doc> = node
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .filter(|tok| tok.kind() == SyntaxKind::Ident)
        .map(|tok| format_token(&tok))
        .collect();
    Doc::join(segments, Doc::text("."))
}

// ── Type definitions ────────────────────────────────────────────────

fn format_type_def(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("type"), Doc::text(" ")];

    if let Some(name) = find_ident(node) {
        parts.push(format_token(&name));
    }

    if let Some(type_params) = find_child_node(node, SyntaxKind::TypeParamList) {
        parts.push(format_node(&type_params));
    }

    // Determine the kind of type def
    if let Some(variants) = find_child_node(node, SyntaxKind::VariantList) {
        // ADT with variants
        parts.push(Doc::text(" ="));
        parts.push(format_node(&variants));
    } else if let Some(record_fields) = find_child_node(node, SyntaxKind::RecordFieldList) {
        // Record type
        parts.push(Doc::text(" = "));
        parts.push(format_node(&record_fields));
    } else {
        // Type alias — find the type expression child
        let type_expr = node.children().find(|c| is_type_expr(c.kind()));
        if let Some(te) = type_expr {
            parts.push(Doc::text(" = "));
            parts.push(format_node(&te));
        }
    }

    Doc::concat(parts)
}

fn is_type_expr(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::NameType
            | SyntaxKind::FnType
            | SyntaxKind::RecordType
            | SyntaxKind::RefinedType
    )
}

fn is_expr(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LiteralExpr
            | SyntaxKind::IdentExpr
            | SyntaxKind::PathExpr
            | SyntaxKind::BinaryExpr
            | SyntaxKind::UnaryExpr
            | SyntaxKind::CallExpr
            | SyntaxKind::FieldExpr
            | SyntaxKind::PipelineExpr
            | SyntaxKind::PropagateExpr
            | SyntaxKind::MatchExpr
            | SyntaxKind::IfExpr
            | SyntaxKind::BlockExpr
            | SyntaxKind::RecordExpr
            | SyntaxKind::ReturnExpr
            | SyntaxKind::HoleExpr
            | SyntaxKind::OldExpr
            | SyntaxKind::ParenExpr
            | SyntaxKind::LambdaExpr
    )
}

fn is_pat(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IdentPat
            | SyntaxKind::ConstructorPat
            | SyntaxKind::WildcardPat
            | SyntaxKind::LiteralPat
            | SyntaxKind::RecordPat
    )
}

fn format_variant_list(node: &SyntaxNode) -> Doc {
    let variants = child_nodes(node, SyntaxKind::Variant);
    let mut parts = Vec::new();
    for v in &variants {
        parts.push(Doc::concat(vec![
            Doc::HardLine,
            Doc::text("  | "),
            format_node(v),
        ]));
    }
    Doc::concat(parts)
}

fn format_variant(node: &SyntaxNode) -> Doc {
    let mut parts = Vec::new();
    if let Some(name) = find_ident(node) {
        parts.push(format_token(&name));
    }
    if let Some(fields) = find_child_node(node, SyntaxKind::VariantFieldList) {
        parts.push(format_node(&fields));
    }
    Doc::concat(parts)
}

fn format_variant_field_list(node: &SyntaxNode) -> Doc {
    let fields = child_nodes(node, SyntaxKind::VariantField);
    let field_docs: Vec<Doc> = fields.iter().map(format_node).collect();
    Doc::group(Doc::concat(vec![
        Doc::text("("),
        Doc::indent(
            INDENT,
            Doc::concat(vec![
                Doc::SoftLine,
                Doc::join(field_docs, Doc::concat(vec![Doc::text(","), Doc::Line])),
                Doc::trailing_comma(),
            ]),
        ),
        Doc::SoftLine,
        Doc::text(")"),
    ]))
}

fn format_variant_field(node: &SyntaxNode) -> Doc {
    // A variant field is just a type expression
    let type_expr = node.children().find(|c| is_type_expr(c.kind()));
    if let Some(te) = type_expr {
        format_node(&te)
    } else {
        verbatim(node)
    }
}

fn format_record_field_list(node: &SyntaxNode) -> Doc {
    let fields = child_nodes(node, SyntaxKind::RecordField);
    let field_docs: Vec<Doc> = fields.iter().map(format_node).collect();
    Doc::group(Doc::concat(vec![
        Doc::text("{"),
        Doc::indent(
            INDENT,
            Doc::concat(vec![
                Doc::Line,
                Doc::join(field_docs, Doc::concat(vec![Doc::text(","), Doc::Line])),
                Doc::trailing_comma(),
            ]),
        ),
        Doc::Line,
        Doc::text("}"),
    ]))
}

fn format_record_field(node: &SyntaxNode) -> Doc {
    let mut parts = Vec::new();
    if let Some(name) = find_ident(node) {
        parts.push(format_token(&name));
    }
    parts.push(Doc::text(": "));
    let type_expr = node.children().find(|c| is_type_expr(c.kind()));
    if let Some(te) = type_expr {
        parts.push(format_node(&te));
    }
    Doc::concat(parts)
}

// ── Function definitions ────────────────────────────────────────────

fn format_fn_def(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("fn"), Doc::text(" ")];
    let mut has_sections = false;

    if let Some(name) = find_ident(node) {
        parts.push(format_token(&name));
    }

    if let Some(type_params) = find_child_node(node, SyntaxKind::TypeParamList) {
        parts.push(format_node(&type_params));
    }

    if let Some(params) = find_child_node(node, SyntaxKind::ParamList) {
        parts.push(format_node(&params));
    }

    if let Some(ret) = find_child_node(node, SyntaxKind::ReturnType) {
        parts.push(Doc::text(" "));
        parts.push(format_node(&ret));
    }

    if let Some(with_clause) = find_child_node(node, SyntaxKind::WithClause) {
        has_sections = true;
        parts.push(Doc::HardLine);
        parts.push(format_node(&with_clause));
    }

    if let Some(pipe_clause) = find_child_node(node, SyntaxKind::PipeClause) {
        has_sections = true;
        parts.push(Doc::HardLine);
        parts.push(format_node(&pipe_clause));
    }

    if let Some(contract) = find_child_node(node, SyntaxKind::ContractSection) {
        has_sections = true;
        parts.push(Doc::HardLine);
        parts.push(format_node(&contract));
    }

    if let Some(body) = find_child_node(node, SyntaxKind::BlockExpr) {
        if has_sections {
            parts.push(Doc::HardLine);
        } else {
            parts.push(Doc::text(" "));
        }
        parts.push(format_node(&body));
    }

    Doc::concat(parts)
}

fn format_param_list(node: &SyntaxNode) -> Doc {
    let params = child_nodes(node, SyntaxKind::Param);
    let param_docs: Vec<Doc> = params.iter().map(format_node).collect();
    Doc::group(Doc::concat(vec![
        Doc::text("("),
        Doc::indent(
            INDENT,
            Doc::concat(vec![
                Doc::SoftLine,
                Doc::join(param_docs, Doc::concat(vec![Doc::text(","), Doc::Line])),
                Doc::trailing_comma(),
            ]),
        ),
        Doc::SoftLine,
        Doc::text(")"),
    ]))
}

fn format_param(node: &SyntaxNode) -> Doc {
    let mut parts = Vec::new();
    if let Some(name) = find_ident(node) {
        parts.push(format_token(&name));
    }
    let type_expr = node.children().find(|c| is_type_expr(c.kind()));
    if let Some(te) = type_expr {
        parts.push(Doc::text(": "));
        parts.push(format_node(&te));
    }
    Doc::concat(parts)
}

fn format_return_type(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("-> ")];
    let type_expr = node.children().find(|c| is_type_expr(c.kind()));
    if let Some(te) = type_expr {
        parts.push(format_node(&te));
    }
    Doc::concat(parts)
}

fn format_with_clause(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("with"), Doc::text(" ")];
    let types: Vec<SyntaxNode> = node.children().filter(|c| is_type_expr(c.kind())).collect();
    let type_docs: Vec<Doc> = types.iter().map(format_node).collect();
    parts.push(Doc::join(
        type_docs,
        Doc::concat(vec![Doc::text(","), Doc::text(" ")]),
    ));
    Doc::concat(parts)
}

fn format_pipe_clause(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("pipe"), Doc::text(" ")];
    let types: Vec<SyntaxNode> = node.children().filter(|c| is_type_expr(c.kind())).collect();
    let type_docs: Vec<Doc> = types.iter().map(format_node).collect();
    parts.push(Doc::join(
        type_docs,
        Doc::concat(vec![Doc::text(","), Doc::text(" ")]),
    ));
    Doc::concat(parts)
}

fn format_contract_section(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("contract")];
    for kind in [
        SyntaxKind::RequiresClause,
        SyntaxKind::EnsuresClause,
        SyntaxKind::InvariantClause,
    ] {
        for clause in child_nodes(node, kind) {
            parts.push(Doc::indent(
                INDENT,
                Doc::concat(vec![Doc::HardLine, format_node(&clause)]),
            ));
        }
    }
    Doc::concat(parts)
}

fn format_requires_clause(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("requires"), Doc::text(" "), Doc::text("(")];
    let expr = node.children().find(|c| is_expr(c.kind()));
    if let Some(e) = expr {
        parts.push(format_node(&e));
    }
    parts.push(Doc::text(")"));
    Doc::concat(parts)
}

fn format_ensures_clause(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("ensures"), Doc::text(" "), Doc::text("(")];
    let expr = node.children().find(|c| is_expr(c.kind()));
    if let Some(e) = expr {
        parts.push(format_node(&e));
    }
    parts.push(Doc::text(")"));
    Doc::concat(parts)
}

fn format_invariant_clause(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("invariant"), Doc::text(" "), Doc::text("(")];
    let expr = node.children().find(|c| is_expr(c.kind()));
    if let Some(e) = expr {
        parts.push(format_node(&e));
    }
    parts.push(Doc::text(")"));
    Doc::concat(parts)
}

// ── Cap / Property ──────────────────────────────────────────────────

fn format_cap_def(node: &SyntaxNode) -> Doc {
    // Keep invalid/extra syntax verbatim to avoid destructive formatting on
    // parse-damaged effect declarations (e.g. effect bodies/type params).
    if node.children().next().is_some() {
        return verbatim(node);
    }

    let mut parts = vec![Doc::text("effect"), Doc::text(" ")];

    if let Some(name) = find_ident(node) {
        parts.push(format_token(&name));
    }

    Doc::concat(parts)
}

fn format_property_def(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("property"), Doc::text(" ")];

    if let Some(name) = find_ident(node) {
        parts.push(format_token(&name));
    }

    if let Some(params) = find_child_node(node, SyntaxKind::PropertyParamList) {
        parts.push(format_node(&params));
    }

    if let Some(where_clause) = find_child_node(node, SyntaxKind::WhereClause) {
        parts.push(Doc::HardLine);
        parts.push(format_node(&where_clause));
    }

    if let Some(body) = find_child_node(node, SyntaxKind::BlockExpr) {
        parts.push(Doc::text(" "));
        parts.push(format_node(&body));
    }

    Doc::concat(parts)
}

fn format_property_param_list(node: &SyntaxNode) -> Doc {
    let params: Vec<Doc> = node
        .children()
        .filter(|c| c.kind() == SyntaxKind::PropertyParam)
        .map(|c| format_node(&c))
        .collect();

    if params.is_empty() {
        Doc::text("()")
    } else {
        Doc::concat(vec![
            Doc::text("("),
            Doc::join(params, Doc::text(", ")),
            Doc::text(")"),
        ])
    }
}

fn format_property_param(node: &SyntaxNode) -> Doc {
    let mut parts = Vec::new();

    if let Some(name) = find_ident(node) {
        parts.push(format_token(&name));
    }

    let type_expr = node.children().find(|c| is_type_expr(c.kind()));
    if let Some(te) = type_expr {
        parts.push(Doc::text(": "));
        parts.push(format_node(&te));
    }

    // Generator expression (after `<-` token).
    let gen_expr = node.children().find(|c| is_expr(c.kind()));
    if let Some(ge) = gen_expr {
        parts.push(Doc::text(" <- "));
        parts.push(format_node(&ge));
    }

    Doc::concat(parts)
}

fn format_where_clause(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("where"), Doc::text(" "), Doc::text("(")];

    let expr = node.children().find(|c| is_expr(c.kind()));
    if let Some(e) = expr {
        parts.push(format_node(&e));
    }

    parts.push(Doc::text(")"));

    Doc::concat(parts)
}

fn format_for_all_binder(node: &SyntaxNode) -> Doc {
    let mut parts = vec![
        Doc::text("for"),
        Doc::text(" "),
        Doc::text("all"),
        Doc::text(" "),
    ];
    if let Some(name) = find_ident(node) {
        parts.push(format_token(&name));
    }
    let type_expr = node.children().find(|c| is_type_expr(c.kind()));
    if let Some(te) = type_expr {
        parts.push(Doc::text(": "));
        parts.push(format_node(&te));
    }
    parts.push(Doc::text("."));
    Doc::concat(parts)
}

// ── Let binding ─────────────────────────────────────────────────────

fn format_let_binding(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("let"), Doc::text(" ")];

    // Pattern
    let pat = node.children().find(|c| is_pat(c.kind()));
    if let Some(p) = pat {
        parts.push(format_node(&p));
    }

    // Optional type annotation
    let type_expr = node.children().find(|c| is_type_expr(c.kind()));
    if let Some(te) = type_expr {
        parts.push(Doc::text(": "));
        parts.push(format_node(&te));
    }

    // Value
    let expr = node.children().find(|c| is_expr(c.kind()));
    if let Some(e) = expr {
        let is_compound = matches!(
            e.kind(),
            SyntaxKind::MatchExpr | SyntaxKind::IfExpr | SyntaxKind::BlockExpr
        );
        if is_compound {
            // Compound expressions handle their own indentation.
            parts.push(Doc::text(" = "));
            parts.push(format_node(&e));
        } else {
            // Simple expressions: break after `=` if long.
            parts.push(Doc::text(" ="));
            parts.push(Doc::group(Doc::indent(
                INDENT,
                Doc::concat(vec![Doc::Line, format_node(&e)]),
            )));
        }
    }

    Doc::concat(parts)
}

// ── Generics ────────────────────────────────────────────────────────

fn format_type_param_list(node: &SyntaxNode) -> Doc {
    let params = child_nodes(node, SyntaxKind::TypeParam);
    let param_docs: Vec<Doc> = params.iter().map(format_node).collect();
    Doc::concat(vec![
        Doc::text("<"),
        Doc::join(
            param_docs,
            Doc::concat(vec![Doc::text(","), Doc::text(" ")]),
        ),
        Doc::text(">"),
    ])
}

fn format_type_param(node: &SyntaxNode) -> Doc {
    if let Some(name) = find_ident(node) {
        format_token(&name)
    } else {
        verbatim(node)
    }
}

fn format_type_arg_list(node: &SyntaxNode) -> Doc {
    let args: Vec<SyntaxNode> = node.children().filter(|c| is_type_expr(c.kind())).collect();
    let arg_docs: Vec<Doc> = args.iter().map(format_node).collect();
    Doc::concat(vec![
        Doc::text("<"),
        Doc::join(arg_docs, Doc::concat(vec![Doc::text(","), Doc::text(" ")])),
        Doc::text(">"),
    ])
}

// ── Type expressions ────────────────────────────────────────────────

fn format_name_type(node: &SyntaxNode) -> Doc {
    let mut parts = Vec::new();
    if let Some(path) = find_child_node(node, SyntaxKind::Path) {
        parts.push(format_node(&path));
    }
    if let Some(args) = find_child_node(node, SyntaxKind::TypeArgList) {
        parts.push(format_node(&args));
    }
    Doc::concat(parts)
}

fn format_fn_type(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("fn")];
    let types: Vec<SyntaxNode> = node.children().filter(|c| is_type_expr(c.kind())).collect();

    if types.len() >= 2 {
        // param types then return type
        let (params, ret) = types.split_at(types.len() - 1);
        let param_docs: Vec<Doc> = params.iter().map(format_node).collect();
        parts.push(Doc::text("("));
        parts.push(Doc::join(
            param_docs,
            Doc::concat(vec![Doc::text(","), Doc::text(" ")]),
        ));
        parts.push(Doc::text(")"));
        parts.push(Doc::text(" -> "));
        parts.push(format_node(&ret[0]));
    } else if types.len() == 1 {
        // Just return type, no params
        parts.push(Doc::text("()"));
        parts.push(Doc::text(" -> "));
        parts.push(format_node(&types[0]));
    }

    Doc::concat(parts)
}

fn format_record_type(node: &SyntaxNode) -> Doc {
    let fields = child_nodes(node, SyntaxKind::RecordField);
    let field_docs: Vec<Doc> = fields.iter().map(format_node).collect();
    Doc::group(Doc::concat(vec![
        Doc::text("{"),
        Doc::indent(
            INDENT,
            Doc::concat(vec![
                Doc::Line,
                Doc::join(field_docs, Doc::concat(vec![Doc::text(","), Doc::Line])),
                Doc::trailing_comma(),
            ]),
        ),
        Doc::Line,
        Doc::text("}"),
    ]))
}

fn format_refined_type(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("{"), Doc::text(" ")];

    if let Some(name) = find_ident(node) {
        parts.push(format_token(&name));
    }

    let type_expr = node.children().find(|c| is_type_expr(c.kind()));
    if let Some(te) = type_expr {
        parts.push(Doc::text(": "));
        parts.push(format_node(&te));
    }

    parts.push(Doc::text(" | "));

    let expr = node.children().find(|c| is_expr(c.kind()));
    if let Some(e) = expr {
        parts.push(format_node(&e));
    }

    parts.push(Doc::text(" }"));
    Doc::concat(parts)
}

// ── Expressions ─────────────────────────────────────────────────────

fn format_literal_expr(node: &SyntaxNode) -> Doc {
    // Find the first non-trivia token
    let tok = non_trivia_tokens(node).into_iter().next();
    if let Some(t) = tok {
        format_token(&t)
    } else {
        verbatim(node)
    }
}

fn format_ident_expr(node: &SyntaxNode) -> Doc {
    if let Some(ident) = find_ident(node) {
        format_token(&ident)
    } else {
        // Could be a keyword used as expr (true/false handled by LiteralExpr typically)
        verbatim(node)
    }
}

fn format_path_expr(node: &SyntaxNode) -> Doc {
    if let Some(path) = find_child_node(node, SyntaxKind::Path) {
        format_node(&path)
    } else {
        verbatim(node)
    }
}

fn format_binary_expr(node: &SyntaxNode) -> Doc {
    // Collect: lhs, op, rhs from children_with_tokens
    let exprs: Vec<SyntaxNode> = node.children().filter(|c| is_expr(c.kind())).collect();
    let op = node
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .find(|tok| tok.kind().is_binary_operator());

    if exprs.len() == 2 {
        let lhs = format_node(&exprs[0]);
        let rhs = format_node(&exprs[1]);
        let op_text = op.map(|t| t.text().to_string()).unwrap_or_default();
        Doc::group(Doc::concat(vec![
            lhs,
            Doc::text(" "),
            Doc::text(&op_text),
            Doc::indent(INDENT, Doc::concat(vec![Doc::Line, rhs])),
        ]))
    } else {
        verbatim(node)
    }
}

fn format_unary_expr(node: &SyntaxNode) -> Doc {
    let op = node
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .find(|tok| tok.kind().is_unary_prefix_operator());
    let operand = node.children().find(|c| is_expr(c.kind()));

    let mut parts = Vec::new();
    if let Some(op) = op {
        parts.push(format_token(&op));
    }
    if let Some(e) = operand {
        parts.push(format_node(&e));
    }
    Doc::concat(parts)
}

fn format_call_expr(node: &SyntaxNode) -> Doc {
    let callee = node.children().find(|c| is_expr(c.kind()));
    let arg_list = find_child_node(node, SyntaxKind::ArgList);

    let mut parts = Vec::new();
    if let Some(c) = callee {
        parts.push(format_node(&c));
    }
    if let Some(al) = arg_list {
        parts.push(format_node(&al));
    }
    Doc::concat(parts)
}

fn format_arg_list(node: &SyntaxNode) -> Doc {
    // Collect all args: positional (Expr children) and named (NamedArg children).
    // They appear interleaved in the CST, so process all children in order.
    let mut arg_docs = Vec::new();
    for child in node.children() {
        if is_expr(child.kind()) || child.kind() == SyntaxKind::NamedArg {
            arg_docs.push(format_node(&child));
        }
    }

    Doc::group(Doc::concat(vec![
        Doc::text("("),
        Doc::indent(
            INDENT,
            Doc::concat(vec![
                Doc::SoftLine,
                Doc::join(arg_docs, Doc::concat(vec![Doc::text(","), Doc::Line])),
                Doc::trailing_comma(),
            ]),
        ),
        Doc::SoftLine,
        Doc::text(")"),
    ]))
}

fn format_named_arg(node: &SyntaxNode) -> Doc {
    let mut parts = Vec::new();
    if let Some(name) = find_ident(node) {
        parts.push(format_token(&name));
    }
    parts.push(Doc::text(": "));
    let expr = node.children().find(|c| is_expr(c.kind()));
    if let Some(e) = expr {
        parts.push(format_node(&e));
    }
    Doc::concat(parts)
}

fn format_field_expr(node: &SyntaxNode) -> Doc {
    let base = node.children().find(|c| is_expr(c.kind()));
    let field = node
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .filter(|tok| tok.kind() == SyntaxKind::Ident)
        .last();

    let mut parts = Vec::new();
    if let Some(b) = base {
        parts.push(format_node(&b));
    }
    parts.push(Doc::text("."));
    if let Some(f) = field {
        parts.push(format_token(&f));
    }
    Doc::concat(parts)
}

fn format_pipeline_expr(node: &SyntaxNode) -> Doc {
    let exprs: Vec<SyntaxNode> = node.children().filter(|c| is_expr(c.kind())).collect();
    if exprs.len() == 2 {
        Doc::group(Doc::concat(vec![
            format_node(&exprs[0]),
            Doc::indent(
                INDENT,
                Doc::concat(vec![Doc::Line, Doc::text("|> "), format_node(&exprs[1])]),
            ),
        ]))
    } else {
        verbatim(node)
    }
}

fn format_propagate_expr(node: &SyntaxNode) -> Doc {
    let inner = node.children().find(|c| is_expr(c.kind()));
    let mut parts = Vec::new();
    if let Some(e) = inner {
        parts.push(format_node(&e));
    }
    parts.push(Doc::text("?"));
    Doc::concat(parts)
}

fn format_match_expr(node: &SyntaxNode) -> Doc {
    let scrutinee = node.children().find(|c| is_expr(c.kind()));
    let arm_list = find_child_node(node, SyntaxKind::MatchArmList);

    let mut parts = vec![Doc::text("match"), Doc::text(" "), Doc::text("(")];
    if let Some(s) = scrutinee {
        parts.push(format_node(&s));
    }
    parts.push(Doc::text(") "));
    if let Some(al) = arm_list {
        parts.push(format_node(&al));
    }
    Doc::concat(parts)
}

fn format_match_arm_list(node: &SyntaxNode) -> Doc {
    let arm_docs = comments::format_children_with_comments(
        node,
        |c| c.kind() == SyntaxKind::MatchArm,
        format_node,
    );
    if arm_docs.is_empty() {
        return Doc::text("{}");
    }
    let has_real_arms = node.children().any(|c| c.kind() == SyntaxKind::MatchArm);
    if !has_real_arms {
        // Comment-only arm list: emit comments without commas.
        return Doc::concat(vec![
            Doc::text("{"),
            Doc::indent(
                INDENT,
                Doc::concat(vec![Doc::HardLine, Doc::concat(arm_docs)]),
            ),
            Doc::HardLine,
            Doc::text("}"),
        ]);
    }
    Doc::concat(vec![
        Doc::text("{"),
        Doc::indent(
            INDENT,
            Doc::concat(vec![
                Doc::HardLine,
                Doc::join(arm_docs, Doc::concat(vec![Doc::text(","), Doc::HardLine])),
                Doc::text(","),
            ]),
        ),
        Doc::HardLine,
        Doc::text("}"),
    ])
}

fn format_match_arm(node: &SyntaxNode) -> Doc {
    let pat = node.children().find(|c| is_pat(c.kind()));
    let body = node.children().find(|c| is_expr(c.kind()));

    let mut parts = Vec::new();
    if let Some(p) = pat {
        parts.push(format_node(&p));
    }
    parts.push(Doc::text(" => "));
    if let Some(b) = body {
        parts.push(format_node(&b));
    }
    Doc::concat(parts)
}

fn format_if_expr(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("if"), Doc::text(" "), Doc::text("(")];

    let condition = node.children().find(|c| is_expr(c.kind()));
    if let Some(c) = condition {
        parts.push(format_node(&c));
    }
    parts.push(Doc::text(")"));

    let blocks: Vec<SyntaxNode> = child_nodes(node, SyntaxKind::BlockExpr);
    if let Some(then) = blocks.first() {
        parts.push(Doc::text(" "));
        parts.push(format_node(then));
    }

    // Check for else
    let has_else = node
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .any(|tok| tok.kind() == SyntaxKind::ElseKw);

    if has_else {
        // else-if chain?
        let nested_ifs: Vec<SyntaxNode> = child_nodes(node, SyntaxKind::IfExpr);
        if let Some(nested_if) = nested_ifs.first() {
            // If we're *this* IfExpr, skip ourself — the nested one
            // is the second IfExpr child (the first is potentially
            // the condition if it were an IfExpr, which it's not for
            // a normal if-expr). Actually IfExpr children of an IfExpr
            // are the else-if branches.
            parts.push(Doc::text(" else "));
            parts.push(format_node(nested_if));
        } else if blocks.len() >= 2 {
            parts.push(Doc::text(" else "));
            parts.push(format_node(&blocks[1]));
        }
    }

    Doc::concat(parts)
}

fn format_block_expr(node: &SyntaxNode) -> Doc {
    let item_docs = comments::format_children_with_comments(
        node,
        |c| is_expr(c.kind()) || c.kind() == SyntaxKind::LetBinding,
        format_node,
    );

    if item_docs.is_empty() {
        return Doc::text("{}");
    }

    Doc::concat(vec![
        Doc::text("{"),
        Doc::indent(
            INDENT,
            Doc::concat(vec![Doc::HardLine, Doc::join(item_docs, Doc::HardLine)]),
        ),
        Doc::HardLine,
        Doc::text("}"),
    ])
}

fn format_record_expr(node: &SyntaxNode) -> Doc {
    let mut parts = Vec::new();
    if let Some(path) = find_child_node(node, SyntaxKind::Path) {
        parts.push(format_node(&path));
    }
    parts.push(Doc::text(" "));
    if let Some(field_list) = find_child_node(node, SyntaxKind::RecordExprFieldList) {
        parts.push(format_node(&field_list));
    }
    Doc::concat(parts)
}

fn format_record_expr_field_list(node: &SyntaxNode) -> Doc {
    let fields = child_nodes(node, SyntaxKind::RecordExprField);
    let field_docs: Vec<Doc> = fields.iter().map(format_node).collect();
    Doc::group(Doc::concat(vec![
        Doc::text("{"),
        Doc::indent(
            INDENT,
            Doc::concat(vec![
                Doc::Line,
                Doc::join(field_docs, Doc::concat(vec![Doc::text(","), Doc::Line])),
                Doc::trailing_comma(),
            ]),
        ),
        Doc::Line,
        Doc::text("}"),
    ]))
}

fn format_record_expr_field(node: &SyntaxNode) -> Doc {
    let mut parts = Vec::new();
    if let Some(name) = find_ident(node) {
        parts.push(format_token(&name));
    }
    let expr = node.children().find(|c| is_expr(c.kind()));
    if let Some(e) = expr {
        parts.push(Doc::text(": "));
        parts.push(format_node(&e));
    }
    Doc::concat(parts)
}

fn format_return_expr(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("return")];
    let expr = node.children().find(|c| is_expr(c.kind()));
    if let Some(e) = expr {
        parts.push(Doc::text(" "));
        parts.push(format_node(&e));
    }
    Doc::concat(parts)
}

fn format_old_expr(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("old(")];
    let expr = node.children().find(|c| is_expr(c.kind()));
    if let Some(e) = expr {
        parts.push(format_node(&e));
    }
    parts.push(Doc::text(")"));
    Doc::concat(parts)
}

fn format_paren_expr(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("(")];
    let expr = node.children().find(|c| is_expr(c.kind()));
    if let Some(e) = expr {
        parts.push(format_node(&e));
    }
    parts.push(Doc::text(")"));
    Doc::concat(parts)
}

fn format_lambda_expr(node: &SyntaxNode) -> Doc {
    let mut parts = vec![Doc::text("fn")];

    if let Some(params) = find_child_node(node, SyntaxKind::ParamList) {
        parts.push(format_node(&params));
    }

    // Return type (optional)
    if let Some(ret) = find_child_node(node, SyntaxKind::ReturnType) {
        parts.push(Doc::text(" "));
        parts.push(format_node(&ret));
    }

    parts.push(Doc::text(" => "));

    let body = node.children().find(|c| is_expr(c.kind()));
    if let Some(b) = body {
        if lambda_body_prefers_multiline(b.kind()) {
            parts.push(Doc::indent(
                INDENT,
                Doc::concat(vec![Doc::HardLine, format_node(&b)]),
            ));
        } else {
            parts.push(Doc::group(Doc::indent(
                INDENT,
                Doc::concat(vec![Doc::SoftLine, format_node(&b)]),
            )));
        }
    }

    Doc::concat(parts)
}

// ── Patterns ────────────────────────────────────────────────────────

fn format_ident_pat(node: &SyntaxNode) -> Doc {
    if let Some(path) = find_child_node(node, SyntaxKind::Path) {
        format_node(&path)
    } else if let Some(ident) = find_ident(node) {
        format_token(&ident)
    } else {
        verbatim(node)
    }
}

fn format_constructor_pat(node: &SyntaxNode) -> Doc {
    let mut parts = Vec::new();
    if let Some(path) = find_child_node(node, SyntaxKind::Path) {
        parts.push(format_node(&path));
    }

    // Pattern arguments
    let pats: Vec<SyntaxNode> = node.children().filter(|c| is_pat(c.kind())).collect();
    if !pats.is_empty() {
        let pat_docs: Vec<Doc> = pats.iter().map(format_node).collect();
        parts.push(Doc::text("("));
        parts.push(Doc::join(
            pat_docs,
            Doc::concat(vec![Doc::text(","), Doc::text(" ")]),
        ));
        parts.push(Doc::text(")"));
    }

    Doc::concat(parts)
}

fn format_literal_pat(node: &SyntaxNode) -> Doc {
    let tok = non_trivia_tokens(node).into_iter().next();
    if let Some(t) = tok {
        format_token(&t)
    } else {
        verbatim(node)
    }
}

fn format_record_pat(node: &SyntaxNode) -> Doc {
    let mut parts = Vec::new();
    if let Some(path) = find_child_node(node, SyntaxKind::Path) {
        parts.push(format_node(&path));
    }
    parts.push(Doc::text(" { "));
    let fields: Vec<SyntaxToken> = node
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .filter(|tok| tok.kind() == SyntaxKind::Ident)
        .collect();
    // Skip the first ident if it's part of the path
    let has_path = find_child_node(node, SyntaxKind::Path).is_some();
    let field_docs: Vec<Doc> = if has_path {
        // The path captures its own idents, so grab remaining idents from direct tokens
        // Actually the path is a child node, so its idents won't show up as direct
        // tokens of the RecordPat. We need all ident tokens that are direct children.
        let direct_idents: Vec<SyntaxToken> = node
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::Ident)
            .collect();
        direct_idents.iter().map(format_token).collect()
    } else {
        fields.iter().map(format_token).collect()
    };
    parts.push(Doc::join(
        field_docs,
        Doc::concat(vec![Doc::text(","), Doc::text(" ")]),
    ));
    parts.push(Doc::text(" }"));
    Doc::concat(parts)
}

fn format_pat_list(node: &SyntaxNode) -> Doc {
    let pats: Vec<SyntaxNode> = node.children().filter(|c| is_pat(c.kind())).collect();
    let pat_docs: Vec<Doc> = pats.iter().map(format_node).collect();
    Doc::join(pat_docs, Doc::concat(vec![Doc::text(","), Doc::text(" ")]))
}
