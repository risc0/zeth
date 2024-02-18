use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};
use zeth_primitives::{Address, B256};

const DEFAULT_RAIKO_USER_CONFIG_SUBDIR_PATH: &str = ".config/raiko";

#[derive(Debug, Parser)]
pub struct App {
    #[clap(flatten)]
    pub global_opts: GlobalOpts,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Server(ServerArgs),
    OneShot(OneShotArgs),
    /// Bootstrap the application and exit. Bootstraping process creates the first
    /// public-private key pair and saves it on disk in encrypted form.
    Bootstrap,
}

#[derive(Debug, Args)]
pub struct ServerArgs {
    #[clap(short, long, require_equals = true, default_value = "0.0.0.0:8080")]
    pub addr: String,
}

#[derive(Debug, Args)]
pub struct OneShotArgs {
    #[clap(long)]
    /// Path of the *.json.gz file with the block data.
    pub blocks_data_file: PathBuf,
    #[clap(long)]
    pub l1_blocks_data_file: PathBuf,
    #[clap(long)]
    pub prover: Address,
    #[clap(long)]
    pub graffiti: B256,
    #[clap(long)]
    pub sgx_instance_id: u32,
    #[clap(long, default_value = "internal_devnet_a")]
    pub l2_chain: Option<String>,
}

fn get_default_raiko_user_config_path(subdir: &str) -> PathBuf {
    let mut home_dir = dirs::home_dir().unwrap();
    home_dir.push(DEFAULT_RAIKO_USER_CONFIG_SUBDIR_PATH);
    home_dir.push(subdir);
    home_dir
}

#[derive(Debug, Args)]
pub struct GlobalOpts {
    #[clap(short, long, default_value=get_default_raiko_user_config_path("secrets").into_os_string())]
    /// Path to the directory with the encrypted private keys being used to sign the
    /// blocks. For more details on the encryption see:
    /// https://gramine.readthedocs.io/en/stable/manifest-syntax.html#encrypted-files
    pub secrets_dir: PathBuf,

    #[clap(short, long, default_value=get_default_raiko_user_config_path("config").into_os_string())]
    /// Path to the directory with raiko configuration files.
    pub config_dir: PathBuf,

    #[clap(long, short, global = true, action = ArgAction::Count)]
    /// Verbosity of the application. Use multiple times to increase verbosity.
    pub verbose: u8,
}
