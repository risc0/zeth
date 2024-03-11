use crate::app_args::{GlobalOpts, ServerArgs};

pub fn ratls_server(_: GlobalOpts, _args: ServerArgs) {
    #[cfg(feature = "sgx")]
    let _ = server_sgx::result_main(_args.addr);
}
