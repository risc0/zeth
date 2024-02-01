pub mod block_builder;
#[cfg(not(target_os = "zkvm"))]
pub mod execute;
pub mod prepare;
pub mod utils;

pub enum Layer {
    L1,
    L2,
}
