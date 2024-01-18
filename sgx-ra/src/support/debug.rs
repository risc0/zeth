/* Copyright (c) Fortanix, Inc.
 *
 * Licensed under the GNU General Public License, version 2 <LICENSE-GPL or
 * https://www.gnu.org/licenses/gpl-2.0.html> or the Apache License, Version
 * 2.0 <LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0>, at your
 * option. This file may not be copied, modified, or distributed except
 * according to those terms. */

use crate::mbedtls::ssl::Config;
use std::borrow::Cow;

#[cfg(feature = "debug")]
pub const DEFAULT_MBEDTLS_DEBUG_LEVEL: i32 = 3;

#[cfg(feature = "debug")]
static MBEDTLS_DEBUG_LEVEL: std::sync::OnceLock<i32> = std::sync::OnceLock::new();

/// Mbed TLS has a feature to show the TLS handshake logs, filtering with
/// certain debug level. Note that [`DEFAULT_MBEDTLS_DEBUG_LEVEL`] is the default
/// level of debug logs if you did not specify it through `MBEDTLS_DEBUG`
/// environment variable.
#[cfg(feature = "debug")]
pub fn get_mbedtls_debug_level() -> i32 {
    *MBEDTLS_DEBUG_LEVEL.get_or_init(|| {
        std::env::var("MBEDTLS_DEBUG")
            .unwrap_or_default()
            .parse()
            .unwrap_or(DEFAULT_MBEDTLS_DEBUG_LEVEL)
    })
}

pub fn set_config_debug(config: &mut Config, prefix: &str) {
    let prefix = prefix.to_owned();
    // This is mostly as an example - how to debug mbedtls
    let dbg_callback = move |level: i32, file: Cow<'_, str>, line: i32, message: Cow<'_, str>| {
        println!("{}{} {}:{} {}", prefix, level, file, line, message);
    };
    config.set_dbg_callback(dbg_callback);

    #[cfg(feature = "debug")]
    unsafe { crate::mbedtls::set_global_debug_threshold(get_mbedtls_debug_level()); }
}

pub fn init_env_logger() {
    let _ = env_logger::builder().is_test(true).try_init();
}
