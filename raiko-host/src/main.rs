#![feature(path_file_prefix)]
#![feature(absolute_path)]
// Copyright 2023 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

mod metrics;
mod prover;
#[allow(dead_code)]
mod rolling;
use std::{fmt::Debug, path::PathBuf};

use anyhow::{Context, Result};
use prover::server::serve;
use serde::Deserialize;
use structopt::StructOpt;
use structopt_toml::StructOptToml;
use tracing::info;

#[derive(StructOpt, StructOptToml, Deserialize, Debug)]
#[serde(default)]
struct Opt {
    #[structopt(long, require_equals = true, default_value = "0.0.0.0:8080")]
    /// Server bind address
    /// [default: 0.0.0.0:8080]
    bind: String,

    #[structopt(long, require_equals = true, default_value = "/tmp")]
    /// Use a local directory as a cache for RPC calls. Accepts a custom directory.
    cache: PathBuf,

    #[structopt(long, require_equals = true, default_value = "raiko-host/guests")]
    /// The guests path
    guest: PathBuf,

    #[structopt(long, require_equals = true, default_value = "0")]
    sgx_instance_id: u32,

    #[structopt(long, require_equals = true)]
    log_path: Option<PathBuf>,

    #[structopt(long, require_equals = true, default_value = "1000")]
    proof_cache: usize,

    #[structopt(long, require_equals = true, default_value = "10")]
    concurrency_limit: usize,

    #[structopt(long, require_equals = true, default_value = "7")]
    max_log_days: usize,

    #[structopt(long, require_equals = true, default_value = "internal_devnet_a")]
    l2_chain: String,

    #[structopt(long, require_equals = true, default_value = "20")]
    // WARNING: must large than concurrency_limit
    max_caches: usize,

    #[structopt(long, require_equals = true)]
    config_path: Option<PathBuf>,

    #[structopt(long, require_equals = true, env = "RUST_LOG", default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    prover::proof::powdr::execute_powdr().await?;
    // let mut opt = Opt::from_args();

    // if let Some(config_path) = opt.config_path {
    //     let config_raw = std::fs::read(&config_path)
    //         .context(format!("read config file {:?} failed", config_path))?;
    //     opt =
    //         Opt::from_args_with_toml(std::str::from_utf8(&config_raw).context("str parse failed")?)
    //             .context("toml parse failed")?;
    // };

    // let subscriber_builder = tracing_subscriber::FmtSubscriber::builder()
    //     .with_env_filter(&opt.log_level)
    //     .with_test_writer();
    // let _guard = match opt.log_path {
    //     Some(ref log_path) => {
    //         let file_appender = tracing_appender::rolling::Builder::new()
    //             .rotation(tracing_appender::rolling::Rotation::DAILY)
    //             .filename_prefix("raiko.log")
    //             .max_log_files(opt.max_log_days)
    //             .build(log_path)
    //             .expect("initializing rolling file appender failed");
    //         let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    //         let subscriber = subscriber_builder.json().with_writer(non_blocking).finish();
    //         tracing::subscriber::set_global_default(subscriber).unwrap();
    //         Some(_guard)
    //     }
    //     None => {
    //         let subscriber = subscriber_builder.finish();
    //         tracing::subscriber::set_global_default(subscriber).unwrap();
    //         None
    //     }
    // };
    // info!("Start args: {:?}", opt);
    // serve(
    //     &opt.bind,
    //     &opt.guest,
    //     &opt.cache,
    //     &opt.l2_chain,
    //     opt.sgx_instance_id,
    //     opt.proof_cache,
    //     opt.concurrency_limit,
    //     opt.max_caches,
    // )
    // .await?;
    Ok(())
}
