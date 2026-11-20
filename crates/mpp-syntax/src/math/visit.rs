//! Walking the children of a node.

use crate::math::ast::{CaseCondition, Node, NodeKind};

/// Calls `f` on every direct child of `node`, in source order.
pub fn children<'n>(node: &'n Node, f: &mut dyn FnMut(&'n Node)) {
    use NodeKind as K;
    match &node.kind {
        K::Number(_)
        | K::Constant(_)
        | K::Symbol(_)
        | K::Dots(_)
        | K::Text(_)
        | K::Continuation
        | K::Error => {}
        K::Name(n) => {
            if let Some(sub) = &n.sub {
                f(sub);
            }
        }
        K::Call { callee, args, .. } => {
            f(callee);
            args.iter().for_each(f);
        }
        K::FuncApp(app) => {
            app.sub.iter().for_each(&mut *f);
            app.power.iter().for_each(&mut *f);
            app.args.iter().for_each(f);
        }
        K::Unary { operand, .. } => f(operand),
        K::Binary { lhs, rhs, .. } => {
            f(lhs);
            f(rhs);
        }
        K::Frac { num, den, .. } => {
            f(num);
            f(den);
        }
        K::Pow { base, exp } => {
            f(base);
            f(exp);
        }
        K::Subscript { base, sub } => {
            f(base);
            f(sub);
        }
        K::Prime { base, .. } => f(base),
        K::Factorial { operand, .. } => f(operand),
        K::Fenced { inner, .. }
        | K::Group { inner, .. }
        | K::Accented { inner, .. }
        | K::Styled { inner, .. } => f(inner),
        K::Quantity { value, .. } => f(value),
        K::Convert { expr, .. } => f(expr),
        K::Sqrt {
            index, radicand, ..
        } => {
            if let Some(i) = index {
                f(i);
            }
            f(radicand);
        }
        K::Binom { n, k, .. } => {
            f(n);
            f(k);
        }
        K::Matrix { rows, .. } => rows.iter().flatten().for_each(f),
        K::Cases { rows } => {
            for row in rows {
                f(&row.value);
                if let CaseCondition::When(c) = &row.condition {
                    f(c);
                }
            }
        }
        K::Set { elems, .. } | K::Tuple { elems, .. } => elems.iter().for_each(f),
        K::Interval { lo, hi, .. } => {
            f(lo);
            f(hi);
        }
        K::BigOp(op) => {
            for n in [&op.index, &op.from, &op.to, &op.over]
                .into_iter()
                .flatten()
            {
                f(n);
            }
            f(&op.body);
        }
        K::Integral(int) => {
            for n in [&int.lower, &int.upper].into_iter().flatten() {
                f(n);
            }
            f(&int.body);
            int.diffs.iter().for_each(f);
        }
        K::Limit(lim) => {
            f(&lim.var);
            f(&lim.target);
            f(&lim.body);
        }
        K::Derivative(d) => {
            if let Some(o) = &d.order {
                f(o);
            }
            for v in &d.vars {
                f(&v.var);
                if let Some(p) = &v.power {
                    f(p);
                }
            }
            if let Some(of) = &d.of {
                f(of);
            }
        }
        K::Nabla { body, .. } => {
            if let Some(b) = body {
                f(b);
            }
        }
        K::NamedOp(op) => {
            op.sub.iter().for_each(&mut *f);
            op.sup.iter().for_each(&mut *f);
            op.args.iter().for_each(f);
        }
        K::Relation { first, rest } => {
            f(first);
            rest.iter().for_each(|(_, n)| f(n));
        }
    }
}

/// Calls `f` on every direct child of `node`, mutably.
pub fn children_mut(node: &mut Node, f: &mut dyn FnMut(&mut Node)) {
    use NodeKind as K;
    match &mut node.kind {
        K::Number(_)
        | K::Constant(_)
        | K::Symbol(_)
        | K::Dots(_)
        | K::Text(_)
        | K::Continuation
        | K::Error => {}
        K::Name(n) => {
            if let Some(sub) = &mut n.sub {
                f(sub);
            }
        }
        K::Call { callee, args, .. } => {
            f(callee);
            args.iter_mut().for_each(f);
        }
        K::FuncApp(app) => {
            app.sub.iter_mut().for_each(&mut *f);
            app.power.iter_mut().for_each(&mut *f);
            app.args.iter_mut().for_each(f);
        }
        K::Unary { operand, .. } => f(operand),
        K::Binary { lhs, rhs, .. } => {
            f(lhs);
            f(rhs);
        }
        K::Frac { num, den, .. } => {
            f(num);
            f(den);
        }
        K::Pow { base, exp } => {
            f(base);
            f(exp);
        }
        K::Subscript { base, sub } => {
            f(base);
            f(sub);
        }
        K::Prime { base, .. } => f(base),
        K::Factorial { operand, .. } => f(operand),
        K::Fenced { inner, .. }
        | K::Group { inner, .. }
        | K::Accented { inner, .. }
        | K::Styled { inner, .. } => f(inner),
        K::Quantity { value, .. } => f(value),
        K::Convert { expr, .. } => f(expr),
        K::Sqrt {
            index, radicand, ..
        } => {
            if let Some(i) = index {
                f(i);
            }
            f(radicand);
        }
        K::Binom { n, k, .. } => {
            f(n);
            f(k);
        }
        K::Matrix { rows, .. } => rows.iter_mut().flatten().for_each(f),
        K::Cases { rows } => {
            for row in rows {
                f(&mut row.value);
                if let CaseCondition::When(c) = &mut row.condition {
                    f(c);
                }
            }
        }
        K::Set { elems, .. } | K::Tuple { elems, .. } => elems.iter_mut().for_each(f),
        K::Interval { lo, hi, .. } => {
            f(lo);
            f(hi);
        }
        K::BigOp(op) => {
            for n in [&mut op.index, &mut op.from, &mut op.to, &mut op.over]
                .into_iter()
                .flatten()
            {
                f(n);
            }
            f(&mut op.body);
        }
        K::Integral(int) => {
            for n in [&mut int.lower, &mut int.upper].into_iter().flatten() {
                f(n);
            }
            f(&mut int.body);
            int.diffs.iter_mut().for_each(f);
        }
        K::Limit(lim) => {
            f(&mut lim.var);
            f(&mut lim.target);
            f(&mut lim.body);
        }
        K::Derivative(d) => {
            if let Some(o) = &mut d.order {
                f(o);
            }
            for v in &mut d.vars {
                f(&mut v.var);
                if let Some(p) = &mut v.power {
                    f(p);
                }
            }
            if let Some(of) = &mut d.of {
                f(of);
            }
        }
        K::Nabla { body, .. } => {
            if let Some(b) = body {
                f(b);
            }
        }
        K::NamedOp(op) => {
            op.sub.iter_mut().for_each(&mut *f);
            op.sup.iter_mut().for_each(&mut *f);
            op.args.iter_mut().for_each(f);
        }
        K::Relation { first, rest } => {
            f(first);
            rest.iter_mut().for_each(|(_, n)| f(n));
        }
    }
}
