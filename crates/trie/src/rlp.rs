// Copyright 2023, 2024 RISC Zero, Inc.
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

use crate::data::MptNodeData;
use crate::node::MptNode;
use alloy_primitives::bytes::Buf;
use alloy_primitives::B256;
use alloy_rlp::{Decodable, Encodable};

/// Provides encoding functionalities for the `MptNode` type.
///
/// This implementation allows for the serialization of an [MptNode] into its RLP-encoded
/// form. The encoding is done based on the type of node data ([MptNodeData]) it holds.
impl Encodable for MptNode<'_> {
    /// Encodes the node into the provided `out` buffer.
    ///
    /// The encoding is done using the Recursive Length Prefix (RLP) encoding scheme. The
    /// method handles different node data types and encodes them accordingly.
    #[inline]
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        match &self.data {
            MptNodeData::Null => {
                out.put_u8(alloy_rlp::EMPTY_STRING_CODE);
            }
            MptNodeData::Branch(nodes) => {
                alloy_rlp::Header {
                    list: true,
                    payload_length: self.payload_length(),
                }
                .encode(out);
                nodes.iter().for_each(|child| match child {
                    Some(node) => node.reference_encode(out),
                    None => out.put_u8(alloy_rlp::EMPTY_STRING_CODE),
                });
                // in the MPT reference, branches have values so always add empty value
                out.put_u8(alloy_rlp::EMPTY_STRING_CODE);
            }
            MptNodeData::Leaf(prefix, value) => {
                alloy_rlp::Header {
                    list: true,
                    payload_length: self.payload_length(),
                }
                .encode(out);
                prefix.as_slice().encode(out);
                value.as_slice().encode(out);
            }
            MptNodeData::Extension(prefix, node) => {
                alloy_rlp::Header {
                    list: true,
                    payload_length: self.payload_length(),
                }
                .encode(out);
                prefix.as_slice().encode(out);
                node.reference_encode(out);
            }
            MptNodeData::Digest(digest) => {
                digest.encode(out);
            }
        }
    }

    /// Returns the length of the encoded node in bytes.
    ///
    /// This method calculates the length of the RLP-encoded node. It's useful for
    /// determining the size requirements for storage or transmission.
    #[inline]
    fn length(&self) -> usize {
        let payload_length = self.payload_length();
        payload_length + alloy_rlp::length_of_length(payload_length)
    }
}

/// Provides decoding functionalities for the [MptNode] type.
///
/// This implementation allows for the deserialization of an RLP-encoded [MptNode] back
/// into its original form. The decoding is done based on the prototype of the RLP data,
/// ensuring that the node is reconstructed accurately.
///
impl Decodable for MptNode<'_> {
    /// Decodes an RLP-encoded node from the provided `rlp` buffer.
    ///
    /// The method handles different RLP prototypes and reconstructs the `MptNode` based
    /// on the encoded data. If the RLP data does not match any known prototype or if
    /// there's an error during decoding, an error is returned.
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        if buf.is_empty() {
            return Ok(MptNodeData::Null.into());
        }
        match rlp_parse_head(buf)? {
            (0, _) => Ok(MptNodeData::Null.into()),
            (2, true) => {
                let path = Vec::from(alloy_rlp::Header::decode_bytes(buf, false)?);
                let prefix = path[0];
                if (prefix & (2 << 4)) == 0 {
                    let node = MptNode::decode(buf)?;
                    Ok(MptNodeData::Extension(path, Box::new(node.into())).into())
                } else {
                    let header = alloy_rlp::Header::decode(buf)?;
                    let value = Vec::from(&buf[..header.payload_length]);
                    buf.advance(header.payload_length);
                    Ok(MptNodeData::Leaf(path, value).into())
                }
            }
            (17, true) => {
                let mut node_list = Vec::with_capacity(16);
                for _ in 0..16 {
                    match *buf.first().ok_or(alloy_rlp::Error::InputTooShort)? {
                        alloy_rlp::EMPTY_STRING_CODE => {
                            buf.advance(1);
                            node_list.push(None);
                        }
                        _ => node_list.push(Some(Box::new(MptNode::decode(buf)?.into()))),
                    }
                }
                let value: Vec<u8> = Vec::from(alloy_rlp::Header::decode_bytes(buf, false)?);
                if value.is_empty() {
                    Ok(MptNodeData::Branch(node_list.try_into().unwrap()).into())
                } else {
                    Err(alloy_rlp::Error::Custom("branch node with value"))
                }
            }
            (32, false) => {
                if buf.length() < 32 {
                    Err(alloy_rlp::Error::InputTooShort)
                } else {
                    let bytes: [u8; 32] = (&buf[0..32]).try_into().unwrap();
                    buf.advance(32);
                    Ok(MptNodeData::Digest(B256::from(bytes)).into())
                }
            }
            _ => Err(alloy_rlp::Error::Custom("bad node encoding")),
        }
    }
}

fn rlp_parse_head(buf: &mut &[u8]) -> alloy_rlp::Result<(usize, bool)> {
    let head = alloy_rlp::Header::decode(buf)?;
    let mut result = 0;
    if head.list {
        let mut buf = &buf[0..head.payload_length];
        while !buf.is_empty() {
            let item_head = alloy_rlp::Header::decode(&mut buf)?;
            buf.advance(item_head.payload_length);
            result += 1;
        }
    } else {
        result = head.payload_length;
    }
    Ok((result, head.list))
}
