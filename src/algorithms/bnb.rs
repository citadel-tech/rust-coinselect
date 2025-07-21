use crate::{
    types::{CoinSelectionOpt, OutputGroup, SelectionError, SelectionOutput, WasteMetric},
    utils::{calculate_fee, calculate_waste},
};

/// Struct MatchParameters encapsulates target_for_match, match_range, and target_feerate, options, tries, best solution.
#[derive(Debug)]
struct BnbContext {
    target_for_match: u64,
    match_range: u64,
    options: CoinSelectionOpt,
    tries: u32,
    best_solution: Option<(Vec<usize>, f32)>,
    // Used as a solution to Clippy's `Too Many Arguments` Warn.
    // https://rust-lang.github.io/rust-clippy/master/#too_many_arguments
}

/// Perform Coinselection via Branch And Bound algorithm, only returns a solution if least waste within target's `match_range` is found.
pub fn select_coin_bnb(
    inputs: &[OutputGroup],
    options: &CoinSelectionOpt,
) -> Result<SelectionOutput, SelectionError> {
    let cost_per_input = calculate_fee(options.avg_input_weight, options.target_feerate)?;
    let cost_per_output = calculate_fee(options.avg_output_weight, options.target_feerate)?;
    let base_fee = calculate_fee(options.base_weight, options.target_feerate)?;

    let mut sorted_inputs: Vec<(usize, &OutputGroup)> = inputs.iter().enumerate().collect();
    sorted_inputs.sort_by_key(|(_, input)| input.value);

    let mut ctx = BnbContext {
        target_for_match: options.target_value
            + options.min_change_value
            + base_fee.max(options.min_absolute_fee),
        match_range: cost_per_input + cost_per_output,
        options: options.clone(),
        tries: 1_000_000,
        best_solution: None,
    };

    let mut selected_inputs = vec![];

    bnb(&sorted_inputs, &mut selected_inputs, 0, 0, 0, &mut ctx);

    match ctx.best_solution {
        Some((selected, waste)) => Ok(SelectionOutput {
            selected_inputs: selected,
            waste: WasteMetric(waste),
        }),
        None => Err(SelectionError::NoSolutionFound),
    }
}

fn bnb(
    sorted: &[(usize, &OutputGroup)],
    selected: &mut Vec<usize>,
    acc_value: u64,
    acc_weight: u64,
    depth: usize,
    ctx: &mut BnbContext,
) {
    if ctx.tries == 0 || depth >= sorted.len() {
        return;
    }
    ctx.tries -= 1;

    // Calculate current fee based on accumulated weight
    let fee = calculate_fee(acc_weight, ctx.options.target_feerate)
        .unwrap_or(ctx.options.min_absolute_fee);
    // .max(ctx.options.min_absolute_fee);

    // Calculate effective value after fees
    let effective_value = acc_value.saturating_sub(fee);

    // Prune if we're way over target (including change consideration)
    if effective_value > ctx.target_for_match + ctx.match_range {
        return;
    }

    // Check for valid solution (must cover target + min change)
    if effective_value >= ctx.target_for_match {
        let waste = calculate_waste(&ctx.options, acc_value, acc_weight, fee);
        if ctx.best_solution.is_none() || waste < ctx.best_solution.as_ref().unwrap().1 {
            ctx.best_solution = Some((selected.clone(), waste));
        }
        return;
    }

    let (index, input) = sorted[depth];
    let input_effective_value = input.value.saturating_sub(
        calculate_fee(input.weight, ctx.options.target_feerate)
            .unwrap_or(ctx.options.min_absolute_fee),
    );

    // Branch 1: Include current input
    selected.push(index);
    bnb(
        sorted,
        selected,
        acc_value + input_effective_value,
        acc_weight + input.weight,
        depth + 1,
        ctx,
    );
    selected.pop();

    // Branch 2: Exclude current input
    bnb(sorted, selected, acc_value, acc_weight, depth + 1, ctx);
}

#[cfg(test)]
mod test {
    use crate::{
        algorithms::bnb::select_coin_bnb,
        types::{CoinSelectionOpt, ExcessStrategy, OutputGroup, SelectionError},
    };

    fn setup_basic_output_groups() -> Vec<OutputGroup> {
        vec![
            OutputGroup {
                value: 1000,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 2000,
                weight: 200,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 3000,
                weight: 300,
                input_count: 1,
                creation_sequence: None,
            },
        ]
    }

    fn bnb_setup_options(target_value: u64) -> CoinSelectionOpt {
        CoinSelectionOpt {
            target_value,
            target_feerate: 0.5, // Simplified feerate
            long_term_feerate: None,
            min_absolute_fee: 500,
            base_weight: 10,
            change_weight: 50,
            change_cost: 10,
            avg_input_weight: 40,
            avg_output_weight: 20,
            min_change_value: 500,
            excess_strategy: ExcessStrategy::ToChange,
        }
    }

    fn test_bnb_solution() {
        // Define the test values
        let mut values = [
            OutputGroup {
                value: 55000,
                weight: 500,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 400,
                weight: 200,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 40000,
                weight: 300,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 25000,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 35000,
                weight: 150,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 600,
                weight: 250,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 30000,
                weight: 120,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 94730,
                weight: 50,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 29810,
                weight: 500,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 78376,
                weight: 200,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 17218,
                weight: 300,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 13728,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
            },
        ];

        // Adjust the target value to ensure it tests for multiple valid solutions
        let opt = bnb_setup_options(195782);
        let ans = select_coin_bnb(&values, &opt);
        values.sort_by_key(|v| v.value);
        if let Ok(selection_output) = ans {
            let expected_solution = vec![1, 5, 11, 6, 4, 2, 9];
            assert_eq!(
                selection_output.selected_inputs, expected_solution,
                "Expected solution {:?}, but got {:?}",
                expected_solution, selection_output.selected_inputs
            );
        } else {
            panic!("Failed to find a solution");
        }
    }

    fn test_bnb_no_solution() {
        let inputs = setup_basic_output_groups();
        let total_input_value: u64 = inputs.iter().map(|input| input.value).sum();
        let impossible_target = total_input_value + 1000;
        let options = bnb_setup_options(impossible_target);
        let result = select_coin_bnb(&inputs, &options);
        assert!(
            matches!(result, Err(SelectionError::NoSolutionFound)),
            "Expected NoSolutionFound error, got {:?}",
            result
        );
    }

    #[test]
    fn test_bnb() {
        test_bnb_solution();
        test_bnb_no_solution();
    }
}
