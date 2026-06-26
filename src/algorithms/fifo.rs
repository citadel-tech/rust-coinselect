use crate::{
    types::{CoinSelectionOpt, OutputGroup, SelectionError, SelectionOutput, WasteMetric},
    utils::{calculate_fee, calculate_fee_and_waste},
};

/// Performs coin selection using the First-In-First-Out (FIFO) algorithm.
///
/// Oldest UTXOs (by `creation_sequence`) are spent first; inputs without a sequence are appended
/// last in their original order.
pub fn select_coin_fifo(
    inputs: &[OutputGroup],
    options: &CoinSelectionOpt,
) -> Result<SelectionOutput, SelectionError> {
    let mut accumulated_value: u64 = 0;
    let mut accumulated_weight: u64 = 0;
    let mut selected_inputs: Vec<usize> = Vec::new();
    let target = options.target_value + options.min_change_value;
    let base_fee =
        calculate_fee(options.base_weight, options.target_feerate)?.max(options.min_absolute_fee);

    // Sorting the inputs vector based on creation_sequence
    let mut sorted_inputs: Vec<_> = inputs
        .iter()
        .enumerate()
        .filter(|(_, og)| og.creation_sequence.is_some())
        .collect();

    sorted_inputs.sort_by(|a, b| a.1.creation_sequence.cmp(&b.1.creation_sequence));

    let inputs_without_sequence: Vec<_> = inputs
        .iter()
        .enumerate()
        .filter(|(_, og)| og.creation_sequence.is_none())
        .collect();

    sorted_inputs.extend(inputs_without_sequence);

    for (index, input) in sorted_inputs {
        accumulated_value += input.value;
        accumulated_weight += input.weight;
        selected_inputs.push(input.index.unwrap_or(index));

        if accumulated_value >= target + base_fee {
            break;
        }
    }

    if accumulated_value < target + base_fee {
        Err(SelectionError::InsufficientFunds)
    } else {
        let (fee, waste) = calculate_fee_and_waste(options, accumulated_value, accumulated_weight)?;
        Ok(SelectionOutput {
            selected_inputs,
            waste: WasteMetric(waste),
            fee,
        })
    }
}

#[cfg(test)]
mod test {

    use crate::{
        algorithms::fifo::select_coin_fifo,
        types::{CoinSelectionOpt, ExcessStrategy, OutputGroup, SelectionError},
    };

    fn setup_basic_output_groups() -> Vec<OutputGroup> {
        vec![
            OutputGroup {
                value: 1000,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 2000,
                weight: 200,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 3000,
                weight: 300,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
        ]
    }
    fn setup_output_groups_withsequence() -> Vec<OutputGroup> {
        vec![
            OutputGroup {
                value: 1000,
                weight: 100,
                input_count: 1,
                creation_sequence: Some(1),
                index: None,
            },
            OutputGroup {
                value: 2000,
                weight: 200,
                input_count: 1,
                creation_sequence: Some(5000),
                index: None,
            },
            OutputGroup {
                value: 3000,
                weight: 300,
                input_count: 1,
                creation_sequence: Some(1001),
                index: None,
            },
            OutputGroup {
                value: 1500,
                weight: 150,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
        ]
    }

    fn setup_options(target_value: u64) -> CoinSelectionOpt {
        CoinSelectionOpt {
            target_value,
            target_feerate: 0.4, // Simplified feerate
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

    fn test_successful_selection() {
        let mut inputs = setup_basic_output_groups();
        let mut options = setup_options(2500);
        let mut result = select_coin_fifo(&inputs, &options);
        assert!(result.is_ok());
        let mut selection_output = result.unwrap();
        assert!(!selection_output.selected_inputs.is_empty());

        inputs = setup_output_groups_withsequence();
        options = setup_options(500);
        result = select_coin_fifo(&inputs, &options);
        assert!(result.is_ok());
        selection_output = result.unwrap();
        assert!(!selection_output.selected_inputs.is_empty());
    }

    fn test_insufficient_funds() {
        let inputs = setup_basic_output_groups();
        let options = setup_options(7000); // Set a target value higher than the sum of all inputs
        let result = select_coin_fifo(&inputs, &options);
        assert!(matches!(result, Err(SelectionError::InsufficientFunds)));
    }

    #[test]
    fn test_fifo() {
        test_successful_selection();
        test_insufficient_funds();
    }
}
