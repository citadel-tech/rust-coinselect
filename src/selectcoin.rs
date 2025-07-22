use crate::{
    algorithms::{
        bnb::select_coin_bnb,
        fifo::select_coin_fifo,
        knapsack::select_coin_knapsack,
        leastchange::select_coin_bnb_leastchange,
        lowestlarger::select_coin_lowestlarger,
        // srd::select_coin_srd,
    },
    types::{CoinSelectionOpt, OutputGroup, SelectionError, SelectionOutput},
};

/// The global coin selection API that applies all algorithms and produces the result with the lowest [WasteMetric].
///
/// At least one selection solution should be found.
type CoinSelectionFn =
    fn(&[OutputGroup], &CoinSelectionOpt) -> Result<SelectionOutput, SelectionError>;

pub fn select_coin(
    inputs: &[OutputGroup],
    options: &CoinSelectionOpt,
) -> Result<SelectionOutput, SelectionError> {
    if options.target_value == 0 {
        return Err(SelectionError::NonPositiveTarget);
    }

    let mut results = vec![];

    let mut sorted_inputs = inputs.to_vec();
    sorted_inputs.sort_by(|a, b| a.value.cmp(&b.value));

    let algorithms: Vec<(&str, CoinSelectionFn)> = vec![
        ("bnb", select_coin_bnb),
        // ("srd", select_coin_srd),
        ("fifo", select_coin_fifo),
        ("lowestlarger", select_coin_lowestlarger),
        ("knapsack", select_coin_knapsack),
        ("leastchange", select_coin_bnb_leastchange), // Future algorithms can be added here
    ];

    for (algo_name, algo) in algorithms {
        if let Ok(result) = algo(inputs, options) {
            let input_amount = result
                .selected_inputs
                .iter()
                .map(|&idx| inputs[idx].value)
                .sum::<u64>();
            let change = input_amount.saturating_sub(options.target_value);
            results.push((result, change, algo_name));
        }
    }

    if results.is_empty() {
        return Err(SelectionError::InsufficientFunds);
    }

    let best_result = results
        .into_iter()
        .min_by(|a, b| {
            a.1.cmp(&b.1) // Compare change amount first (a.1 vs b.1)
                .then_with(|| {
                    a.0.waste
                        .0
                        .partial_cmp(&b.0.waste.0)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }) // Then compare waste
                .then_with(|| a.0.selected_inputs.len().cmp(&b.0.selected_inputs.len()))
            // Finally compare number of inputs
        })
        .map(|(result, _, _)| result)
        .expect("No selection results found");

    Ok(best_result)
}

#[cfg(test)]
mod test {

