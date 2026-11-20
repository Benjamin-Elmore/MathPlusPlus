//! The step log ("show the working"): each rule the CAS applies can record a step.

use crate::expr::Expr;

/// One step of working.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// Name of the rule, such as "power rule" or "substitution".
    pub rule: String,
    /// The expression before the rule.
    pub before: Expr,
    /// The expression after the rule.
    pub after: Expr,
    /// Optional extra detail, such as `u = x^2, du = 2x dx`.
    pub note: Option<String>,
}

/// An ordered log of steps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Steps(pub Vec<Step>);

impl Steps {
    /// An empty log.
    pub fn new() -> Steps {
        Steps(Vec::new())
    }

    /// Appends a step.
    pub fn push(
        &mut self,
        rule: impl Into<String>,
        before: Expr,
        after: Expr,
        note: Option<String>,
    ) {
        self.0.push(Step {
            rule: rule.into(),
            before,
            after,
            note,
        });
    }

    /// Records a step when a log is present (the form CAS functions take).
    pub fn record(
        log: &mut Option<&mut Steps>,
        rule: impl Into<String>,
        before: &Expr,
        after: &Expr,
        note: Option<String>,
    ) {
        if let Some(steps) = log.as_deref_mut() {
            steps.push(rule, before.clone(), after.clone(), note);
        }
    }

    /// Number of steps.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates over the steps in order.
    pub fn iter(&self) -> std::slice::Iter<'_, Step> {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a Steps {
    type Item = &'a Step;
    type IntoIter = std::slice::Iter<'a, Step>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_only_when_logging() {
        let x = Expr::sym("x");
        let mut steps = Steps::new();
        let mut log = Some(&mut steps);
        Steps::record(
            &mut log,
            "power rule",
            &x.pow(2),
            &(Expr::int(2) * x.clone()),
            None,
        );
        let mut none: Option<&mut Steps> = None;
        Steps::record(&mut none, "ignored", &x, &x, None);
        assert_eq!(steps.len(), 1);
        assert_eq!(
            steps.iter().next().map(|s| s.rule.as_str()),
            Some("power rule")
        );
        assert!(!steps.is_empty());
    }
}
