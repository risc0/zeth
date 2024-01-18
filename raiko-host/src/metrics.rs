use lazy_static::lazy_static;
use prometheus::{
    labels, register_int_counter_vec, register_int_gauge, register_int_gauge_vec, IntCounterVec,
    IntGauge, IntGaugeVec,
};

lazy_static! {
    pub static ref SGX_PROOF_GEN_TIME: IntGaugeVec = register_int_gauge_vec!(
        "sgx_proof_time_gauge",
        "time taken for sgx proof generation",
        &["blockid"]
    )
    .unwrap();
    pub static ref SGX_PROOF_SUCCESS_COUNTER: IntCounterVec = register_int_counter_vec!(
        "sgx_proof_success_counter",
        "number of successful sgx proof calls",
        &["blockid"]
    )
    .unwrap();
    pub static ref SGX_PROOF_ERROR_COUNTER: IntCounterVec = register_int_counter_vec!(
        "sgx_proof_error_counter",
        "number of failed sgx proof calls",
        &["blockid"]
    )
    .unwrap();
    pub static ref PREPARE_INPUT_TIME: IntGauge = register_int_gauge!(
        "prepare_input_time_gauge",
        "time taken for preparing input before proof generation"
    )
    .unwrap();
}

pub fn observe_sgx_gen(block: u64, time: i64) {
    let bid = &block.to_string()[..];
    let label = labels! {
        "blockid" => bid,
    };
    SGX_PROOF_GEN_TIME.with(&label).set(time);
}

pub fn inc_sgx_success(block: u64) {
    let bid = &block.to_string()[..];
    let label = labels! {
        "blockid" => bid,
    };
    SGX_PROOF_SUCCESS_COUNTER.with(&label).inc();
}

pub fn inc_sgx_error(block: u64) {
    let bid = &block.to_string()[..];
    let label = labels! {
        "blockid" => bid,
    };
    SGX_PROOF_ERROR_COUNTER.with(&label).inc();
}

pub fn observe_input(time: i64) {
    PREPARE_INPUT_TIME.set(time);
}
