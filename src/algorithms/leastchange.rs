use std::vec;

use crate::{
    types::{CoinSelectionOpt, OutputGroup, SelectionError, SelectionOutput, WasteMetric},
    utils::{calculate_fee, calculate_waste, effective_value},
};

/// A Branch and Bound state for Least Change selection which stores the state while traversing the tree.
struct BnBState {
    index: usize,
    current_eff_value: u64,
    current_selection: Vec<usize>,
    current_count: usize,
    current_weight: u64,
}

/// Selects inputs using BnB to first minimize change and then the input count.
pub fn select_coin_bnb_leastchange(
    inputs: &[OutputGroup],
    options: &CoinSelectionOpt,
) -> Result<SelectionOutput, SelectionError> {
    let target = options.target_value + options.min_change_value;
    let mut best: Option<(Vec<usize>, u64, usize)> = None; // (selection, change, count)

    // Precompute net values and filter beneficial inputs
    let mut filtered = inputs
        .iter()
        .enumerate()
        .filter_map(
            |(i, inp)| match effective_value(inp, options.target_feerate) {
                Ok(net_value) if net_value > 0 => Some((i, net_value, inp.weight)),
                _ => None,
            },
        )
        .collect::<Vec<_>>();

    // Sort by net value descending
    filtered.sort_by(|(_, a, _), (_, b, _)| b.cmp(a));

    // Precompute remaining net values for pruning
    let n = filtered.len();
    let mut remaining_net = vec![0; n + 1];
    for i in (0..n).rev() {
        remaining_net[i] = remaining_net[i + 1] + filtered[i].1;
    }

    // DFS with BnB pruning
    let mut stack = vec![BnBState {
        index: 0,
        current_eff_value: 0,
        current_selection: Vec::new(),
        current_count: 0,
        current_weight: 0,
    }];

    while let Some(state) = stack.pop() {
        if state.index >= n {
            continue;
        }

        // Prune if impossible to reach target
        if state.current_eff_value + remaining_net[state.index] < target {
            continue;
        }

        stack.push(BnBState {
            index: state.index + 1,
            current_eff_value: state.current_eff_value,
            current_selection: state.current_selection.clone(),
            current_count: state.current_count,
            current_weight: state.current_weight,
        });

        let (orig_idx, net_value, weight) = filtered[state.index];
        let new_eff_value = state.current_eff_value + net_value;
        let mut new_selection = state.current_selection.clone();
        new_selection.push(orig_idx);
        let new_count = state.current_count + 1;
        let new_weight = state.current_weight + weight;

        // Calculate fees based on current selection
        let estimated_fees = calculate_fee(new_weight, options.target_feerate).unwrap_or(0);
        let required_value = options.target_value
            + estimated_fees.max(options.min_absolute_fee)
            + options.min_change_value;

        if new_eff_value >= required_value {
            let change = new_eff_value - required_value;
            let update = match best {
                None => true,
                Some((_, best_change, best_count)) => {
                    change < best_change || (change == best_change && new_count < best_count)
                }
            };
            if update {
                best = Some((new_selection, change, new_count));
            }
        } else {
            stack.push(BnBState {
                index: state.index + 1,
                current_eff_value: new_eff_value,
                current_selection: new_selection,
                current_count: new_count,
                current_weight: new_weight,
            });
        }
    }

    if let Some((selected_inputs, _change, _count)) = best {
        let total_value: u64 = selected_inputs.iter().map(|&i| inputs[i].value).sum();
        let total_weight: u64 = selected_inputs.iter().map(|&i| inputs[i].weight).sum();
        let estimated_fees = calculate_fee(total_weight, options.target_feerate).unwrap_or(0);
        let waste = calculate_waste(options, total_value, total_weight, estimated_fees);

        Ok(SelectionOutput {
            selected_inputs,
            waste: WasteMetric(waste),
        })
    } else {
        Err(SelectionError::InsufficientFunds)
    }
}

