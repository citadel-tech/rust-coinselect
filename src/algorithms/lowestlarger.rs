use crate::{
    types::{CoinSelectionOpt, OutputGroup, SelectionError, SelectionOutput, WasteMetric},
    utils::{calculate_fee, calculate_fee_and_waste, insufficient_funds, prepare_output_groups},
};

/// Performs coin selection using the Lowest Larger algorithm.
///
/// Two candidate selections are considered and the one with the lower waste is returned:
///
/// 1. **Lowest larger** : the single smallest input that on its own covers the target plus fees.
/// 2. **Accumulated smaller** : the inputs that are not individually sufficient, accumulated smallest-first until they cover the target plus fees.
pub fn select_coin_lowestlarger(
    inputs: &[OutputGroup],
    options: &CoinSelectionOpt,
) -> Result<SelectionOutput, SelectionError> {
    let insufficient_funds_error = insufficient_funds(inputs, options);
    let inputs = prepare_output_groups(inputs, options)?;
    let base_fee = calculate_fee(
        options.base_weight + options.change_weight,
        options.target_feerate,
    )
    .max(options.min_absolute_fee);
    // Effective values already net out per-input fees, so the target only needs the base fee.
    let target = options.target_value + base_fee;

    let mut sorted_inputs: Vec<_> = inputs.iter().collect();
    sorted_inputs.sort_by_key(|input| input.value);

    // Candidate 1: the smallest single input that alone covers target + its own fee.
    let mut single_candidate: Option<SelectionOutput> = None;
    for &input in &sorted_inputs {
        if input.value >= target {
            let (fee, waste) = calculate_fee_and_waste(options, input.value, input.weight)?;
            single_candidate = Some(SelectionOutput {
                selected_inputs: vec![input.index],
                waste: WasteMetric(waste),
                fee,
            });
            break;
        }
    }

    // Candidate 2: accumulate the inputs that are not individually sufficient, smallest first.
    let mut accumulated_value: u64 = 0;
    let mut accumulated_weight: u64 = 0;
    let mut selected_inputs: Vec<usize> = Vec::new();
    let mut accumulated_sufficient = false;
    for &input in &sorted_inputs {
        if input.value >= target {
            // Individually sufficient inputs belong to the single-coin candidate, skip here.
            continue;
        }
        accumulated_value += input.value;
        accumulated_weight += input.weight;
        selected_inputs.push(input.index);

        if accumulated_value >= target {
            accumulated_sufficient = true;
            break;
        }
    }
    let accumulated_candidate = if accumulated_sufficient {
        let (fee, waste) = calculate_fee_and_waste(options, accumulated_value, accumulated_weight)?;
        Some(SelectionOutput {
            selected_inputs,
            waste: WasteMetric(waste),
            fee,
        })
    } else {
        None
    };

    // Pick the candidate with the lower waste.
    match (single_candidate, accumulated_candidate) {
        (Some(single), Some(accumulated)) => {
            if accumulated.waste <= single.waste {
                Ok(accumulated)
            } else {
                Ok(single)
            }
        }
        (Some(single), None) => Ok(single),
        (None, Some(accumulated)) => Ok(accumulated),
        (None, None) => Err(insufficient_funds_error),
    }
}

#[cfg(test)]
mod test {

    use crate::{
        algorithms::lowestlarger::select_coin_lowestlarger,
        types::{
            basic_output_group, CoinSelectionOpt, ExcessStrategy, OutputGroup, SelectionError,
        },
    };

    fn setup_lowestlarger_output_groups() -> Vec<OutputGroup> {
        vec![
            basic_output_group(100, 100),
            basic_output_group(1500, 200),
            basic_output_group(3400, 300),
            basic_output_group(2200, 150),
            basic_output_group(1190, 200),
            basic_output_group(3300, 100),
            basic_output_group(1000, 190),
            basic_output_group(2000, 210),
            basic_output_group(3000, 300),
            basic_output_group(2250, 250),
            basic_output_group(190, 220),
            basic_output_group(1750, 170),
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
        assert!(matches!(
            result,
            Err(SelectionError::InsufficientFunds { .. })
        ));
    }
}
