//! Bridge: convert parser events + raw tokens into a rowan `GreenNode`.
//!
//! This module resolves forward-parent pointers, attaches trivia tokens,
//! and builds the green tree in a single pass.

use kyokara_parser::{Event, SyntaxKind};
use rowan::GreenNodeBuilder;

use crate::lexer::LexToken;

/// Build a rowan `GreenNode` from parser events and raw tokens.
pub fn build_tree(events: Vec<Event>, tokens: &[LexToken<'_>]) -> rowan::GreenNode {
    // Step 1: Resolve forward_parent chains.
    let events = resolve_forward_parents(events);

    // Step 2: Walk events and tokens together, building the green tree.
    let mut builder = GreenNodeBuilder::new();
    let mut raw_pos: usize = 0;
    let mut depth: u32 = 0;

    for event in &events {
        match event {
            Event::StartNode { kind, .. } => {
                // Attach leading trivia before opening the node (except for
                // the root SourceFile which wraps everything).
                if *kind != SyntaxKind::SourceFile {
                    attach_trivia(&mut builder, tokens, &mut raw_pos);
                }
                builder.start_node(rowan::SyntaxKind(*kind as u16));
                depth += 1;
            }
            Event::FinishNode => {
                // Before closing the root node, attach any remaining trivia
                // so it stays inside the SourceFile.
                if depth == 1 {
                    attach_trivia(&mut builder, tokens, &mut raw_pos);
                }
                builder.finish_node();
                depth -= 1;
            }
            Event::Token { n_raw_tokens, .. } => {
                // Attach leading trivia tokens first.
                attach_trivia(&mut builder, tokens, &mut raw_pos);
                // Then attach the actual token(s).
                for _ in 0..*n_raw_tokens {
                    if raw_pos < tokens.len() {
                        let tok = &tokens[raw_pos];
                        builder.token(rowan::SyntaxKind(tok.kind as u16), tok.text);
                        raw_pos += 1;
                    }
                }
            }
            Event::Error { .. } => {
                // Errors are collected separately; we don't need them in the tree.
            }
            Event::Tombstone => {
                // Skip — these are placeholders.
            }
        }
    }

    builder.finish()
}

/// Attach consecutive trivia tokens (whitespace, comments) to the tree.
fn attach_trivia(builder: &mut GreenNodeBuilder, tokens: &[LexToken<'_>], pos: &mut usize) {
    while *pos < tokens.len() && tokens[*pos].kind.is_trivia() {
        let tok = &tokens[*pos];
        builder.token(rowan::SyntaxKind(tok.kind as u16), tok.text);
        *pos += 1;
    }
}

/// Resolve forward-parent chains by reordering StartNode events.
///
/// When `CompletedMarker::precede()` is used (e.g., for Pratt parsing),
/// the original `StartNode` gets a `forward_parent` offset pointing to
/// the *real* parent. We resolve these into the correct nesting order
/// so the tree builder can process events linearly.
fn resolve_forward_parents(mut events: Vec<Event>) -> Vec<Event> {
    let mut result = Vec::with_capacity(events.len());

    for i in 0..events.len() {
        match &events[i] {
            Event::StartNode {
                forward_parent: Some(_),
                ..
            } => {
                // This node's start has been forwarded — collect the chain.
                let mut chain = Vec::new();
                let mut idx = i;
                while let Event::StartNode {
                    kind,
                    forward_parent,
                } = events[idx].clone()
                {
                    // Consume only valid StartNode entries.
                    events[idx] = Event::Tombstone;
                    chain.push(kind);

                    if let Some(delta) = forward_parent {
                        let Some(next_idx) = idx.checked_add(delta as usize) else {
                            break;
                        };
                        if next_idx >= events.len() {
                            break;
                        }
                        idx = next_idx;
                    } else {
                        break;
                    }
                }
                // Emit in reverse order (outermost parent first).
                for kind in chain.into_iter().rev() {
                    result.push(Event::StartNode {
                        kind,
                        forward_parent: None,
                    });
                }
            }
            Event::Tombstone => {
                // Skip tombstones (already consumed by forward_parent resolution
                // or abandoned markers).
            }
            _ => {
                result.push(events[i].clone());
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::resolve_forward_parents;
    use kyokara_parser::{Event, SyntaxKind};

    #[test]
    fn resolve_forward_parents_ignores_out_of_bounds_forward_parent() {
        let events = vec![
            Event::StartNode {
                kind: SyntaxKind::SourceFile,
                forward_parent: Some(100),
            },
            Event::FinishNode,
        ];

        let resolved = resolve_forward_parents(events);
        assert_eq!(resolved.len(), 2, "should preserve valid events");
        assert!(matches!(
            resolved[0],
            Event::StartNode {
                kind: SyntaxKind::SourceFile,
                forward_parent: None
            }
        ));
        assert!(matches!(resolved[1], Event::FinishNode));
    }

    #[test]
    fn resolve_forward_parents_ignores_non_startnode_forward_target() {
        let events = vec![
            Event::StartNode {
                kind: SyntaxKind::SourceFile,
                forward_parent: Some(1),
            },
            Event::Token {
                kind: SyntaxKind::Ident,
                n_raw_tokens: 0,
            },
            Event::FinishNode,
        ];

        let resolved = resolve_forward_parents(events);
        assert_eq!(resolved.len(), 3, "should keep non-start events intact");
        assert!(matches!(
            resolved[0],
            Event::StartNode {
                kind: SyntaxKind::SourceFile,
                forward_parent: None
            }
        ));
        assert!(matches!(
            resolved[1],
            Event::Token {
                kind: SyntaxKind::Ident,
                n_raw_tokens: 0
            }
        ));
        assert!(matches!(resolved[2], Event::FinishNode));
    }
}
