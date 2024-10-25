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

use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};

pub type Rescued<T> = Arc<Mutex<Option<T>>>;

pub trait Recoverable: Sized {
    fn rescue(&mut self) -> Option<Self>;
}

#[derive(Default)]
pub struct Wrapper<T: Recoverable> {
    pub inner: T,
    pub rescue: Rescued<T>,
}

impl<T: Recoverable> Wrapper<T> {
    pub fn rescued(&self) -> Rescued<T> {
        self.rescue.clone()
    }

    pub fn unwrap(self) -> T {
        let rescued = self.rescued();
        drop(self);
        let inner = rescued.lock().unwrap().take().unwrap();
        inner
    }
}

impl<T: Recoverable> Drop for Wrapper<T> {
    fn drop(&mut self) {
        if let Some(value) = self.inner.rescue() {
            if let Ok(mut rescue) = self.rescue.lock() {
                rescue.replace(value);
            }
        }
    }
}

impl<T: Recoverable> From<T> for Wrapper<T> {
    fn from(value: T) -> Self {
        Self {
            inner: value,
            rescue: Default::default(),
        }
    }
}

impl<T: Recoverable> From<Rescued<T>> for Wrapper<T> {
    fn from(rescue: Rescued<T>) -> Self {
        let value = rescue.lock().unwrap().take().unwrap();
        Self { inner: value, rescue }
    }
}

impl<T: Recoverable> Deref for Wrapper<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: Recoverable> DerefMut for Wrapper<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}