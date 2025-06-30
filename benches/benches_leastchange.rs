use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rust_coinselect::{
    algorithms::leastchange::select_coin_bnb_leastchange,
    types::{CoinSelectionOpt, ExcessStrategy, OutputGroup, SelectionError, SelectionOutput},
};

fn benchmark_select_coin_leastchange(c: &mut Criterion) {
    let inputs = [
        OutputGroup {
            value: 55000,
            weight: 500,
            input_count: 1,
            creation_sequence: None,
        },
        OutputGroup {
            value: 400,
            weight: 200,
            input_count: 1,
            creation_sequence: None,
        },
        OutputGroup {
            value: 40000,
            weight: 300,
            input_count: 1,
            creation_sequence: None,
        },
        OutputGroup {
            value: 25000,
            weight: 100,
            input_count: 1,
            creation_sequence: None,
        },
        OutputGroup {
            value: 35000,
            weight: 150,
            input_count: 1,
            creation_sequence: None,
        },
        OutputGroup {
            value: 600,
            weight: 250,
            input_count: 1,
            creation_sequence: None,
        },
        OutputGroup {
            value: 30000,
            weight: 120,
            input_count: 1,
            creation_sequence: None,
        },
        OutputGroup {
            value: 5000,
            weight: 50,
            input_count: 1,
            creation_sequence: None,
        },
    ];

    let options = CoinSelectionOpt {
        target_value: 60000,
        target_feerate: 0.5,
        long_term_feerate: None,
        min_absolute_fee: 0,
        base_weight: 10,
        change_weight: 50,
        change_cost: 10,
        avg_input_weight: 20,
        avg_output_weight: 10,
        min_change_value: 500,
        excess_strategy: ExcessStrategy::ToChange,
    };

    let mut final_result: Option<Result<SelectionOutput, SelectionError>> = None;

    c.bench_function("select_coin_leastchange", |b| {
        b.iter(|| {
            final_result = Some(select_coin_bnb_leastchange(
                black_box(&inputs),
                black_box(&options),
            ));
            black_box(&final_result);
        })
    });

    // Print result after benchmarking finishes
    if let Some(result) = &final_result {
        match result {
            Ok(selection) => println!("SelectionOutput: {:?}", selection),
            Err(e) => println!("Error: {:?}", e),
        }
    }
}

criterion_group!(benches, benchmark_select_coin_leastchange);
criterion_main!(benches);
