//! Vitest mock operations. The mock and its recorded calls live behind the
//! erased ABI, a genuine dynamic boundary; see `smelt_vitest_mock_new`.

use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// `vi.fn(implementation)`.
    ///
    /// Vitest mock rvalues: the mock and its recorded calls live behind
    /// the erased ABI (a genuine dynamic boundary — see the prelude
    /// comment on `smelt_vitest_mock_new`), so every operand is erased
    /// to `SmeltUnknown` before crossing into the runtime helpers.
    pub(super) fn vitest_mock_fn_expr(
        &self,
        implementation: Option<&Operand>,
    ) -> Result<RustExpr, EmitError> {
        Ok(match implementation {
            Some(implementation) => RustExpr::chain(format!(
                "smelt_vitest_mock_new(Some({}))",
                self.value_at_type(implementation, self.type_id(Type::Unknown)?)?.delimited()
            )),
            None => RustExpr::chain("smelt_vitest_mock_new(None)"),
        })
    }

    /// An assertion rule answered by the runtime over erased operands.
    pub(super) fn test_runtime_check_expr(
        &self,
        check: &str,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let rendered = args
            .iter()
            .map(|arg| Ok(self.value_at_type(arg, unknown_ty)?.delimited().to_owned()))
            .collect::<Result<Vec<_>, EmitError>>()?;
        Ok(RustExpr::chain(format!("smelt_test_check({check:?}, vec![{}])", rendered.join(", "))))
    }

    /// `test.prop(arbitraries, parameters)(name, predicate)`.
    pub(super) fn fast_check_run_expr(
        &self,
        arbitraries: &Operand,
        predicate: &Operand,
        parameters: Option<&Operand>,
    ) -> Result<RustExpr, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let parameter_text = match parameters {
            Some(given) => format!("Some(&({}))", self.value_at_type(given, unknown_ty)?.delimited()),
            None => "None".to_owned(),
        };
        Ok(RustExpr::chain(format!(
            "smelt_fast_check_run(&({}), &({}), {parameter_text})",
            self.value_at_type(arbitraries, unknown_ty)?.delimited(),
            self.value_at_type(predicate, unknown_ty)?.delimited(),
        )))
    }

    /// `toHaveBeenCalledTimes`.
    pub(super) fn vitest_mock_called_times_expr(
        &self,
        mock: &Operand,
        count: &Operand,
    ) -> Result<RustExpr, EmitError> {
        Ok(RustExpr::chain(format!(
            "smelt_vitest_mock_called_times(&({}), ({}) as f64)",
            self.value_at_type(mock, self.type_id(Type::Unknown)?)?.delimited(),
            self.value_at_type(count, self.type_id(Type::Float)?)?.delimited()
        )))
    }

    /// `toHaveBeenCalledWith` and its `Last` form.
    pub(super) fn vitest_mock_called_with_expr(
        &self,
        mock: &Operand,
        args: &[Operand],
        last: bool,
    ) -> Result<RustExpr, EmitError> {
        Ok(RustExpr::chain(format!(
            "smelt_vitest_mock_called_with(&({}), vec![{}], {})",
            self.value_at_type(mock, self.type_id(Type::Unknown)?)?.delimited(),
            args.iter()
                .map(|arg| self.value_at_type(arg, self.type_id(Type::Unknown)?))
                .collect::<Result<Vec<_>, _>>()?
                .join(", "),
            last
        )))
    }

    /// `vi.spyOn`.
    pub(super) fn vitest_spy_on_expr(
        &self,
        target: &Operand,
        name: &Operand,
    ) -> Result<RustExpr, EmitError> {
        Ok(RustExpr::chain(format!(
            "smelt_vitest_spy_on(&({}), &({}))",
            self.value_at_type(target, self.type_id(Type::Unknown)?)?.delimited(),
            self.value_at_type(name, self.type_id(Type::String)?)?.delimited()
        )))
    }

    /// Asymmetric equality of a matcher.
    ///
    /// Both operands are erased first: an asymmetric matcher is a
    /// marker-bearing record, so the comparison is a runtime walk over
    /// the dynamic carrier -- deliberately NOT `SmeltUnknown`'s own
    /// `PartialEq`, which library code observes and which must stay
    /// blind to the test harness's markers.
    pub(super) fn vitest_asymmetric_equal_expr(
        &self,
        actual: &Operand,
        expected: &Operand,
    ) -> Result<RustExpr, EmitError> {
        Ok(RustExpr::chain(format!(
            "smelt_vitest_asymmetric_equals(&({}), &({}), &mut ::std::collections::HashSet::new())",
            self.value_at_type(actual, self.type_id(Type::Unknown)?)?.delimited(),
            self.value_at_type(expected, self.type_id(Type::Unknown)?)?.delimited()
        )))
    }

    /// `toHaveLastResolvedWith`.
    pub(super) fn vitest_mock_last_resolved_with_expr(
        &self,
        mock: &Operand,
        expected: &Operand,
    ) -> Result<RustExpr, EmitError> {
        Ok(RustExpr::chain(format!(
            "smelt_vitest_mock_last_resolved_with(&({}), {})",
            self.value_at_type(mock, self.type_id(Type::Unknown)?)?.delimited(),
            self.value_at_type(expected, self.type_id(Type::Unknown)?)?.delimited()
        )))
    }
}
