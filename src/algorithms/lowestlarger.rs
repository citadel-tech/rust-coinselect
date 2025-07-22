use crate::{
    types::{CoinSelectionOpt, OutputGroup, SelectionError, SelectionOutput, WasteMetric},
    utils::{calculate_fee, calculate_waste, effective_value, sum},
};

/// Performs coin selection using the Lowest Larger algorithm.
///
/// Returns `NoSolutionFound` if no solution exists.
pub fn select_coin_lowestlarger(
    inputs: &[OutputGroup],
    options: &CoinSelectionOpt,
) -> Result<SelectionOutput, SelectionError> {
    let mut accumulated_value: u64 = 0;
    let mut accumulated_weight: u64 = 0;
    let mut selected_inputs: Vec<usize> = Vec::new();
    let mut estimated_fees: u64 = 0;
    let base_fees = calculate_fee(options.base_weight, options.target_feerate)?;
    let target = sum(
        sum(options.target_value, options.min_change_value)?,
        base_fees.max(options.min_absolute_fee),
    )?;

    let mut sorted_inputs: Vec<_> = inputs.iter().enumerate().collect();
    sorted_inputs.sort_by_key(|(_, input)| effective_value(input, options.target_feerate));

    let index = sorted_inputs.partition_point(|(_, input)| {
        if let Ok(fee) = calculate_fee(input.weight, options.target_feerate) {
            if let Ok(target_and_fee) = sum(target, fee) {
                return input.value <= target_and_fee;
            }
        }
        false
    });

    for (idx, input) in sorted_inputs.iter().take(index).rev() {
        accumulated_value = sum(accumulated_value, input.value)?;
        accumulated_weight = sum(accumulated_weight, input.weight)?;
        estimated_fees = calculate_fee(accumulated_weight, options.target_feerate)?;
        selected_inputs.push(*idx);

        if accumulated_value >= sum(target, estimated_fees)? {
            break;
        }
    }

    if accumulated_value < sum(target, estimated_fees)? {
        for (idx, input) in sorted_inputs.iter().skip(index) {
            accumulated_value = sum(accumulated_value, input.value)?;
            accumulated_weight = sum(accumulated_weight, input.weight)?;
            estimated_fees = calculate_fee(accumulated_weight, options.target_feerate)?;
            selected_inputs.push(*idx);

            if accumulated_value >= sum(target, estimated_fees.max(options.min_absolute_fee))? {
                break;
            }
        }
    }

    if accumulated_value < sum(target, estimated_fees)? {
        Err(SelectionError::InsufficientFunds)
    } else {
        let waste: f32 = calculate_waste(
            options,
            accumulated_value,
            accumulated_weight,
            estimated_fees,
        );
        Ok(SelectionOutput {
            selected_inputs,
            waste: WasteMetric(waste),
        })
    }
}

#[cfg(test)]
mod test {

    use crate::{
        algorithms::lowestlarger::select_coin_lowestlarger,
        types::{CoinSelectionOpt, ExcessStrategy, OutputGroup, SelectionError},
    };

    fn setup_lowestlarger_output_groups() -> Vec<OutputGroup> {
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

    #[test]
    fn test_lowestlarger_successful() {
        let inputs = setup_lowestlarger_output_groups();
        let options = setup_options(20000);
        let result = select_coin_lowestlarger(&inputs, &options);
        assert!(result.is_ok());
        let selection_output = result.unwrap();
        assert!(!selection_output.selected_inputs.is_empty());
    }

    #[test]
    fn test_lowestlarger_insufficient() {
        let inputs = setup_lowestlarger_output_groups();
        let options = setup_options(40000);
        let result = select_coin_lowestlarger(&inputs, &options);
        assert!(matches!(result, Err(SelectionError::InsufficientFunds)));
    }
}
