use crate::{
    types::{CoinSelectionOpt, OutputGroup, SelectionError, SelectionOutput, WasteMetric},
    utils::{calculate_fee, calculate_fee_and_waste},
};

/// Upper bound on explored nodes, matching the bounded-search policy used by BnB.
const TOTAL_TRIES: u32 = 100_000;

#[derive(Debug, Clone, Copy)]
struct Candidate {
    index: usize,
    value: u64,
    weight: u64,
    input_count: usize,
}

#[derive(Debug, Clone)]
struct BestSelection {
    selected: Vec<usize>,
    value: u64,
    weight: u64,
    input_count: usize,
}

impl BestSelection {
    fn is_better_than(&self, other: &Self) -> bool {
        self.weight < other.weight
            || (self.weight == other.weight && self.value < other.value)
            || (self.weight == other.weight
                && self.value == other.value
                && self.input_count < other.input_count)
            || (self.weight == other.weight
                && self.value == other.value
                && self.input_count == other.input_count
                && self.selected < other.selected)
    }
}

/// Deterministic change-producing fallback issuing the least total weight to minimize the fee.
///
/// BnB is the changeless path. CoinGrinder is the fallback for when change is expected: it searches
/// for a selection that covers the target, total fee, and a minimally useful change amount, then
/// minimizes selected input weight. This avoids the old least-change behavior where many tiny inputs
/// could be linked together merely to shave the change amount down by a few sats.
pub fn select_coin_coingrinder(
    inputs: &[OutputGroup],
    options: &CoinSelectionOpt,
) -> Result<SelectionOutput, SelectionError> {
    let mut candidates = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        candidates.push(Candidate {
            index: input.index.unwrap_or(index),
            value: input.value,
            weight: input.weight,
            input_count: input.input_count,
        });
    }

    if candidates.is_empty() {
        return Err(SelectionError::InsufficientFunds);
    }

    candidates.sort_by(|a, b| {
        b.value
            .cmp(&a.value)
            .then_with(|| a.weight.cmp(&b.weight))
            .then_with(|| a.index.cmp(&b.index))
    });

    let mut remaining_value = vec![0u64; candidates.len() + 1];
    for index in (0..candidates.len()).rev() {
        remaining_value[index] = remaining_value[index + 1] + candidates[index].value;
    }

    let mut best = None;
    let mut selected = Vec::new();
    let mut tries = TOTAL_TRIES;
    let base_fee =
        calculate_fee(options.base_weight, options.target_feerate)?.max(options.min_absolute_fee);

    search(
        &candidates,
        &remaining_value,
        0,
        0,
        0,
        0,
        &mut selected,
        options,
        base_fee,
        &mut best,
        &mut tries,
    )?;

    let best = best.ok_or(SelectionError::InsufficientFunds)?;
    let (fee, waste) = calculate_fee_and_waste(options, best.value, best.weight)?;

    Ok(SelectionOutput {
        selected_inputs: best.selected,
        waste: WasteMetric(waste),
        fee,
    })
}

#[allow(clippy::too_many_arguments)]
fn search(
    candidates: &[Candidate],
    remaining_value: &[u64],
    index: usize,
    value: u64,
    weight: u64,
    input_count: usize,
    selected: &mut Vec<usize>,
    options: &CoinSelectionOpt,
    base_fee: u64,
    best: &mut Option<BestSelection>,
    tries: &mut u32,
) -> Result<(), SelectionError> {
    if *tries == 0 || index >= candidates.len() {
        return Ok(());
    }
    if value + remaining_value[index] < options.target_value + options.min_change_value {
        return Ok(());
    }
    if best.as_ref().is_some_and(|best| weight > best.weight) {
        return Ok(());
    }

    *tries -= 1;

    let candidate = candidates[index];
    let new_value = value + candidate.value;
    let new_weight = weight + candidate.weight;
    let new_input_count = input_count + candidate.input_count;
    selected.push(candidate.index);

    let required_value = options.target_value + options.min_change_value + base_fee;
    if new_value >= required_value {
        let candidate_best = BestSelection {
            selected: selected.clone(),
            value: new_value,
            weight: new_weight,
            input_count: new_input_count,
        };
        if best
            .as_ref()
            .is_none_or(|current| candidate_best.is_better_than(current))
        {
            *best = Some(candidate_best);
        }
    } else {
        search(
            candidates,
            remaining_value,
            index + 1,
            new_value,
            new_weight,
            new_input_count,
            selected,
            options,
            base_fee,
            best,
            tries,
        )?;
    }
    selected.pop();

    search(
        candidates,
        remaining_value,
        index + 1,
        value,
        weight,
        input_count,
        selected,
        options,
        base_fee,
        best,
        tries,
    )
}

#[cfg(test)]
mod test {
    use crate::{
        algorithms::coingrinder::select_coin_coingrinder,
        types::{CoinSelectionOpt, ExcessStrategy, OutputGroup, SelectionError},
    };

    fn setup_options(target_value: u64) -> CoinSelectionOpt {
        CoinSelectionOpt {
            target_value,
            target_feerate: 1.0,
            long_term_feerate: Some(1.0),
            min_absolute_fee: 0,
            base_weight: 0,
            change_weight: 50,
            change_cost: 20,
            avg_input_weight: 20,
            avg_output_weight: 10,
            min_change_value: 100,
            excess_strategy: ExcessStrategy::ToChange,
        }
    }

    #[test]
    fn test_coingrinder_prefers_lower_weight_over_lower_change() {
        let inputs = vec![
            OutputGroup {
                value: 10_500,
                weight: 100,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 4_000,
                weight: 80,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 3_500,
                weight: 80,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 3_000,
                weight: 80,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
        ];

        let result = select_coin_coingrinder(&inputs, &setup_options(10_000)).unwrap();
        assert_eq!(result.selected_inputs, vec![0]);
    }

    #[test]
    fn test_coingrinder_uses_multiple_inputs_when_needed() {
        let inputs = vec![
            OutputGroup {
                value: 6_000,
                weight: 90,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 5_000,
                weight: 90,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
            OutputGroup {
                value: 2_000,
                weight: 50,
                input_count: 1,
                creation_sequence: None,
                index: None,
            },
        ];

        let result = select_coin_coingrinder(&inputs, &setup_options(10_000)).unwrap();
        let mut selected = result.selected_inputs;
        selected.sort();
        assert_eq!(selected, vec![0, 1]);
    }

    #[test]
    fn test_coingrinder_insufficient_funds() {
        let inputs = vec![OutputGroup {
            value: 1_000,
            weight: 100,
            input_count: 1,
            creation_sequence: None,
            index: None,
        }];

        let result = select_coin_coingrinder(&inputs, &setup_options(10_000));
        assert!(matches!(result, Err(SelectionError::InsufficientFunds)));
    }
}
