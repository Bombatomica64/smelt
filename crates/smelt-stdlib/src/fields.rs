//! Exact source field-name recognition metadata shared by frontends and codegen.

/// Field read whose semantics come from the standard library rather than a
/// user-defined type shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum FieldRule {
    /// JavaScript `.length` on string/array-shaped receivers.
    TsLength,
    /// JavaScript `.constructor`, which Smelt erases to a null value.
    TsConstructor,
    /// JavaScript `.sort` read as a bound array method on erased receivers.
    TsSort,
}

/// Return the shared field rule for an exact TypeScript field spelling.
///
/// Callers stay responsible for receiver-shape checks; this only resolves the
/// stdlib identity of the field name itself.
#[must_use]
pub fn typescript_field_rule(field: &str) -> Option<FieldRule> {
    match field {
        "length" => Some(FieldRule::TsLength),
        "constructor" => Some(FieldRule::TsConstructor),
        "sort" => Some(FieldRule::TsSort),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exact stdlib field spellings resolve to their registry identity.
    #[test]
    fn recognizes_stdlib_field_names() {
        assert_eq!(typescript_field_rule("length"), Some(FieldRule::TsLength));
        assert_eq!(
            typescript_field_rule("constructor"),
            Some(FieldRule::TsConstructor)
        );
        assert_eq!(typescript_field_rule("sort"), Some(FieldRule::TsSort));
    }

    /// Ordinary user field names never resolve to a stdlib rule.
    #[test]
    fn rejects_user_field_names() {
        for field in ["len", "lengthInMeters", "construct", "sorted", "value"] {
            assert_eq!(typescript_field_rule(field), None);
        }
    }
}
