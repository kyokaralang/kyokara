use crate::expr::CallArg;
use crate::item_tree::FnParam;
use crate::name::Name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallShapeError {
    ArgCountMismatch { expected: usize, actual: usize },
    UnknownNamedArg { name: Name },
    DuplicateNamedArg { name: Name },
    PositionalAfterNamedArg,
    MissingArg { name: Name },
    NamedOnlyArg { name: Name },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallArgBinding {
    pub arg_to_param: Vec<Option<usize>>,
    pub param_to_arg: Vec<Option<usize>>,
    pub errors: Vec<CallShapeError>,
}

impl CallArgBinding {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallFamilySelection<T> {
    Selected { candidate: T, binding: CallArgBinding },
    InvalidShape { errors: Vec<CallShapeError> },
    ArgCountMismatch { expected: Vec<usize>, actual: usize },
    Ambiguous,
}

pub fn bind_call_args_to_params(args: &[CallArg], params: &[FnParam]) -> CallArgBinding {
    let mut binding = CallArgBinding {
        arg_to_param: vec![None; args.len()],
        param_to_arg: vec![None; params.len()],
        errors: Vec::new(),
    };

    if args.len() != params.len() {
        binding.errors.push(CallShapeError::ArgCountMismatch {
            expected: params.len(),
            actual: args.len(),
        });
        return binding;
    }

    let mut next_pos = 0usize;
    let mut saw_named = false;
    let mut saw_shape_error = false;
    let mut suppress_missing_args = false;

    for (arg_idx, arg) in args.iter().enumerate() {
        match arg {
            CallArg::Positional(_) => {
                if saw_named {
                    binding.errors.push(CallShapeError::PositionalAfterNamedArg);
                    saw_shape_error = true;
                    suppress_missing_args = true;
                    continue;
                }
                while next_pos < binding.param_to_arg.len() && binding.param_to_arg[next_pos].is_some()
                {
                    next_pos += 1;
                }
                if next_pos >= params.len() {
                    saw_shape_error = true;
                    continue;
                }
                if params[next_pos].named_only {
                    binding.errors.push(CallShapeError::NamedOnlyArg {
                        name: params[next_pos].name,
                    });
                    saw_shape_error = true;
                    suppress_missing_args = true;
                    next_pos += 1;
                    continue;
                }
                binding.arg_to_param[arg_idx] = Some(next_pos);
                binding.param_to_arg[next_pos] = Some(arg_idx);
                next_pos += 1;
            }
            CallArg::Named { name, .. } => {
                saw_named = true;
                let Some(param_idx) = params.iter().position(|param| param.name == *name) else {
                    binding
                        .errors
                        .push(CallShapeError::UnknownNamedArg { name: *name });
                    saw_shape_error = true;
                    continue;
                };
                if binding.param_to_arg[param_idx].is_some() {
                    binding
                        .errors
                        .push(CallShapeError::DuplicateNamedArg { name: *name });
                    saw_shape_error = true;
                    continue;
                }
                binding.arg_to_param[arg_idx] = Some(param_idx);
                binding.param_to_arg[param_idx] = Some(arg_idx);
            }
        }
    }

    if !saw_shape_error || !suppress_missing_args {
        for (param_idx, arg_idx) in binding.param_to_arg.iter().enumerate() {
            if arg_idx.is_none() {
                binding.errors.push(CallShapeError::MissingArg {
                    name: params[param_idx].name,
                });
            }
        }
    }

    binding
}

pub fn call_shapes_overlap(lhs: &[FnParam], rhs: &[FnParam]) -> bool {
    if lhs.len() != rhs.len() {
        return false;
    }

    (0..=lhs.len()).any(|positional_prefix_len| {
        let lhs_prefix_ok = lhs[..positional_prefix_len]
            .iter()
            .all(|param| !param.named_only);
        let rhs_prefix_ok = rhs[..positional_prefix_len]
            .iter()
            .all(|param| !param.named_only);
        if !lhs_prefix_ok || !rhs_prefix_ok {
            return false;
        }

        let mut lhs_named: Vec<Name> = lhs[positional_prefix_len..]
            .iter()
            .map(|param| param.name)
            .collect();
        let mut rhs_named: Vec<Name> = rhs[positional_prefix_len..]
            .iter()
            .map(|param| param.name)
            .collect();
        lhs_named.sort_unstable_by_key(|name| name.0);
        rhs_named.sort_unstable_by_key(|name| name.0);
        lhs_named == rhs_named
    })
}

pub fn select_call_family_candidate<'a, T, F>(
    args: &[CallArg],
    candidates: &[T],
    mut params_for: F,
) -> CallFamilySelection<T>
where
    T: Copy,
    F: FnMut(T) -> &'a [FnParam],
{
    let mut successes = Vec::new();
    let mut shape_failures: Vec<Vec<CallShapeError>> = Vec::new();
    let mut expected = Vec::new();

    for &candidate in candidates {
        let binding = bind_call_args_to_params(args, params_for(candidate));
        if binding.is_valid() {
            successes.push((candidate, binding));
            continue;
        }

        if binding.errors.len() == 1
            && let CallShapeError::ArgCountMismatch { expected: want, .. } = binding.errors[0]
        {
            expected.push(want);
        } else {
            shape_failures.push(binding.errors);
        }
    }

    match successes.len() {
        1 => {
            let (candidate, binding) = successes.pop().expect("len checked above");
            CallFamilySelection::Selected { candidate, binding }
        }
        n if n > 1 => CallFamilySelection::Ambiguous,
        _ if !shape_failures.is_empty() => {
            shape_failures.sort_by_key(Vec::len);
            CallFamilySelection::InvalidShape {
                errors: shape_failures.remove(0),
            }
        }
        _ => {
            expected.sort_unstable();
            expected.dedup();
            CallFamilySelection::ArgCountMismatch {
                expected,
                actual: args.len(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use kyokara_intern::Interner;

    use crate::expr::CallArg;
    use crate::item_tree::FnParam;
    use crate::name::Name;
    use crate::type_ref::TypeRef;

    use super::{
        CallFamilySelection, CallShapeError, bind_call_args_to_params, call_shapes_overlap,
        select_call_family_candidate,
    };

    fn int_ty(interner: &mut Interner) -> TypeRef {
        TypeRef::Path {
            path: crate::path::Path::single(Name::new(interner, "Int")),
            args: Vec::new(),
        }
    }

    fn param(interner: &mut Interner, name: &str) -> FnParam {
        FnParam {
            name: Name::new(interner, name),
            ty: int_ty(interner),
            named_only: false,
        }
    }

    fn named_only_param(interner: &mut Interner, name: &str) -> FnParam {
        FnParam {
            name: Name::new(interner, name),
            ty: int_ty(interner),
            named_only: true,
        }
    }

    fn pos(idx: usize) -> CallArg {
        CallArg::Positional(crate::expr::ExprIdx::from_raw((idx as u32).into()))
    }

    fn named(interner: &mut Interner, name: &str, idx: usize) -> CallArg {
        CallArg::Named {
            name: Name::new(interner, name),
            value: crate::expr::ExprIdx::from_raw((idx as u32).into()),
        }
    }

    #[test]
    fn bind_rejects_positional_for_named_only_param() {
        let mut interner = Interner::default();
        let params = vec![param(&mut interner, "prefix"), named_only_param(&mut interner, "start")];
        let binding = bind_call_args_to_params(&[pos(0), pos(1)], &params);
        assert_eq!(
            binding.errors,
            vec![CallShapeError::NamedOnlyArg {
                name: Name::new(&mut interner, "start")
            }]
        );
    }

    #[test]
    fn bind_accepts_named_only_param_when_named() {
        let mut interner = Interner::default();
        let params = vec![param(&mut interner, "prefix"), named_only_param(&mut interner, "start")];
        let binding = bind_call_args_to_params(
            &[pos(0), named(&mut interner, "start", 1)],
            &params,
        );
        assert!(binding.errors.is_empty(), "{binding:?}");
        assert_eq!(binding.param_to_arg, vec![Some(0), Some(1)]);
    }

    #[test]
    fn family_selects_by_named_arg_presence() {
        let mut interner = Interner::default();
        let prefix_only = vec![param(&mut interner, "prefix")];
        let with_offset = vec![param(&mut interner, "prefix"), named_only_param(&mut interner, "start")];
        let families = [0usize, 1usize];
        let args = [pos(0), named(&mut interner, "start", 1)];
        let selection = select_call_family_candidate(&args, &families, |candidate| match candidate {
            0 => prefix_only.as_slice(),
            1 => with_offset.as_slice(),
            _ => unreachable!(),
        });
        assert!(matches!(
            selection,
            CallFamilySelection::Selected { candidate: 1, .. }
        ));
    }

    #[test]
    fn family_reports_shape_error_before_family_arity_error() {
        let mut interner = Interner::default();
        let prefix_only = vec![param(&mut interner, "prefix")];
        let with_offset = vec![param(&mut interner, "prefix"), named_only_param(&mut interner, "start")];
        let families = [0usize, 1usize];
        let args = [pos(0), pos(1)];
        let selection = select_call_family_candidate(&args, &families, |candidate| match candidate {
            0 => prefix_only.as_slice(),
            1 => with_offset.as_slice(),
            _ => unreachable!(),
        });
        assert_eq!(
            selection,
            CallFamilySelection::InvalidShape {
                errors: vec![CallShapeError::NamedOnlyArg {
                    name: Name::new(&mut interner, "start"),
                }],
            }
        );
    }

    #[test]
    fn family_reports_arity_choices_when_no_shape_matches() {
        let mut interner = Interner::default();
        let zero = Vec::<FnParam>::new();
        let one = vec![param(&mut interner, "predicate")];
        let families = [0usize, 1usize];
        let args = [pos(0), pos(1)];
        let selection = select_call_family_candidate(&args, &families, |candidate| match candidate {
            0 => zero.as_slice(),
            1 => one.as_slice(),
            _ => unreachable!(),
        });
        assert_eq!(
            selection,
            CallFamilySelection::ArgCountMismatch {
                expected: vec![0, 1],
                actual: 2,
            }
        );
    }

    #[test]
    fn family_marks_ambiguous_same_shape_candidates() {
        let mut interner = Interner::default();
        let one = vec![param(&mut interner, "x")];
        let families = [0usize, 1usize];
        let args = [pos(0)];
        let selection = select_call_family_candidate(&args, &families, |_| one.as_slice());
        assert_eq!(selection, CallFamilySelection::Ambiguous);
    }

    #[test]
    fn overlap_detects_same_shape_even_when_names_are_the_same() {
        let mut interner = Interner::default();
        let lhs = vec![param(&mut interner, "x"), param(&mut interner, "y")];
        let rhs = vec![param(&mut interner, "x"), param(&mut interner, "y")];
        assert!(call_shapes_overlap(&lhs, &rhs));
    }

    #[test]
    fn overlap_rejects_families_distinguished_only_by_types() {
        let mut interner = Interner::default();
        let lhs = vec![param(&mut interner, "x")];
        let rhs = vec![param(&mut interner, "x")];
        assert!(call_shapes_overlap(&lhs, &rhs));
    }

    #[test]
    fn overlap_allows_arity_distinct_families() {
        let mut interner = Interner::default();
        let lhs = vec![];
        let rhs = vec![param(&mut interner, "x")];
        assert!(!call_shapes_overlap(&lhs, &rhs));
    }

    #[test]
    fn overlap_allows_distinct_named_suffix_shapes() {
        let mut interner = Interner::default();
        let lhs = vec![param(&mut interner, "prefix"), named_only_param(&mut interner, "start")];
        let rhs = vec![param(&mut interner, "prefix"), named_only_param(&mut interner, "offset")];
        assert!(!call_shapes_overlap(&lhs, &rhs));
    }

    #[test]
    fn overlap_blocks_positional_and_named_only_same_arity_family() {
        let mut interner = Interner::default();
        let lhs = vec![param(&mut interner, "x"), param(&mut interner, "y")];
        let rhs = vec![param(&mut interner, "x"), named_only_param(&mut interner, "y")];
        assert!(call_shapes_overlap(&lhs, &rhs));
    }
}
