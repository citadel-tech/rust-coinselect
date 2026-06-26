use crate::{
    types::{CoinSelectionOpt, OutputGroup, SelectionError, SelectionOutput, WasteMetric},
    utils::{calculate_fee, calculate_fee_and_waste},
};

/// Upper bound on the number of branches explored, matching Bitcoin Core's `TOTAL_TRIES`.
/// This guarantees the search terminates in bounded time regardless of the input set size.
const TOTAL_TRIES: u32 = 100_000;

/// Performs coin selection via the Branch and Bound algorithm.
///
/// This is a port of Bitcoin Core's `SelectCoinsBnB` after the "Optimize BnB exploration" rewrite
/// (bitcoin/bitcoin#32150), which adopts the CoinGrinder-style traversal: instead of explicitly
/// backtracking and re-testing omission branches, the search tracks the next candidate to explore
/// and *shifts* directly to it, and it uses a precomputed `lookahead` of the remaining effective
/// value at each depth to prune dead branches early. Successive candidates with identical effective
/// value to a just-omitted one are skipped, since they would only re-derive an already-seen set.
///
/// The search looks for a combination whose summed effective value lands in the window
/// `[target, target + cost_of_change]`. Because it searches for a *changeless* solution (the
/// leftover is small enough to drop to fees rather than create a change output), the target here is
/// deliberately *not* padded by `min_change_value` — unlike the accumulative algorithms. Effective
/// values are used throughout, so an input's spend fee is accounted for exactly once. Among all
/// solutions in range, the one with the least waste is returned.
///
/// Returns [`SelectionError::InsufficientFunds`] when the inputs cannot reach the target at all, and
/// [`SelectionError::NoSolutionFound`] when no in-range (changeless) combination exists.
pub fn select_coin_bnb(
    inputs: &[OutputGroup],
    options: &CoinSelectionOpt,
) -> Result<SelectionOutput, SelectionError> {
    let base_fee =
        calculate_fee(options.base_weight, options.target_feerate)?.max(options.min_absolute_fee);
    let actual_target = options.target_value + base_fee;
    let cost_of_change = options.change_cost;

    // The wrapper already mutates input values to effective values and filters dust-like inputs.
    let mut pool = inputs.to_vec();
    for (index, input) in pool.iter_mut().enumerate() {
        input.index.get_or_insert(index);
    }

    if pool.is_empty() {
        return Err(SelectionError::InsufficientFunds);
    }

    // Sort by descending effective value (largest first exploration).
    pool.sort_by(|a, b| b.value.cmp(&a.value));

    // `lookahead[i]` is the total effective value of all candidates *after* index `i` — i.e. the
    // value still reachable from depth `i`. Used to cut branches that can no longer hit the target.
    let mut lookahead = vec![0u64; pool.len()];
    let mut total_available: u64 = 0;
    for index in (0..pool.len()).rev() {
        lookahead[index] = total_available;
        total_available += pool[index].value;
    }

    if total_available < actual_target {
        return Err(SelectionError::InsufficientFunds);
    }

    // At high feerates, including more inputs only increases waste, which enables an extra pruning
    // branch. Mirrors Core's `is_feerate_high`.
    let is_feerate_high = options
        .long_term_feerate
        .is_some_and(|long_term_feerate| options.target_feerate > long_term_feerate);

    let mut current_selection: Vec<usize> = Vec::with_capacity(pool.len());
    let mut current_amount: u64 = 0;
    let mut current_waste: i64 = 0;

    let mut best_selection: Option<Vec<usize>> = None;
    let mut best_waste: i64 = i64::MAX;

    let mut next_utxo: usize = 0;
    let mut tries = TOTAL_TRIES;
    let mut is_done = false;

    while !is_done {
        // EXPLORE: add `next_utxo` to the current selection.
        let candidate = &pool[next_utxo];
        current_amount += candidate.value;
        current_waste += calculate_fee(candidate.weight, options.target_feerate)? as i64
            - calculate_fee(
                candidate.weight,
                options.long_term_feerate.unwrap_or(options.target_feerate),
            )? as i64;
        current_selection.push(next_utxo);
        next_utxo += 1;

        tries -= 1;
        if tries == 0 {
            break;
        }

        // EVALUATE: decide whether to keep exploring, SHIFT to the omission branch, or CUT.
        let last = *current_selection.last().unwrap();
        let mut should_shift = false;
        let mut should_cut = false;

        if current_amount + lookahead[last] < actual_target {
            // Even adding every remaining candidate cannot reach the target: CUT this subtree.
            should_cut = true;
        } else if current_amount > actual_target + cost_of_change {
            // Overshot the window: no deeper selection helps, SHIFT to the next branch.
            should_shift = true;
        } else if is_feerate_high && current_waste > best_waste {
            // Already wasteful and adding inputs only makes it worse: SHIFT.
            should_shift = true;
        } else if current_amount >= actual_target {
            // In range: a valid changeless solution. Record it if it improves on the best.
            should_shift = true;
            let excess = current_amount - actual_target;
            let waste = current_waste + excess as i64;
            if waste <= best_waste {
                best_waste = waste;
                best_selection = Some(current_selection.clone());
            }
        }
        // Otherwise: keep exploring deeper (the loop adds `next_utxo` next iteration).

        // A CUT is a SHIFT preceded by also dropping the last candidate (it leads nowhere).
        if should_cut {
            deselect_last(
                &pool,
                options,
                &mut current_selection,
                &mut current_amount,
                &mut current_waste,
            )?;
            should_shift = true;
        }

        while should_shift {
            // No selected candidate left to omit: the whole search space is exhausted.
            if current_selection.is_empty() {
                is_done = true;
                break;
            }
            // Move to the omission branch: explore the candidate after the last selected one, and
            // drop that last selected candidate from the running totals.
            next_utxo = current_selection.last().unwrap() + 1;
            deselect_last(
                &pool,
                options,
                &mut current_selection,
                &mut current_amount,
                &mut current_waste,
            )?;
            should_shift = false;

            // Skip candidates identical in effective value to the just-omitted one (clones): trying
            // them would only re-derive a set whose waste we have already considered. If we run off
            // the end of the pool, there is no fresh branch here, so SHIFT again (backtrack further).
            loop {
                if next_utxo >= pool.len() {
                    should_shift = true;
                    break;
                }
                if pool[next_utxo - 1].value == pool[next_utxo].value {
                    next_utxo += 1;
                    continue;
                }
                break;
            }
        }
    }

    let selected_pool_indices = match best_selection {
        Some(s) => s,
        None => return Err(SelectionError::NoSolutionFound),
    };

    let selected_inputs: Vec<usize> = selected_pool_indices
        .iter()
        .map(|&i| pool[i].index.expect("pool index is set before sorting"))
        .collect();

    // Recompute the reported waste from the concrete selection using the shared waste function so
    // the metric is comparable with the other algorithms.
    let accumulated_value: u64 = selected_pool_indices.iter().map(|&i| pool[i].value).sum();
    let accumulated_weight: u64 = selected_pool_indices.iter().map(|&i| pool[i].weight).sum();
    let (fee, waste) = calculate_fee_and_waste(options, accumulated_value, accumulated_weight)?;

    Ok(SelectionOutput {
        selected_inputs,
        waste: WasteMetric(waste),
        fee,
    })
}

