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

use core2::{io, io::Read};

/// An adaptor that chains multiple readers together.
pub struct MultiReader<I, R> {
    readers: I,
    current: Option<R>,
}

impl<I, R> MultiReader<I, R>
where
    I: IntoIterator<Item = R>,
    R: Read,
{
    /// Creates a new instance of `MultiReader`.
    ///
    /// This function takes an iterator over readers and returns a `MultiReader`.
    pub fn new(readers: I) -> MultiReader<I::IntoIter, R> {
        let mut readers = readers.into_iter();
        let current = readers.next();
        MultiReader { readers, current }
    }
}

/// Implementation of the `Read` trait for `MultiReader`.
impl<I, R> Read for MultiReader<I, R>
where
    I: Iterator<Item = R>,
    R: Read,
{
    /// Reads data from the current reader into a buffer.
    ///
    /// This function reads as much data as possible from the current reader into the
    /// buffer, and switches to the next reader when the current one is exhausted.
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.current {
                Some(ref mut r) => {
                    let n = r.read(buf)?;
                    if n > 0 {
                        return Ok(n);
                    }
                }
                None => return Ok(0),
            }
            self.current = self.readers.next();
        }
    }
}
