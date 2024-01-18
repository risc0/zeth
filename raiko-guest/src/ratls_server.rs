use crate::app_args::{GlobalOpts, ServerArgs};

pub fn ratls_server(_: GlobalOpts, args: ServerArgs) {
    #[cfg(feature = "sgx")]
    let _ = server_sgx::result_main(args.addr);
}
