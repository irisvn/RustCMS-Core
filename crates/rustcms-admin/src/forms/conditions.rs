//! Conditions under which an input or a layout is shown (ADR 0037).
//!
//! ```rust,ignore
//! TextInput::make("url").shown_when(when("link_type").eq("external")),
//! Section::make("Discount").shown_when(when("on_sale").is_true()).schema(components![/* … */]),
//! TextInput::make("code").shown_when(Condition::any([when("a").filled(), when("b").is_true()])),
//! ```
//!
//! Conditions are data: the SPA evaluates them on every change (no request),
//! the server again on every write, where it decides.
//!
//! In a repeater's rows, `when` names an input of the same row; `when_root`
//! an input of the record's form and `when_parent` one of the enclosing row
//! (ADR 0044).

use serde::Serialize;
use serde_json::Value;

/// When an input or a layout is shown: a test of another input's value, or
/// several combined. Serialized as `{"op": "eq", "name": …, "value": …}`,
/// `{"op": "all", "conditions": [...]}`, … (vocabulary 13).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Condition {
    /// The value equals `value` (in canonical wire form).
    Eq {
        /// Input name.
        name: String,
        /// Where the input is (absent: the same level).
        #[serde(default, skip_serializing_if = "Scope::is_row")]
        #[cfg_attr(feature = "ts", ts(as = "Option<Scope>", optional))]
        scope: Scope,
        /// Compared value.
        value: Value,
    },
    /// The value differs from `value`.
    Ne {
        /// Input name.
        name: String,
        /// Where the input is (absent: the same level).
        #[serde(default, skip_serializing_if = "Scope::is_row")]
        #[cfg_attr(feature = "ts", ts(as = "Option<Scope>", optional))]
        scope: Scope,
        /// Compared value.
        value: Value,
    },
    /// The value is one of `values`.
    IsIn {
        /// Input name.
        name: String,
        /// Where the input is (absent: the same level).
        #[serde(default, skip_serializing_if = "Scope::is_row")]
        #[cfg_attr(feature = "ts", ts(as = "Option<Scope>", optional))]
        scope: Scope,
        /// Allowed values.
        values: Vec<Value>,
    },
    /// The value is set: not `null`, `""`, `[]` or `{}`.
    Filled {
        /// Input name.
        name: String,
        /// Where the input is (absent: the same level).
        #[serde(default, skip_serializing_if = "Scope::is_row")]
        #[cfg_attr(feature = "ts", ts(as = "Option<Scope>", optional))]
        scope: Scope,
    },
    /// The value is `true`.
    IsTrue {
        /// Input name.
        name: String,
        /// Where the input is (absent: the same level).
        #[serde(default, skip_serializing_if = "Scope::is_row")]
        #[cfg_attr(feature = "ts", ts(as = "Option<Scope>", optional))]
        scope: Scope,
    },
    /// Every condition holds.
    All {
        /// Combined conditions.
        conditions: Vec<Condition>,
    },
    /// At least one condition holds.
    Any {
        /// Combined conditions.
        conditions: Vec<Condition>,
    },
}

impl Condition {
    /// Every condition holds.
    pub fn all(conditions: impl IntoIterator<Item = Condition>) -> Self {
        Self::All { conditions: conditions.into_iter().collect() }
    }

    /// At least one condition holds.
    pub fn any(conditions: impl IntoIterator<Item = Condition>) -> Self {
        Self::Any { conditions: conditions.into_iter().collect() }
    }

    /// Input names this condition reads, with where they are, depth first,
    /// possibly repeated.
    pub(crate) fn names(&self) -> Vec<(Scope, &str)> {
        let mut out = Vec::new();
        self.collect_names(&mut out);
        out
    }

