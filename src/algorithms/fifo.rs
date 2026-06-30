use crate::{
    types::{CoinSelectionOpt, OutputGroup, SelectionError, SelectionOutput, WasteMetric},
    utils::{calculate_fee, calculate_fee_and_waste, insufficient_funds, prepare_output_groups},
};

/// Performs coin selection using the First-In-First-Out (FIFO) algorithm.
///
/// Oldest UTXOs (by `creation_sequence`) are spent first; inputs without a sequence are appended
/// last in their original order.
pub fn select_coin_fifo(
    inputs: &[OutputGroup],
    options: &CoinSelectionOpt,
) -> Result<SelectionOutput, SelectionError> {
    let insufficient_funds_error = insufficient_funds(inputs, options);
    let inputs = prepare_output_groups(inputs, options)?;
    let mut accumulated_value: u64 = 0;
    let mut accumulated_weight: u64 = 0;
    let mut selected_inputs: Vec<usize> = Vec::new();
    let base_fee = calculate_fee(
        options.base_weight + options.change_weight,
        options.target_feerate,
    )
    .max(options.min_absolute_fee);
    // Effective values already net out per-input fees, so the target only needs the base fee.
    let target = options.target_value + base_fee;

    // Sorting the inputs vector based on creation_sequence
    let mut sorted_inputs: Vec<_> = inputs
        .iter()
        .filter(|og| og.creation_sequence.is_some())
        .collect();

    sorted_inputs.sort_by(|a, b| a.creation_sequence.cmp(&b.creation_sequence));

    let inputs_without_sequence: Vec<_> = inputs
        .iter()
        .filter(|og| og.creation_sequence.is_none())
        .collect();

    sorted_inputs.extend(inputs_without_sequence);

    for input in sorted_inputs {
        accumulated_value += input.value;
        accumulated_weight += input.weight;
        selected_inputs.push(input.index);

        if accumulated_value >= target {
            break;
        }
    }

    if accumulated_value < target {
        Err(insufficient_funds_error)
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
        types::{
            basic_output_group, CoinSelectionOpt, ExcessStrategy, OutputGroup, SelectionError,
        },
    };

    fn setup_basic_output_groups() -> Vec<OutputGroup> {
        vec![
            basic_output_group(1000, 100),
            basic_output_group(2000, 200),
            basic_output_group(3000, 300),
        ]
    }
    fn setup_output_groups_withsequence() -> Vec<OutputGroup> {
        let mut inputs = vec![
            basic_output_group(1000, 100),
            basic_output_group(2000, 200),
            basic_output_group(3000, 300),
            basic_output_group(1500, 150),
        ];
        inputs[0].creation_sequence = Some(1);
        inputs[1].creation_sequence = Some(5000);
        inputs[2].creation_sequence = Some(1001);
        inputs
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
        assert!(matches!(
            result,
            Err(SelectionError::InsufficientFunds { .. })
        ));
    }

    #[test]
    fn test_fifo() {
        test_successful_selection();
        test_insufficient_funds();
    }
}
