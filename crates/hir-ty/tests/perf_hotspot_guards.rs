#[test]
fn infer_expr_and_pat_hot_paths_do_not_clone_whole_nodes() {
    let infer_expr_src = include_str!("../src/infer/expr.rs");
    let infer_pat_src = include_str!("../src/infer/pat.rs");

    assert!(
        !infer_expr_src.contains("self.body.exprs[idx].clone()"),
        "infer_expr_inner should borrow Expr instead of cloning whole node"
    );
    assert!(
        !infer_expr_src.contains("self.body.pats[pat_idx].clone()"),
        "is_irrefutable_let_pattern should borrow Pat instead of cloning whole node"
    );
    assert!(
        !infer_pat_src.contains("self.body.pats[pat_idx].clone()"),
        "infer_pat should borrow Pat instead of cloning whole node"
    );
}
