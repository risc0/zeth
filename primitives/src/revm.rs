// Copyright 2024 RISC Zero, Inc.
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

//! Convert to revm types.

extern crate alloc;
extern crate core;

pub use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
pub use core::{
    convert::From,
    default::Default,
    option::{Option, Option::*},
    result::{Result, Result::*},
};

use revm_primitives::Log as RevmLog;

use crate::receipt::Log;

/// Provides a conversion from `RevmLog` to the local [Log].
impl From<RevmLog> for Log {
    fn from(log: RevmLog) -> Self {
        Log {
            address: log.address,
            topics: log.data.topics().to_vec(),
            data: log.data.data,
        }
    }
}
