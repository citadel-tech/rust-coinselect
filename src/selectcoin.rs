use std::thread;

use crate::{
    algorithms::{
        bnb::select_coin_bnb, coingrinder::select_coin_coingrinder, fifo::select_coin_fifo,
        lowestlarger::select_coin_lowestlarger,
    },
    types::{CoinSelectionOpt, OutputGroup, SelectionAlgorithm, SelectionError, SelectionOutput},
    utils::insufficient_funds,
};

/// Signature shared by every individual coin selection algorithm.
type CoinSelectionFn =
    fn(&[OutputGroup], &CoinSelectionOpt) -> Result<SelectionOutput, SelectionError>;

/// The algorithms run by [`select_coin`], tagged with their identity.
const ALGORITHMS: [(SelectionAlgorithm, CoinSelectionFn); 4] = [
    (SelectionAlgorithm::BranchAndBound, select_coin_bnb),
    (SelectionAlgorithm::CoinGrinder, select_coin_coingrinder),
    (SelectionAlgorithm::Fifo, select_coin_fifo),
    (SelectionAlgorithm::LowestLarger, select_coin_lowestlarger),
];

/// The global coin selection API. Runs every algorithm and returns *all* successful results, each
/// tagged with the [`SelectionAlgorithm`] that produced it, ordered best-first.
///
/// The best-result policy is: fewest real UTXOs, then fewest groups, then least waste. So the first
/// element is the overall best selection. (`selected_inputs.len()` counts the chosen
/// [`OutputGroup`]s, whereas the `input_count` sum counts the actual UTXOs they bundle; the two
/// differ only when a group holds more than one UTXO.)
pub fn select_coin(
    inputs: &[OutputGroup],
    options: &CoinSelectionOpt,
) -> Result<Vec<(SelectionAlgorithm, SelectionOutput)>, SelectionError> {
    // Run all algorithms concurrently. Checks only after all threads return and join.
    let outcomes: Vec<(SelectionAlgorithm, Result<SelectionOutput, SelectionError>)> =
        thread::scope(|scope| {
            let handles: Vec<_> = ALGORITHMS
                .into_iter()
                .map(|(name, algo)| scope.spawn(move || (name, algo(inputs, options))))
                .collect();
            handles
                .into_iter()
                // A panicking algorithm is treated as "no solution" rather than poisoning the API.
                .map(|handle| {
                    handle.join().unwrap_or((
                        SelectionAlgorithm::BranchAndBound,
                        Err(SelectionError::NoSolutionFound),
                    ))
                })
                .collect()
        });

    let mut results = Vec::new();
    for (name, outcome) in outcomes {
        match outcome {
            Ok(result) => results.push((name, result)),
            Err(
                error @ (SelectionError::NonPositiveTarget
                | SelectionError::NonPositiveFeeRate
                | SelectionError::AbnormallyHighFeeRate),
            ) => return Err(error),
            Err(SelectionError::InsufficientFunds { .. } | SelectionError::NoSolutionFound) => {
                continue
            }
        }
    }

    if results.is_empty() {
        return Err(insufficient_funds(inputs, options));
    }

    // Order best-first: fewest real UTXOs, then fewest groups, then waste.
    results.sort_by_key(|(_, output)| {
        let total_input_count = output
            .selected_inputs
            .iter()
            .map(|&idx| inputs[idx].input_count)
            .sum::<usize>();
        (
            total_input_count,
            output.selected_inputs.len(),
            output.waste.0,
        )
    });
    Ok(results)
}

#[cfg(test)]
mod test {

    use crate::{
        algorithms::{
            bnb::select_coin_bnb, coingrinder::select_coin_coingrinder, fifo::select_coin_fifo,
            lowestlarger::select_coin_lowestlarger,
        },
        selectcoin::select_coin,
        types::{
            basic_output_group, CoinSelectionOpt, ExcessStrategy, OutputGroup, SelectionError,
            SelectionOutput,
        },
        utils::calculate_fee,
    };

    fn setup_basic_output_groups() -> Vec<OutputGroup> {
        vec![
            basic_output_group(1_500_000, 50),
            basic_output_group(2_000_000, 200),
            basic_output_group(3_000_000, 300),
            basic_output_group(2_500_000, 100),
            basic_output_group(4_000_000, 150),
            basic_output_group(500_000, 250),
            basic_output_group(6_000_000, 120),
            basic_output_group(70_000, 50),
            basic_output_group(800_000, 60),
            basic_output_group(900_000, 70),
            basic_output_group(100_000, 80),
            basic_output_group(1_000_000, 90),
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
        let selected_weight: u64 = selected.iter().map(|&i| inputs[i].weight).sum();
        let total_fee = calculate_fee(
            options.base_weight + selected_weight,
            options.target_feerate,
        )
        .max(options.min_absolute_fee);
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
        let ranked = result.unwrap();
        let best = &ranked[0].1;
        assert!(!best.selected_inputs.is_empty());
        assert_covers_target(&inputs, &options, &best.selected_inputs);
    }

    #[test]
    fn test_select_coin_insufficient_funds() {
        let inputs = setup_basic_output_groups();
        let options = setup_options(999_999_999); // Set a target value higher than the sum of all inputs
        let result = select_coin(&inputs, &options);
        assert!(matches!(
            result,
            Err(SelectionError::InsufficientFunds {
                available: 22_370_000,
                required: 1_000_003_059,
            })
        ));
    }

    #[test]
    fn test_select_coin_keeps_success_when_another_algorithm_is_insufficient() {
        let inputs = vec![basic_output_group(1_000, 0)];
        let mut options = setup_options(1_000);
        options.target_feerate = 1.0;
        options.long_term_feerate = Some(1.0);
        options.base_weight = 0;
        options.change_cost = 20;
        options.min_change_value = 100;

        let ranked = select_coin(&inputs, &options).expect("BnB should find the exact match");
        assert_eq!(ranked[0].1.selected_inputs, vec![0]);
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

        let ranked = select_coin(&inputs, &options).expect("selection should succeed");
        let chosen = &ranked[0].1;
        assert_eq!(
            chosen.selected_inputs.len(),
            individual_min_inputs,
            "wrapper did not return the minimum-input selection"
        );
        assert_covers_target(&inputs, &options, &chosen.selected_inputs);
    }

    /// `select_coin` must return every successful algorithm's result, ordered best-first by the
    /// policy: fewest real UTXOs, then fewest groups, then waste.
    #[test]
    fn test_select_coin_orders_best_first() {
        let inputs = setup_basic_output_groups();
        let options = setup_options(654321);

        let ranked = select_coin(&inputs, &options).expect("selection should succeed");
        assert!(!ranked.is_empty());

        // The list is sorted best-first by the rank key (non-decreasing keys).
        let key = |output: &SelectionOutput| {
            let total_input_count: usize = output
                .selected_inputs
                .iter()
                .map(|&idx| inputs[idx].input_count)
                .sum();
            (
                total_input_count,
                output.selected_inputs.len(),
                output.waste.0,
            )
        };
        let keys: Vec<_> = ranked.iter().map(|(_, output)| key(output)).collect();
        assert!(
            keys.windows(2).all(|w| w[0] <= w[1]),
            "ranked results are not ordered best-first: {keys:?}"
        );
    }
}