/// Removes the most recently selected candidate, undoing its contribution to the running totals.
fn deselect_last(
    pool: &[OutputGroup],
    options: &CoinSelectionOpt,
    current_selection: &mut Vec<usize>,
    current_amount: &mut u64,
    current_waste: &mut i64,
) -> Result<(), SelectionError> {
    let last = current_selection
        .pop()
        .expect("deselect_last on empty selection");
    let candidate = &pool[last];
    *current_amount -= candidate.value;
    *current_waste -= calculate_fee(candidate.weight, options.target_feerate)? as i64
        - calculate_fee(
            candidate.weight,
            options.long_term_feerate.unwrap_or(options.target_feerate),
        )? as i64;
    Ok(())
}

#[cfg(test)]
mod test {
    use crate::{
        algorithms::bnb::select_coin_bnb,
        types::{CoinSelectionOpt, ExcessStrategy, OutputGroup, SelectionError},
    };

    /// Inputs whose effective values (weight 0 => fee 0 at feerate 1.0) are exactly their values,
    /// so subset sums are easy to reason about: 80000, 40000, 20000, 10000, 5000.
    fn setup_output_groups() -> Vec<OutputGroup> {
        [80_000u64, 40_000, 20_000, 10_000, 5_000]
            .into_iter()
            .map(|value| OutputGroup {
                value,
                weight: 0,
                input_count: 1,
                creation_sequence: None,
                index: None,
            })
            .collect()
    }

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
            min_change_value: 500,
            excess_strategy: ExcessStrategy::ToChange,
        }
    }

    #[test]
    fn test_bnb_finds_exact_changeless_solution() {
        // Target 65000 is uniquely hit by 40000 + 20000 + 5000 (indices 1, 2, 4) among these coins.
        let inputs = setup_output_groups();
        let options = setup_options(65_000);
        let result = select_coin_bnb(&inputs, &options).expect("a solution should exist");

        let mut selected = result.selected_inputs;
        selected.sort();
        assert_eq!(selected, vec![1, 2, 4]);
    }

    #[test]
    fn test_bnb_no_changeless_solution() {
        // All coins are multiples of 5000, so no subset lands in [63000, 63020]. There are
        // sufficient funds overall, so this is NoSolutionFound rather than InsufficientFunds.
        let inputs = setup_output_groups();
        let options = setup_options(63_000);
        let result = select_coin_bnb(&inputs, &options);
        assert!(
            matches!(result, Err(SelectionError::NoSolutionFound)),
            "expected NoSolutionFound, got {:?}",
            result
        );
    }

    #[test]
    fn test_bnb_insufficient_funds() {
        let inputs = setup_output_groups();
        let total: u64 = inputs.iter().map(|i| i.value).sum();
        let options = setup_options(total + 1_000);
        let result = select_coin_bnb(&inputs, &options);
        assert!(matches!(result, Err(SelectionError::InsufficientFunds)));
    }

    #[test]
    fn test_bnb_prefers_single_exact_input() {
        // 80000 alone exactly matches the target; BnB should pick it rather than a larger combo.
        let inputs = setup_output_groups();
        let options = setup_options(80_000);
        let result = select_coin_bnb(&inputs, &options).expect("a solution should exist");
        assert_eq!(result.selected_inputs, vec![0]);
    }

    /// Skipping clones must not cause an out-of-bounds or miss a solution. Several coins share the
    /// same effective value, and the target is only reachable by combining some of them.
    #[test]
    fn test_bnb_handles_clones() {
        let inputs: Vec<OutputGroup> = [10_000u64, 10_000, 10_000, 10_000, 7_000]
            .into_iter()
            .map(|value| OutputGroup {
                value,
                weight: 0,
                input_count: 1,
                creation_sequence: None,
                index: None,
            })
            .collect();
        let options = setup_options(30_000); // 10000 * 3
        let result = select_coin_bnb(&inputs, &options).expect("a solution should exist");
        assert_eq!(result.selected_inputs.len(), 3);
        let value: u64 = result
            .selected_inputs
            .iter()
            .map(|&i| inputs[i].value)
            .sum();
        assert_eq!(value, 30_000);
    }

    /// Brute-force cross-check: for many small input sets, BnB must return a selection whose summed
    /// effective value is inside `[target, target + cost_of_change]` with the minimum BnB-internal
    /// waste over *all* such subsets, and must report NoSolutionFound exactly when none exist.
    #[test]
    fn test_bnb_matches_brute_force() {
        // Effective value == value here (weight 0 => zero fee), and fee - long_term_fee == 0, so the
        // BnB-internal waste of any in-window subset is just its excess over the target.
        let value_sets: [&[u64]; 4] = [
            &[100, 200, 300, 400, 500, 600],
            &[1, 2, 4, 8, 16, 32, 64],
            &[55, 55, 55, 70, 13, 200, 9],
            &[1000, 999, 998, 5, 7, 3, 2],
        ];

        for values in value_sets {
            let inputs: Vec<OutputGroup> = values
                .iter()
                .map(|&value| OutputGroup {
                    value,
                    weight: 0,
                    input_count: 1,
                    creation_sequence: None,
                    index: None,
                })
                .collect();

            for target in 1u64..=120 {
                let mut options = setup_options(target);
                options.change_cost = 5; // cost_of_change window width

                // Brute force: minimum excess over all subsets summing into the window.
                let n = inputs.len();
                let mut brute_best: Option<u64> = None;
                for mask in 1u64..(1u64 << n) {
                    let sum: u64 = (0..n)
                        .filter(|&i| mask & (1 << i) != 0)
                        .map(|i| inputs[i].value)
                        .sum();
                    if sum >= target && sum <= target + options.change_cost {
                        let excess = sum - target;
                        brute_best = Some(brute_best.map_or(excess, |b| b.min(excess)));
                    }
                }

                match select_coin_bnb(&inputs, &options) {
                    Ok(result) => {
                        let sum: u64 = result
                            .selected_inputs
                            .iter()
                            .map(|&i| inputs[i].value)
                            .sum();
                        assert!(
                            sum >= target && sum <= target + options.change_cost,
                            "target {target}: selection sum {sum} out of window"
                        );
                        let excess = sum - target;
                        assert_eq!(
                            Some(excess),
                            brute_best,
                            "target {target}, values {values:?}: BnB excess {excess} != brute force {brute_best:?}"
                        );
                    }
                    Err(SelectionError::NoSolutionFound) => {
                        assert!(
                            brute_best.is_none(),
                            "target {target}, values {values:?}: BnB found nothing but brute force did"
                        );
                    }
                    Err(SelectionError::InsufficientFunds) => {
                        let total: u64 = values.iter().sum();
                        assert!(
                            total < target,
                            "target {target}: spurious InsufficientFunds"
                        );
                    }
                    Err(e) => panic!("unexpected error {e:?}"),
                }
            }
        }
    }
}
