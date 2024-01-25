pub mod block_builder;
#[cfg(not(target_os = "zkvm"))]
pub mod execute;
#[cfg(not(target_os = "zkvm"))]
pub mod host;
pub mod precheck;
pub mod prepare;
pub mod protocol_instance;
pub mod utils;
pub mod verify;

pub enum Layer {
    L1,
    L2,
}