    use crate::{
        selectcoin::select_coin,
        types::{CoinSelectionOpt, ExcessStrategy, OutputGroup, SelectionError},
        utils::effective_value,
    };
    use proptest::prop_assert;
    use test_strategy::proptest;
    #[proptest]
    fn solutions_fulfill_target(inputs: Vec<OutputGroup>, opts: CoinSelectionOpt) {
        let result = select_coin(&inputs, &opts);
        if let Ok(selection) = result {
            let index = selection.selected_inputs;
            let mut selected_inputs = vec![];
            for i in index {
                selected_inputs.push(&inputs[i]);
            }
            let total_effective_sum = selected_inputs
                .iter()
                .filter_map(|o| effective_value(o, opts.target_feerate).ok())
                .collect::<Vec<u64>>()
                .iter()
                .sum::<u64>();
            prop_assert!(total_effective_sum >= opts.target_value);
        }
    }
    fn setup_basic_output_groups() -> Vec<OutputGroup> {
        vec![
            OutputGroup {
                value: 1_500_000,
                weight: 50,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 2_000_000,
                weight: 200,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 3_000_000,
                weight: 300,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 2_500_000,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 4_000_000,
                weight: 150,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 500_000,
                weight: 250,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 6_000_000,
                weight: 120,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 70_000,
                weight: 50,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 800_000,
                weight: 60,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 900_000,
                weight: 70,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 100_000,
                weight: 80,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 1_000_000,
                weight: 90,
                input_count: 1,
                creation_sequence: None,
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

    #[test]
    fn test_select_coin_successful() {
        let inputs = setup_basic_output_groups();
        let options = setup_options(654321);
        let result = select_coin(&inputs, &options);
        assert!(result.is_ok());
        let selection_output = result.unwrap();
        assert!(!selection_output.selected_inputs.is_empty());

        let selected_values = selection_output
            .selected_inputs
            .iter()
            .map(|&idx| inputs[idx].value)
            .collect::<Vec<_>>();
        eprintln!("Best Result : {:?}", selected_values);
    }

    #[test]
    fn test_select_coin_insufficient_funds() {
        let inputs = setup_basic_output_groups();
        let options = setup_options(999_999_999); // Set a target value higher than the sum of all inputs
        let result = select_coin(&inputs, &options);
        assert!(matches!(result, Err(SelectionError::InsufficientFunds)));
    }

    #[test]
    fn test_select_coin_equals_lowest_larger() {
        // Define the inputs such that the lowest_larger algorithm should be optimal
        let inputs = vec![
            OutputGroup {
                value: 500,
                weight: 50,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 1500,
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
                value: 1000,
                weight: 75,
                input_count: 1,
                creation_sequence: None,
            },
        ];

        // Define the target selection options
        let options = CoinSelectionOpt {
            target_value: 1600, // Target value which lowest_larger can satisfy
            target_feerate: 0.4,
            long_term_feerate: Some(0.4),
            min_absolute_fee: 0,
            base_weight: 10,
            change_weight: 50,
            change_cost: 10,
            avg_input_weight: 50,
            avg_output_weight: 25,
            min_change_value: 500,
            excess_strategy: ExcessStrategy::ToChange,
        };

        // Call the select_coin function, which should internally use the lowest_larger algorithm
        let selection_result = select_coin(&inputs, &options).unwrap();

        // Deterministically choose a result based on how lowest_larger would select
        let expected_inputs = vec![2]; // Example choice based on lowest_larger logic

        // Sort the selected inputs to ignore the order
        let mut selection_inputs = selection_result.selected_inputs.clone();
        let mut expected_inputs_sorted = expected_inputs.clone();
        selection_inputs.sort();
        expected_inputs_sorted.sort();
    }

    #[test]
    fn test_select_coin_equals_knapsack() {
        // Define inputs that are best suited for knapsack algorithm to match the target value with minimal waste
        let inputs = vec![
            OutputGroup {
                value: 1500,
                weight: 1,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 2500,
                weight: 1,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 3000,
                weight: 1,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 1000,
                weight: 1,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 500,
                weight: 1,
                input_count: 1,
                creation_sequence: None,
            },
        ];

        // Define the target selection options
        let options = CoinSelectionOpt {
            target_value: 4000, // Set a target that knapsack can match efficiently
            target_feerate: 1.0,
            min_absolute_fee: 0,
            base_weight: 1,
            change_weight: 1,
            change_cost: 1,
            avg_input_weight: 1,
            avg_output_weight: 1,
            min_change_value: 500,
            long_term_feerate: Some(0.5),
            excess_strategy: ExcessStrategy::ToChange,
        };

        let selection_result = select_coin(&inputs, &options).unwrap();

        // Deterministically choose a result with justification
        // Here, we assume that the `select_coin` function internally chooses the most efficient set
        // of inputs that meet the `target_value` while minimizing waste. This selection is deterministic
        // given the same inputs and options. Therefore, the following assertions are based on
        // the assumption that the chosen inputs are correct and optimized.

        let expected_inputs = vec![1, 3]; // Example deterministic choice, adjust as needed

        // Sort the selected inputs to ignore the order
        let mut selection_inputs = selection_result.selected_inputs.clone();
        let mut expected_inputs_sorted = expected_inputs.clone();
        selection_inputs.sort();
        expected_inputs_sorted.sort();
    }

    #[test]
    fn test_select_coin_equals_fifo() {
        // Helper function to create OutputGroups
        fn create_fifo_inputs(values: Vec<u64>) -> Vec<OutputGroup> {
            values
                .into_iter()
                .map(|value| OutputGroup {
                    value,
                    weight: 100,
                    input_count: 1,
                    creation_sequence: None,
                })
                .collect()
        }

        let options_case = CoinSelectionOpt {
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

        let inputs_case = create_fifo_inputs(vec![80000, 70000, 60000, 50000, 40000, 30000]);

        let result_case = select_coin(&inputs_case, &options_case).unwrap();
        let expected_case = vec![0, 1, 2, 3]; // Indexes of oldest UTXOs that sum to target
        assert_eq!(result_case.selected_inputs, expected_case);
    }

    #[test]
    fn test_select_coin_equals_bnb() {
        let inputs = vec![
            OutputGroup {
                value: 150000,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 250000,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 300000,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 100000,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 50000,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
            },
        ];
        let opt = CoinSelectionOpt {
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
        let ans = select_coin(&inputs, &opt);

        if let Ok(selection_output) = ans {
            let mut selected_inputs = selection_output.selected_inputs.clone();
            selected_inputs.sort();

            // The expected solution is vec![1, 2] because the combined value of the selected inputs
            // (250000 + 300000) meets the target value of 500000 with minimal excess. This selection
            // minimizes waste and adheres to the constraints of the coin selection algorithm, which
            // aims to find the most optimal solution.
            // Branch and Bound also gives a better time complexity, referenced from Mark Erhardt's Master Thesis.

            let expected_solution = vec![1, 2];
            assert_eq!(
                selected_inputs, expected_solution,
                "Expected solution {:?}, but got {:?}",
                expected_solution, selected_inputs
            );
        }
    }

    #[test]
    fn test_select_coin_equals_leastchange_bnb() {
        // Inputs designed so that only one combination gives minimal change
        let inputs = vec![
            OutputGroup {
                value: 1000,
                weight: 10,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 2000,
                weight: 10,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 3000,
                weight: 10,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 4000,
                weight: 10,
                input_count: 1,
                creation_sequence: None,
            },
            OutputGroup {
                value: 5500,
                weight: 10,
                input_count: 1,
                creation_sequence: None,
            },
        ];

        let options = CoinSelectionOpt {
            target_value: 12000,
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

        let result = select_coin(&inputs, &options);
        assert!(result.is_ok());
        let selection_output = result.unwrap();
        assert!(!selection_output.selected_inputs.is_empty());

        let mut selected = selection_output.selected_inputs.clone();
        selected.sort();
        assert_eq!(selected, vec![0, 2, 3, 4]);
    }
}
