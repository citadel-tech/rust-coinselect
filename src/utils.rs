use crate::types::{
    CoinSelectionOpt, EffectiveValue, ExcessStrategy, OutputGroup, SelectionError, Weight,
};
use std::{collections::HashSet, fmt, ops::Deref};

#[derive(Debug, Clone)]
pub(crate) struct PreparedOutputGroup {
    output_group: OutputGroup,
    pub index: usize,
}

impl Deref for PreparedOutputGroup {
    type Target = OutputGroup;

    fn deref(&self) -> &Self::Target {
        &self.output_group
    }
}

/// Builds the internal effective-value working set used by every selection algorithm.
pub(crate) fn prepare_output_groups(
    inputs: &[OutputGroup],
    options: &CoinSelectionOpt,
) -> Result<Vec<PreparedOutputGroup>> {
    if options.target_value == 0 {
        return Err(SelectionError::NonPositiveTarget);
    }
    if options.target_feerate <= 0.0
        || options
            .long_term_feerate
            .is_some_and(|feerate| feerate <= 0.0)
    {
        return Err(SelectionError::NonPositiveFeeRate);
    }
    if options.target_feerate > 1000.0
        || options
            .long_term_feerate
            .is_some_and(|feerate| feerate > 1000.0)
    {
        return Err(SelectionError::AbnormallyHighFeeRate);
    }

    let mut prepared = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        let effective_value = input
            .value
            .saturating_sub(calculate_fee(input.weight, options.target_feerate));
        if effective_value >= options.min_change_value {
            let mut output_group = input.clone();
            output_group.value = effective_value;
            prepared.push(PreparedOutputGroup {
                output_group,
                index,
            });
        }
    }
    if prepared.is_empty() {
        return Err(insufficient_funds(inputs, options));
    }
    Ok(prepared)
}

/// Reports the raw available value and the amount required when spending every supplied input.
pub(crate) fn insufficient_funds(
    inputs: &[OutputGroup],
    options: &CoinSelectionOpt,
) -> SelectionError {
    let available = inputs
        .iter()
        .fold(0u64, |total, input| total.saturating_add(input.value));
    let base_fee =
        calculate_fee(options.base_weight, options.target_feerate).max(options.min_absolute_fee);
    let total_input_fee = inputs
        .iter()
        .map(|input| calculate_fee(input.weight, options.target_feerate))
        .sum::<u64>();
    let required = options
        .target_value
        .saturating_add(base_fee)
        .saturating_add(total_input_fee);

    SelectionError::InsufficientFunds {
        available,
        required,
    }
}

/// Computes the total fee and waste metric (in satoshis) for a selection.
///
/// waste = weight * (target_feerate - long_term_feerate) + (cost_of_change OR excess)
#[inline]
pub fn calculate_fee_and_waste(
    options: &CoinSelectionOpt,
    accumulated_effective_value: u64,
    accumulated_weight: u64,
) -> Result<(u64, i64)> {
    let base_fee = calculate_fee(
        options.base_weight + options.change_weight,
        options.target_feerate,
    )
    .max(options.min_absolute_fee);
    let input_fee = calculate_fee(accumulated_weight, options.target_feerate);
    let long_term_feerate = options.long_term_feerate.unwrap_or(options.target_feerate);
    let fee_difference = (options.target_feerate - long_term_feerate) as f64;
    let mut waste = (accumulated_weight as f64 * fee_difference).round() as i64;
    let excess = accumulated_effective_value.saturating_sub(options.target_value + base_fee);
    if options.excess_strategy == ExcessStrategy::ToChange && excess >= options.min_change_value {
        // A change output is actually created, so we pay its cost (now and when spent later).
        waste += options.change_cost as i64;
    } else {
        // No change output is created; whatever is left over is wasted to fees/recipient.
        waste += excess as i64;
    }
    Ok((base_fee + input_fee, waste))
}

/// `adjusted_target` is the target value plus the estimated fee.
///
/// `smaller_coins` is a slice of pairs where the `usize` refers to the index of the `OutputGroup` in the provided inputs.
/// This slice should be sorted in descending order by the value of each `OutputGroup`, with each value being less than `adjusted_target`.
pub fn calculate_accumulated_weight(
    smaller_coins: &[(usize, EffectiveValue, Weight)],
    selected_inputs: &HashSet<usize>,
) -> u64 {
    let mut accumulated_weight: u64 = 0;
    for &(index, _value, weight) in smaller_coins {
        if selected_inputs.contains(&index) {
            accumulated_weight += weight;
        }
    }
    accumulated_weight
}

impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectionError::NonPositiveFeeRate => write!(f, "Negative fee rate"),
            SelectionError::NonPositiveTarget => write!(f, "Target value must be positive"),
            SelectionError::AbnormallyHighFeeRate => write!(f, "Abnormally high fee rate"),
            SelectionError::InsufficientFunds {
                available,
                required,
            } => write!(
                f,
                "Insufficient funds: available {available} sats, required {required} sats"
            ),
            SelectionError::NoSolutionFound => write!(f, "No solution could be derived"),
        }
    }
}

impl std::error::Error for SelectionError {}

type Result<T> = std::result::Result<T, SelectionError>;

#[inline]
pub fn calculate_fee(weight: u64, rate: f32) -> u64 {
    (weight as f32 * rate).ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CoinSelectionOpt, ExcessStrategy};

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

    /// The waste metric considers:
    /// - Long-term vs current fee rates
    /// - Cost of creating change outputs
    /// - Excess amounts based on selected strategy (fee/change/recipient)
    ///
    /// Test vectors cover:
    /// - Change output creation (ToChange strategy)
    /// - Fee payment (ToFee strategy)
    /// - Insufficient funds scenario
    #[test]
    fn test_calculate_fee_and_waste() {
        struct TestVector {
            options: CoinSelectionOpt,
            accumulated_value: u64,
            accumulated_weight: u64,
            fee: u64,
            result: i64,
        }

        let options = setup_options(100).clone();
        let test_vectors = [
            // Test for excess strategy to drain(change output)
            TestVector {
                options: options.clone(),
                accumulated_value: 1000,
                accumulated_weight: 50,
                fee: 24,
                result: options.change_cost as i64,
            },
            // Test for excess strategy to miners
            TestVector {
                options: CoinSelectionOpt {
                    excess_strategy: ExcessStrategy::ToFee,
                    ..options
                },
                accumulated_value: 1000,
                accumulated_weight: 50,
                fee: 24,
                result: 896,
            },
            // Test accumulated_value minus target_value < 0
            TestVector {
                options: CoinSelectionOpt {
                    target_value: 1000,
                    excess_strategy: ExcessStrategy::ToFee,
                    ..options
                },
                accumulated_value: 200,
                accumulated_weight: 50,
                fee: 24,
                result: 0,
            },
        ];

        for vector in test_vectors {
            let (fee, waste) = calculate_fee_and_waste(
                &vector.options,
                vector.accumulated_value,
                vector.accumulated_weight,
            )
            .unwrap();

            assert_eq!(fee, vector.fee);
            assert_eq!(waste, vector.result)
        }
    }
}
