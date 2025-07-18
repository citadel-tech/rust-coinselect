#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use rust_coinselect::{
    selectcoin::select_coin,
    types::{CoinSelectionOpt, ExcessStrategy, OutputGroup},
};

#[derive(Debug, Arbitrary)]
pub struct ArbitraryOutputGroup {
    pub value: u64,
    pub weight: u64,
    pub input_count: usize,
    pub creation_sequence: Option<u32>,
}

impl Into<OutputGroup> for ArbitraryOutputGroup {
    fn into(self) -> OutputGroup {
        OutputGroup {
            value: self.value,
            weight: self.weight,
            input_count: self.input_count,
            creation_sequence: self.creation_sequence,
        }
    }
}

#[derive(Debug, Arbitrary)]
pub struct ArbitraryCoinSelectionOpt {
    pub target_value: u64,
    pub target_feerate: f32,
    pub long_term_feerate: Option<f32>,
    pub min_absolute_fee: u64,
    pub base_weight: u64,
    pub change_weight: u64,
    pub change_cost: u64,
    pub avg_input_weight: u64,
    pub avg_output_weight: u64,
    pub min_change_value: u64,
    pub excess_strategy: ArbitraryExcessStrategy,
}

impl Into<CoinSelectionOpt> for ArbitraryCoinSelectionOpt {
    fn into(self) -> CoinSelectionOpt {
        CoinSelectionOpt {
            target_value: self.target_value,
            target_feerate: self.target_feerate,
            long_term_feerate: self.long_term_feerate,
            min_absolute_fee: self.min_absolute_fee,
            base_weight: self.base_weight,
            change_weight: self.change_weight,
            change_cost: self.change_cost,
            avg_input_weight: self.avg_input_weight,
            avg_output_weight: self.avg_output_weight,
            min_change_value: self.min_change_value,
            excess_strategy: self.excess_strategy.into(),
        }
    }
}
#[derive(Debug, Arbitrary)]
pub enum ArbitraryExcessStrategy {
    ToFee,
    ToRecipient,
    ToChange,
}

impl Into<ExcessStrategy> for ArbitraryExcessStrategy {
    fn into(self) -> ExcessStrategy {
        match self {
            ArbitraryExcessStrategy::ToFee => ExcessStrategy::ToFee,
            ArbitraryExcessStrategy::ToChange => ExcessStrategy::ToChange,
            ArbitraryExcessStrategy::ToRecipient => ExcessStrategy::ToRecipient,
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(&data);
    let arbitrary_inputs = Vec::<ArbitraryOutputGroup>::arbitrary(&mut u).unwrap();
    let mut inputs = Vec::new();
    for o in arbitrary_inputs {
        inputs.push(o.into());
    }
    let opts = ArbitraryCoinSelectionOpt::arbitrary(&mut u).unwrap().into();
    dbg!(&inputs);
    dbg!(&opts);
    let _ = select_coin(&inputs, &opts);
});
