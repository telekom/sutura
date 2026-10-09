//-----------------------------------------------------------------------------
// Copyright (c) 2026, Oracle and/or its affiliates.
//
// This software is dual-licensed to you under the Universal Permissive License
// (UPL) 1.0 as shown at https://oss.oracle.com/licenses/upl and Apache License
// 2.0 as shown at http://www.apache.org/licenses/LICENSE-2.0. You may choose
// either license.
//
// If you elect to accept the software under the Apache License, Version 2.0,
// the following applies:
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//-----------------------------------------------------------------------------

//-----------------------------------------------------------------------------
// packet.rs
//
// Defines the structure corresponding to a full packet.
//-----------------------------------------------------------------------------

use crate::constants;
use crate::error::Error;

pub(crate) struct Packet {
    pub(crate) packet_type: u8,
    pub(crate) packet_flags: u8,
    pub(crate) data_flags: u16,
    pub(crate) buf: Vec<u8>,
}

impl Packet {
    /// Returns a boolean indicating if the packet contains the end of response
    /// marker.
    pub(crate) fn has_end_of_response(&self) -> bool {
        if self.packet_type != constants::PACKET_TYPE_DATA
            || self.data_flags & constants::TTC_DATA_FLAGS_END_OF_RESPONSE != 0
            || self.data_flags & constants::TTC_DATA_FLAGS_EOF != 0
        {
            true
        } else if let Some(last_byte) = self.buf.last() {
            *last_byte == constants::TTC_MSG_TYPE_END_OF_RESPONSE
        } else {
            false
        }
    }

    /// Returns a boolean indicating if the packet refers to a marker packet
    /// containing a reset marker type.
    pub(crate) fn has_reset_marker(&self) -> bool {
        if self.packet_type == constants::PACKET_TYPE_MARKER {
            self.buf.get(2) == Some(&constants::MARKER_TYPE_RESET)
        } else {
            false
        }
    }

    /// Returns the size of the header. Data packets are 10 bytes (because they
    /// include the data flags, unlike other packets).
    pub(crate) fn header_size(&self) -> usize {
        if self.packet_type == constants::PACKET_TYPE_DATA {
            10
        } else {
            8
        }
    }

    /// Creates a new packet from the supplied buffer. A buffer shorter than
    /// the header of its packet type is an error.
    pub(crate) fn new(data: &[u8]) -> Result<Packet, Error> {
        let too_short = || Error::packet_too_short(data.len());
        let header = data.get(..8).ok_or_else(too_short)?;
        let packet_type = header[4];
        let packet_flags = header[5];
        let mut data_flags: u16 = 0;
        let mut buf = &data[8..];
        if packet_type == constants::PACKET_TYPE_DATA {
            let flags = data.get(8..10).ok_or_else(too_short)?;
            data_flags = u16::from_be_bytes([flags[0], flags[1]]);
            buf = &data[10..];
        }
        Ok(Packet {
            packet_type,
            packet_flags,
            data_flags,
            buf: buf.into(),
        })
    }
}