    fn collect_names<'a>(&'a self, out: &mut Vec<(Scope, &'a str)>) {
        match self {
            Self::Eq { name, scope, .. }
            | Self::Ne { name, scope, .. }
            | Self::IsIn { name, scope, .. }
            | Self::Filled { name, scope }
            | Self::IsTrue { name, scope } => out.push((*scope, name)),
            Self::All { conditions } | Self::Any { conditions } => {
                conditions.iter().for_each(|c| c.collect_names(out));
            }
        }
    }

    /// Both conditions.
    pub(crate) fn and(self, other: Condition) -> Self {
        match self {
            Self::All { mut conditions } => {
                conditions.push(other);
                Self::All { conditions }
            }
            first => Self::All { conditions: vec![first, other] },
        }
    }
}

/// Where the input a condition reads is, from a repeater's row (ADR 0044).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// The same level: the row's inputs (or the form's, outside rows).
    #[default]
    Row,
    /// The enclosing row's inputs.
    Parent,
    /// The record's form (the top level).
    Root,
}

impl Scope {
    #[allow(clippy::trivially_copy_pass_by_ref)] // serde's `skip_serializing_if` passes a reference
    pub(crate) fn is_row(&self) -> bool {
        *self == Self::Row
    }
}

/// Starts a condition on the value of input `name` (in a repeater's row: an
/// input of the same row).
pub fn when(name: impl Into<String>) -> When {
    When(name.into(), Scope::Row)
}

/// In a repeater's row: starts a condition on the value of input `name` of
/// the record's form (refused at the form's own level).
pub fn when_root(name: impl Into<String>) -> When {
    When(name.into(), Scope::Root)
}

/// In a repeater's row: starts a condition on the value of input `name` of
/// the enclosing row (refused outside nested rows).
pub fn when_parent(name: impl Into<String>) -> When {
    When(name.into(), Scope::Parent)
}

/// A condition being built on one input's value ([`when`]).
#[derive(Debug, Clone)]
pub struct When(String, Scope);

impl When {
    /// The value equals `value`.
    pub fn eq(self, value: impl Into<Value>) -> Condition {
        Condition::Eq { name: self.0, scope: self.1, value: value.into() }
    }

    /// The value differs from `value`.
    pub fn ne(self, value: impl Into<Value>) -> Condition {
        Condition::Ne { name: self.0, scope: self.1, value: value.into() }
    }

    /// The value is one of `values`.
    pub fn is_in<V: Into<Value>>(self, values: impl IntoIterator<Item = V>) -> Condition {
        Condition::IsIn { name: self.0, scope: self.1, values: values.into_iter().map(Into::into).collect() }
    }

    /// The value is set: not `null`, `""`, `[]` or `{}`.
    pub fn filled(self) -> Condition {
        Condition::Filled { name: self.0, scope: self.1 }
    }

    /// The value is `true` (a boolean input).
    pub fn is_true(self) -> Condition {
        Condition::IsTrue { name: self.0, scope: self.1 }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn serializes_as_tagged_data() {
        assert_eq!(
            serde_json::to_value(when("link_type").eq("external")).unwrap(),
            json!({"op": "eq", "name": "link_type", "value": "external"})
        );
        assert_eq!(
            serde_json::to_value(Condition::any([when("a").filled(), when("b").is_in([1, 2])])).unwrap(),
            json!({"op": "any", "conditions": [
                {"op": "filled", "name": "a"},
                {"op": "is_in", "name": "b", "values": [1, 2]},
            ]})
        );
        assert_eq!(
            serde_json::to_value(when_root("type").eq("quiz")).unwrap(),
            json!({"op": "eq", "name": "type", "scope": "root", "value": "quiz"})
        );
    }

    #[test]
    fn lists_names_and_combines() {
        let condition = when("a").is_true().and(Condition::all([when("b").filled(), when("a").ne(3)]));
        let names: Vec<&str> = condition.names().into_iter().map(|(_, n)| n).collect();
        assert_eq!(names, ["a", "b", "a"]);
        assert_eq!(when_root("t").filled().names(), [(Scope::Root, "t")]);
    }
}
