//! Walking every expression and pattern in a syntax tree.

use super::expression::{Block, Expression, ExpressionKind, Pattern, PatternKind, VariantFields};
use super::typeinfo::TypeInfo;

#[derive(Debug, Clone, Copy)]
pub enum Node<'a> {
    Expression(&'a Expression),
    Pattern(&'a Pattern),
}

/// Calls `visit` on every expression and pattern in `expressions` and
/// everything nested in them, parents before children.
pub fn walk<'a>(
    expressions: impl IntoIterator<Item = &'a Expression>,
    visit: &mut impl FnMut(Node<'a>),
) {
    for expression in expressions {
        walk_expression(expression, visit);
    }
}

fn walk_block<'a>(block: &'a Block, visit: &mut impl FnMut(Node<'a>)) {
    walk(block.body.iter().chain(block.result.as_deref()), visit);
}

/// Types contain expressions only as array initializers, like `u8[4]{0}`.
fn walk_type<'a>(typeinfo: &'a TypeInfo, visit: &mut impl FnMut(Node<'a>)) {
    match typeinfo {
        TypeInfo::NamedType { .. } => {}
        TypeInfo::PointerType { pointee: inner }
        | TypeInfo::ReferenceType { referent: inner }
        | TypeInfo::IteratorType { item: inner } => walk_type(inner, visit),
        TypeInfo::ArrayType {
            element,
            initializer,
            ..
        } => {
            walk_type(element, visit);
            if let Some(initializer) = initializer {
                walk_expression(initializer, visit);
            }
        }
    }
}

fn walk_pattern<'a>(pattern: &'a Pattern, visit: &mut impl FnMut(Node<'a>)) {
    visit(Node::Pattern(pattern));
    match &pattern.kind {
        PatternKind::WildcardPattern | PatternKind::BindingPattern { .. } => {}
        PatternKind::ValuePattern { value } => walk_expression(value, visit),
        PatternKind::TupleVariantPattern { elements, .. } => {
            for element in elements {
                walk_pattern(element, visit);
            }
        }
        PatternKind::StructVariantPattern { fields, .. } => {
            for field in fields {
                walk_pattern(&field.pattern, visit);
            }
        }
    }
}

fn walk_expression<'a>(expression: &'a Expression, visit: &mut impl FnMut(Node<'a>)) {
    visit(Node::Expression(expression));

    let mut each = |expression: &'a Expression| walk_expression(expression, visit);

    match &expression.kind {
        ExpressionKind::ImportExpression { .. }
        | ExpressionKind::StringLiteralExpression { .. }
        | ExpressionKind::IntegerLiteralExpression { .. }
        | ExpressionKind::FloatLiteralExpression { .. }
        | ExpressionKind::BooleanLiteralExpression { .. }
        | ExpressionKind::IdentifierExpression { .. }
        | ExpressionKind::AtExpression
        | ExpressionKind::BreakExpression
        | ExpressionKind::ContinueExpression => {}
        ExpressionKind::TypeExpression { typeinfo } => walk_type(typeinfo, visit),
        ExpressionKind::FunctionDeclarationExpression {
            params,
            return_type,
            body,
            ..
        } => {
            for param in params {
                walk_type(&param.r#type, visit);
            }
            if let Some(return_type) = return_type {
                walk_type(return_type, visit);
            }
            walk_block(body, visit);
        }
        ExpressionKind::LetExpression {
            typeinfo, value, ..
        } => {
            if let Some(typeinfo) = typeinfo {
                walk_type(typeinfo, visit);
            }
            if let Some(value) = value {
                walk_expression(value, visit);
            }
        }
        ExpressionKind::StructDeclarationExpression { fields, .. } => {
            for field in fields {
                walk_type(&field.r#type, visit);
            }
        }
        ExpressionKind::EnumDeclarationExpression {
            backing_type,
            variants,
            ..
        } => {
            if let Some(backing_type) = backing_type {
                walk_type(backing_type, visit);
            }
            for variant in variants {
                match &variant.fields {
                    VariantFields::Unit => {}
                    VariantFields::Tuple { types } => {
                        for typeinfo in types {
                            walk_type(typeinfo, visit);
                        }
                    }
                    VariantFields::Struct { fields } => {
                        for field in fields {
                            walk_type(&field.r#type, visit);
                        }
                    }
                }
                if let Some(value) = &variant.value {
                    walk_expression(value, visit);
                }
            }
        }
        ExpressionKind::StructLiteralExpression { fields, .. } => {
            for field in fields {
                each(&field.value);
            }
        }
        ExpressionKind::BlockExpression { block } => walk_block(block, visit),
        ExpressionKind::AssignmentExpression { target, value, .. } => {
            each(target);
            each(value);
        }
        ExpressionKind::CastExpression { value, typeinfo } => {
            each(value);
            walk_type(typeinfo, visit);
        }
        ExpressionKind::BinaryExpression { left, right, .. } => {
            each(left);
            each(right);
        }
        ExpressionKind::UnaryExpression { operand, .. } => each(operand),
        ExpressionKind::CallExpression { callee, args } => {
            each(callee);
            for arg in args {
                each(arg);
            }
        }
        ExpressionKind::IndexExpression { target, index } => {
            each(target);
            each(index);
        }
        ExpressionKind::MemberExpression { target, .. } => each(target),
        ExpressionKind::IncrementExpression { target, .. } => each(target),
        ExpressionKind::IfExpression {
            condition,
            then,
            otherwise,
        } => {
            each(condition);
            walk_block(then, visit);
            if let Some(otherwise) = otherwise {
                walk_expression(otherwise, visit);
            }
        }
        ExpressionKind::SwitchExpression {
            subject,
            cases,
            default,
        } => {
            each(subject);
            for case in cases {
                walk_pattern(&case.pattern, visit);
                walk_block(&case.body, visit);
            }
            if let Some(default) = default {
                walk_block(default, visit);
            }
        }
        ExpressionKind::WhileExpression { condition, body } => {
            each(condition);
            walk_block(body, visit);
        }
        ExpressionKind::LoopExpression { body } => walk_block(body, visit),
        ExpressionKind::DoExpression { body, condition } => {
            walk_block(body, visit);
            if let Some(condition) = condition {
                walk_expression(condition, visit);
            }
        }
        ExpressionKind::ForExpression { iterable, body, .. } => {
            each(iterable);
            walk_block(body, visit);
        }
        ExpressionKind::ForClauseExpression {
            init,
            condition,
            step,
            each: item,
            body,
        } => {
            for clause in [init, condition, step, item].into_iter().flatten() {
                walk_expression(clause, visit);
            }
            walk_block(body, visit);
        }
        ExpressionKind::ReturnExpression { value } => {
            if let Some(value) = value {
                each(value);
            }
        }
        ExpressionKind::YieldExpression { value } => each(value),
    }
}
