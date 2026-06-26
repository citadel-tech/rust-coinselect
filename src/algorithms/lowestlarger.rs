use crate::{
    types::{CoinSelectionOpt, OutputGroup, SelectionError, SelectionOutput, WasteMetric},
    utils::{calculate_fee, calculate_fee_and_waste},
};

/// Performs coin selection using the Lowest Larger algorithm.
///
/// Two candidate selections are considered and the one with the lower waste is returned:
///
/// 1. **Lowest larger** — the single smallest input that on its own covers the target plus fees.
/// 2. **Accumulated smaller** — the inputs that are *not* individually sufficient, accumulated
///    smallest-first until they cover the target plus fees.
///
/// Crucially, the smaller coins are compared *against* the smallest sufficient larger coin rather
/// than being combined with it, so a single tidy larger coin can win over a pile of small ones.
///
/// Returns [`SelectionError::InsufficientFunds`] if neither candidate can cover the target.
pub fn select_coin_lowestlarger(
    inputs: &[OutputGroup],
    options: &CoinSelectionOpt,
) -> Result<SelectionOutput, SelectionError> {
    // Accumulative path leaves a usable change output, so the target is padded by min_change_value.
    let target = options.target_value + options.min_change_value;
    let base_fee =
        calculate_fee(options.base_weight, options.target_feerate)?.max(options.min_absolute_fee);

    let mut sorted_inputs: Vec<(usize, &OutputGroup)> = inputs.iter().enumerate().collect();
    sorted_inputs.sort_by_key(|(_, input)| input.value);

    // Candidate 1: the smallest single input that alone covers target + its own fee.
    let mut single_candidate: Option<SelectionOutput> = None;
    for &(idx, input) in &sorted_inputs {
        if input.value >= target + base_fee {
            let (fee, waste) = calculate_fee_and_waste(options, input.value, input.weight)?;
            single_candidate = Some(SelectionOutput {
                selected_inputs: vec![input.index.unwrap_or(idx)],
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
    for &(idx, input) in &sorted_inputs {
        if input.value >= target + base_fee {
            // Individually sufficient inputs belong to the single-coin candidate, skip here.
            continue;
        }
        accumulated_value += input.value;
        accumulated_weight += input.weight;
        selected_inputs.push(input.index.unwrap_or(idx));

        if accumulated_value >= target + base_fee {
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
        (None, None) => Err(SelectionError::InsufficientFunds),
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
                index: None,
            },
            OutputGroup {
                value: 1500,
                weight: 200,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 3400,
                weight: 300,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 2200,
                weight: 150,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 1190,
                weight: 200,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 3300,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 1000,
                weight: 190,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 2000,
                weight: 210,
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
            OutputGroup {
                value: 2250,
                weight: 250,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 190,
                weight: 220,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 1750,
                weight: 170,
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
