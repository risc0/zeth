/* Copyright (c) Fortanix, Inc.
 *
 * Licensed under the GNU General Public License, version 2 <LICENSE-GPL or
 * https://www.gnu.org/licenses/gpl-2.0.html> or the Apache License, Version
 * 2.0 <LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0>, at your
 * option. This file may not be copied, modified, or distributed except
 * according to those terms. */

cfg_if::cfg_if! {
    if #[cfg(any(feature = "rdrand", target_env = "sgx"))] {
        pub fn entropy_new() -> crate::mbedtls::rng::Rdseed {
            crate::mbedtls::rng::Rdseed
        }
    } else if #[cfg(feature = "std")] {
        pub fn entropy_new() -> crate::mbedtls::rng::OsEntropy {
            crate::mbedtls::rng::OsEntropy::new()
        }
    } else {
        pub fn entropy_new() -> ! {
            panic!("Unable to run test without entropy source")
        }
    }
}
