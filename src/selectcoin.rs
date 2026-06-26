use std::thread;

use crate::{
    algorithms::{
        bnb::select_coin_bnb, coingrinder::select_coin_coingrinder, fifo::select_coin_fifo,
        lowestlarger::select_coin_lowestlarger,
    },
    types::{CoinSelectionOpt, OutputGroup, SelectionError, SelectionOutput},
    utils::calculate_fee,
};

/// Signature shared by every individual coin selection algorithm.
type CoinSelectionFn =
    fn(&[OutputGroup], &CoinSelectionOpt) -> Result<SelectionOutput, SelectionError>;

/// The global coin selection API that applies all algorithms and produces the result with the lowest [WasteMetric].
///
/// At least one selection solution should be found.
pub fn select_coin(
    inputs: &[OutputGroup],
    options: &CoinSelectionOpt,
) -> Result<SelectionOutput, SelectionError> {
    if options.target_value == 0 {
        return Err(SelectionError::NonPositiveTarget);
    }

    let mut selectable_inputs = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        let effective_value = input
            .value
            .saturating_sub(calculate_fee(input.weight, options.target_feerate)?);
        if effective_value >= options.min_change_value {
            let mut input = input.clone();
            input.value = effective_value;
            input.index = Some(index);
            selectable_inputs.push(input);
        }
    }

    if selectable_inputs.is_empty() {
        return Err(SelectionError::InsufficientFunds);
    }

    let algorithms: [CoinSelectionFn; 4] = [
        select_coin_bnb,
        select_coin_coingrinder,
        select_coin_fifo,
        select_coin_lowestlarger,
    ];

    // Run all algorithms concurrently. Checks only after all threads return and join.
    let outcomes: Vec<Result<SelectionOutput, SelectionError>> = thread::scope(|scope| {
        let selectable_inputs = &selectable_inputs;
        let handles: Vec<_> = algorithms
            .into_iter()
            .map(|algo| scope.spawn(move || algo(selectable_inputs, options)))
            .collect();
        handles
            .into_iter()
            // A panicking algorithm is treated as "no solution" rather than poisoning the API.
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or(Err(SelectionError::NoSolutionFound))
            })
            .collect()
    });

    let results: Vec<SelectionOutput> = outcomes.into_iter().flatten().collect();

    if results.is_empty() {
        return Err(SelectionError::InsufficientFunds);
    }

    // Ordering by least no. of inputs, then waste
    let best_result = results
        .into_iter()
        .min_by(|a, b| {
            a.selected_inputs
                .iter()
                .map(|&idx| inputs[idx].input_count)
                .sum::<usize>()
                .cmp(
                    &b.selected_inputs
                        .iter()
                        .map(|&idx| inputs[idx].input_count)
                        .sum::<usize>(),
                )
                .then_with(|| a.selected_inputs.len().cmp(&b.selected_inputs.len()))
                .then_with(|| a.waste.cmp(&b.waste))
        })
        .expect("results is non-empty");

    Ok(best_result)
}

#[cfg(test)]
mod test {

    use crate::{
        algorithms::{
            bnb::select_coin_bnb, coingrinder::select_coin_coingrinder, fifo::select_coin_fifo,
            lowestlarger::select_coin_lowestlarger,
        },
        selectcoin::select_coin,
        types::{CoinSelectionOpt, ExcessStrategy, OutputGroup, SelectionError},
        utils::calculate_fee_and_waste,
    };