#[cfg(test)]
mod test {

    use crate::{
        algorithms::leastchange::select_coin_bnb_leastchange,
        types::{CoinSelectionOpt, ExcessStrategy, OutputGroup, SelectionError},
    };

    fn setup_leastchange_output_groups() -> Vec<OutputGroup> {
        vec![
            OutputGroup {
                value: 100,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 1500,
                weight: 200,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 3400,
                weight: 300,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 2200,
                weight: 150,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 1190,
                weight: 200,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 1200,
                weight: 200,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 3300,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 1000,
                weight: 190,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 2000,
                weight: 210,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 3000,
                weight: 300,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 2250,
                weight: 250,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 190,
                weight: 220,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 1750,
                weight: 170,
                input_count: 1,
                creation_sequence: None,
            },
        ]
    }

    fn setup_options(target_value: u64) -> CoinSelectionOpt {
        CoinSelectionOpt {
            target_value,
            target_feerate: 50.0, // Simplified feerate
            long_term_feerate: Some(0.4),
            min_absolute_fee: 500,
            base_weight: 10,
            change_weight: 50,
            change_cost: 10,
            avg_input_weight: 20,
            avg_output_weight: 10,
            min_change_value: 294 * 5,
            excess_strategy: ExcessStrategy::ToRecipient,
        }
    }

    #[test]
    fn test_leastchange_successful() {
        let inputs = setup_leastchange_output_groups();
        let options = setup_options(6600);
        let result = select_coin_bnb_leastchange(&inputs, &options);
        assert!(result.is_ok());
        let selection_output = result.unwrap();
        assert!(!selection_output.selected_inputs.is_empty());
        let mut selected = selection_output.selected_inputs.clone();
        println!(
            "Total value minus Target : {} - {} = {}",
            selection_output
                .selected_inputs
                .iter()
                .map(|&i| inputs[i].value)
                .sum::<u64>(),
            options.target_value,
            selection_output
                .selected_inputs
                .iter()
                .map(|&i| inputs[i].value)
                .sum::<u64>()
                - options.target_value
        );
        println!(
            "Selected inputs: {:?}",
            selection_output
                .selected_inputs
                .iter()
                .map(|&i| (i, inputs[i].value))
                .collect::<Vec<_>>()
        );
        selected.sort();
        assert_eq!(selected, vec![0, 2, 6]);
    }

    #[test]
    fn test_leastchange_insufficient() {
        let inputs = setup_leastchange_output_groups();
        let options = setup_options(40000);
        let result = select_coin_bnb_leastchange(&inputs, &options);
        assert!(matches!(result, Err(SelectionError::InsufficientFunds)));
    }

    #[test]
    fn test_lc_solution() {
        // Define the test values
        let values = [
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
        let opt = setup_options(195782);
        let ans = select_coin_bnb_leastchange(&values, &opt);
        // values.sort_by_key(|v| v.value);
        if let Ok(selection_output) = ans {
            println!(
                "Total value minus Target : {} - {} = {}",
                selection_output
                    .selected_inputs
                    .iter()
                    .map(|&i| values[i].value)
                    .sum::<u64>(),
                opt.target_value,
                selection_output
                    .selected_inputs
                    .iter()
                    .map(|&i| values[i].value)
                    .sum::<u64>()
                    - opt.target_value
            );
            println!(
                "Selected inputs: {:?}",
                selection_output
                    .selected_inputs
                    .iter()
                    .map(|&i| (i, values[i].value))
                    .collect::<Vec<_>>()
            );
            let expected_solution = vec![7, 9, 0, 4, 2, 3];
            assert_eq!(
                selection_output.selected_inputs, expected_solution,
                "Expected solution {:?}, but got {:?}",
                expected_solution, selection_output.selected_inputs
            );
        } else {
            panic!("Failed to find a solution");
        }
    }
}