    fn setup_basic_output_groups() -> Vec<OutputGroup> {
        vec![
            OutputGroup {
                value: 1_500_000,
                weight: 50,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 2_000_000,
                weight: 200,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 3_000_000,
                weight: 300,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 2_500_000,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 4_000_000,
                weight: 150,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 500_000,
                weight: 250,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 6_000_000,
                weight: 120,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 70_000,
                weight: 50,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 800_000,
                weight: 60,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 900_000,
                weight: 70,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 100_000,
                weight: 80,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 1_000_000,
                weight: 90,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
        ]
    }

    fn setup_options(target_value: u64) -> CoinSelectionOpt {
        CoinSelectionOpt {
            target_value,
            target_feerate: 2.0, // Simplified feerate
            long_term_feerate: Some(0.4),
            min_absolute_fee: 0,
            base_weight: 10,
            change_weight: 50,
            change_cost: 10,
            avg_input_weight: 20,
            avg_output_weight: 10,
            min_change_value: 500,
            excess_strategy: ExcessStrategy::ToChange,
        }
    }

    /// Asserts that a set of selected inputs actually covers the target plus the total fee.
    fn assert_covers_target(
        inputs: &[OutputGroup],
        options: &CoinSelectionOpt,
        selected: &[usize],
    ) {
        let value: u64 = selected.iter().map(|&i| inputs[i].value).sum();
        let weight: u64 = selected.iter().map(|&i| inputs[i].weight).sum();
        let (total_fee, _) = calculate_fee_and_waste(options, value, weight).unwrap();
        assert!(
            value >= options.target_value + total_fee,
            "selection {:?} (value {}) does not cover target {} + fee {}",
            selected,
            value,
            options.target_value,
            total_fee
        );
    }

    #[test]
    fn test_select_coin_rejects_zero_target() {
        let inputs = setup_basic_output_groups();
        let options = setup_options(0);
        let result = select_coin(&inputs, &options);
        assert!(matches!(result, Err(SelectionError::NonPositiveTarget)));
    }

    #[test]
    fn test_select_coin_successful() {
        let inputs = setup_basic_output_groups();
        let options = setup_options(654321);
        let result = select_coin(&inputs, &options);
        assert!(result.is_ok());
        let selection_output = result.unwrap();
        assert!(!selection_output.selected_inputs.is_empty());
        assert_covers_target(&inputs, &options, &selection_output.selected_inputs);
    }

    #[test]
    fn test_select_coin_insufficient_funds() {
        let inputs = setup_basic_output_groups();
        let options = setup_options(999_999_999); // Set a target value higher than the sum of all inputs
        let result = select_coin(&inputs, &options);
        assert!(matches!(result, Err(SelectionError::InsufficientFunds)));
    }

    /// The core contract of the wrapper: it must return the fewest inputs achievable by any of the
    /// individual algorithms (and a selection that actually covers the target).
    #[test]
    fn test_select_coin_returns_minimum_inputs() {
        let inputs = setup_basic_output_groups();
        let options = setup_options(654321);

        let individual_min_inputs = [
            select_coin_bnb(&inputs, &options),
            select_coin_coingrinder(&inputs, &options),
            select_coin_fifo(&inputs, &options),
            select_coin_lowestlarger(&inputs, &options),
        ]
        .into_iter()
        .filter_map(|r| r.ok())
        .map(|r| r.selected_inputs.len())
        .min()
        .expect("at least one algorithm should succeed");

        let chosen = select_coin(&inputs, &options).expect("selection should succeed");
        assert_eq!(
            chosen.selected_inputs.len(),
            individual_min_inputs,
            "wrapper did not return the minimum-input selection"
        );
        assert_covers_target(&inputs, &options, &chosen.selected_inputs);
    }

    /// Exercises the FIFO-friendly scenario (oldest-first ordering) end-to-end through the wrapper
    /// and checks the returned selection is valid.
    #[test]
    fn test_select_coin_fifo_scenario() {
        let inputs: Vec<OutputGroup> = [80000u64, 70000, 60000, 50000, 40000, 30000]
            .into_iter()
            .map(|value| OutputGroup {
                value,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
                index: None,
            })
            .collect();

        let options = CoinSelectionOpt {
            target_value: 250000,
            target_feerate: 1.0,
            min_absolute_fee: 0,
            base_weight: 100,
            change_weight: 10,
            change_cost: 20,
            avg_input_weight: 10,
            avg_output_weight: 10,
            min_change_value: 400,
            long_term_feerate: Some(0.5),
            excess_strategy: ExcessStrategy::ToChange,
        };

        let result = select_coin(&inputs, &options).expect("selection should succeed");
        assert_covers_target(&inputs, &options, &result.selected_inputs);
    }

    /// Exercises a BnB-friendly scenario (a changeless exact match exists) through the wrapper.
    #[test]
    fn test_select_coin_bnb_scenario() {
        let inputs: Vec<OutputGroup> = [150000u64, 250000, 300000, 100000, 50000]
            .into_iter()
            .map(|value| OutputGroup {
                value,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
                index: None,
            })
            .collect();
        let options = CoinSelectionOpt {
            target_value: 500000,
            target_feerate: 1.0,
            min_absolute_fee: 0,
            base_weight: 100,
            change_weight: 10,
            change_cost: 20,
            avg_input_weight: 10,
            avg_output_weight: 10,
            min_change_value: 400,
            long_term_feerate: Some(0.5),
            excess_strategy: ExcessStrategy::ToChange,
        };

        let result = select_coin(&inputs, &options).expect("selection should succeed");
        assert_covers_target(&inputs, &options, &result.selected_inputs);
    }
}
