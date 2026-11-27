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
// mod.rs (client module)
//
// Defines the structures used for handling the requests to and the responses
// from the server.
//-----------------------------------------------------------------------------

mod capabilities;

use std::mem;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::sync::Arc;

use crate::bind_params::BindParameters;
use crate::config::Address;
use crate::config::Config;
use crate::config::Description;
use crate::config::parse_redirect_data;
use crate::constants;
use crate::db_info::DbInfo;
use crate::dbobject::DbObjectTypeCache;
use crate::end_user_security_context::EndUserSecurityContext;
use crate::error::Error;
use crate::external_auth;
use crate::messages::AuthMessage;
use crate::messages::CommitMessage;
use crate::messages::ConnectMessage;
use crate::messages::DataTypesMessage;
use crate::messages::EofMessage;
use crate::messages::ExecuteMessage;
use crate::messages::FastAuthMessage;
use crate::messages::FetchMessage;
use crate::messages::FlushOutBindsMessage;
use crate::messages::LobOpMessage;
use crate::messages::LoginKey;
use crate::messages::LogoffMessage;
use crate::messages::MarkerMessage;
use crate::messages::Message;
use crate::messages::PingMessage;
use crate::messages::ProtocolMessage;
use crate::messages::RollbackMessage;
use crate::ora_type::OracleIntervalDS;
use crate::packet::Packet;
use crate::response::Response;
use crate::row::DbRow;
use crate::row::Row;
use crate::statement::CachedStatement;
use crate::statement::StatementCache;
use crate::statement::StatementOptions;
use crate::transport::Transport;
use crate::write_buffer::WriteBuffer;

use capabilities::Capabilities;

const TTC_SECURITY_CONTEXT_ATTACH_FLAG: u32 = 0x01;
const TTC_END_USER_SECURITY_CONTEXT_KEY: &str = "ORCL_XS_AUTHZ_CONTEXT";

pub struct Client {
    iam_auth: Option<(String, String)>,
    transport: Transport,
    caps: capabilities::Capabilities,
    config: Config,
    combo_key: Option<LoginKey>,
    dbobject_type_cache: DbObjectTypeCache,
    charset_id: u16,
    ncharset_id: u16,
    statement_cache: StatementCache,
    drcp_establish_session: bool,
    in_request: bool,
    override_ttc_field_version: u8,
    pending_error_num: usize,
    pending_action: Option<Vec<u8>>,
    pending_client_identifier: Option<Vec<u8>>,
    pending_client_info: Option<Vec<u8>>,
    pending_db_op: Option<Vec<u8>>,
    pending_module: Option<Vec<u8>>,
    pending_ha_readiness: bool,
    pending_session_state: u8,
    transaction_in_progress: bool,
    pool_id: String,
    last_warning: Option<String>,
    security_context: Option<EndUserSecurityContext>,
    temp_lobs_to_close: Vec<Vec<u8>>,
    db_timezone_offset: OracleIntervalDS,
}

pub(crate) type ClientRef = std::sync::Arc<std::sync::Mutex<Client>>;

impl Client {
    /// Performs a round trip to the database by sending the message and
    /// receiving back the database response.
    fn perform_round_trip(
        &mut self,
        message: &mut impl Message,
    ) -> Result<Response, Error> {
        message.pre_process(self)?;
        self.send_message(message)?;
        let mut response = Response::new();
        if let Err(e) = self.receive_response(message, &mut response) {
            response.cleanup_pending_values(self);
            Err(e)
        } else {
            Ok(response)
        }
    }

    /// Process a control packet received from the database.
    fn process_control_packet(&mut self, packet: Packet) -> Result<(), Error> {
        let mut resp = Response::new();
        resp.add_packets(vec![packet]);
        let control_type = resp.read_u16be()?;
        if control_type == constants::TTC_CONTROL_TYPE_INBAND_NOTIF {
            resp.advance(4)?;
            self.pending_error_num = resp.read_u32be()? as usize;
        }
        Ok(())
    }

    /// Processes the call status flags returned by the server.
    fn process_call_status(&mut self, call_status: u32) {
        self.transaction_in_progress = call_status & 0x00000002 != 0
    }

    /// Processes a single message and receives back the response. If the
    /// message requires resending, that is done and only the second response
    /// is returned.
    fn process_message(
        &mut self,
        message: &mut impl Message,
    ) -> Result<Response, Error> {
        let mut response = self.perform_round_trip(message)?;
        if message.renegotiation_needed() {
            let mut data_types_message = DataTypesMessage::new();
            response = self.perform_round_trip(&mut data_types_message)?;
            message.deserialize(self, &mut response)?;
        } else if message.resend_needed() {
            response = self.perform_round_trip(message)?;
        }
        Ok(response)
    }

    /// Receives a data packet from the database. Control packets and marker
    /// packets are processed. Only data packets are returned.
    fn receive_data_packet(&mut self) -> Result<(Packet, bool), Error> {
        loop {
            match self.transport.receive_packet() {
                Ok(packet) => match packet.packet_type {
                    constants::PACKET_TYPE_CONTROL => {
                        self.process_control_packet(packet)?;
                        continue;
                    }
                    constants::PACKET_TYPE_MARKER => {
                        let packet = self
                            .reset(packet.has_reset_marker())
                            .map_err(|err| self.unrecoverable_error(err))?;
                        return Ok((packet, false));
                    }
                    _ => return Ok((packet, true)),
                },
                Err(err) => {
                    if err.is_call_timeout_exceeded() {
                        let packet = self.recover_from_error(err)?;
                        return Ok((packet, false));
                    }
                    return Err(err);
                }
            }
        }
    }

    /// Returns the list of packets making up the response from the server (if
    /// the database is capable of indicating the end of its response) or a
    /// single data packet which may not be the entire response.
    fn receive_packets(&mut self) -> Result<Vec<Packet>, Error> {
        let mut packets = Vec::<Packet>::new();
        let supports_end_of_response = self.supports_end_of_response();
        loop {
            let (packet, check_end_of_response) =
                self.receive_data_packet()?;
            let wait_for_more = check_end_of_response
                && supports_end_of_response
                && !packet.has_end_of_response();
            packets.push(packet);
            if !wait_for_more {
                break;
            }
        }
        Ok(packets)
    }

    /// Returns the response of the database to the message sent by the client.
    fn receive_response(
        &mut self,
        message: &mut impl Message,
        response: &mut Response,
    ) -> Result<(), Error> {
        response.add_packets(self.receive_packets()?);
        message.pre_deserialize(self, response);
        while let Err(e) = message.deserialize(self, response) {
            if e.is_out_of_data() {
                response.add_packets(self.receive_packets()?);
                continue;
            }
            return Err(e);
        }
        if response.get_flush_out_binds() {
            self.send_message(&mut FlushOutBindsMessage)?;
            response.add_packets(self.receive_packets()?);
            message.deserialize(self, response)?;
        }
        message.post_deserialize(self, response)?;
        self.process_call_status(response.call_status());
        if let Some(warning) = response.take_warning() {
            self.last_warning = Some(warning);
        }
        Ok(())
    }

    /// Called when a timeout occurs and attempts to recover from it by sending
    /// an interrupt to the server and waiting for its response. If any error
    /// occurs during the recovery process, the connection is deemed unusable
    /// and the connection closed - but the error returned is always the
    /// timeout that triggered recovery, never the recovery attempt's own
    /// failure: a caller checking `is_call_timeout_exceeded()` must see the
    /// same answer whether or not the interrupt/reset handshake itself
    /// succeeded, since the handshake commonly fails for the same reason the
    /// original call did (the peer that already timed out the call is often
    /// the one that then fails to answer the interrupt too).
    fn recover_from_error(&mut self, err: Error) -> Result<Packet, Error> {
        if self
            .send_marker(constants::MARKER_TYPE_INTERRUPT)
            .and_then(|_| self.reset(false))
            .is_err()
        {
            let _ = self.transport.close();
        }
        Err(err)
    }

    /// Resets the transport after an error has taken place. All packets
    /// received prior to a reset marker packet being received are discarded.
    /// The packet received after the reset has completed is returned to the
    /// caller. Note that some databases return multiple reset markers so
    /// these are also accommodated.
    ///
    /// On a native-encrypted transport the marker packets this exchanges are
    /// the protocol's own unprotected marker/break packets (the transport
    /// admits exactly those unprotected), so a server-signalled break - e.g. a
    /// statement that fails with an ORA error - reaches the caller as that
    /// error instead of a transport refusal. But the packet types admitted
    /// unprotected are exactly marker packets, never data: on native, the
    /// packet this returns must be a protected `DATA` packet read through
    /// `protection.open` like any other, and a `DATA` packet arriving before
    /// the reset marker, or any non-reset marker packet taking its place
    /// after it, is refused by naming the expected and the arrived packet
    /// type - the same unprotected bytes that are legitimately a break marker
    /// must never be forwarded to the caller as if they were the awaited
    /// response, and a protected packet arriving early must never be
    /// silently discarded as if it were pre-reset noise.
    ///
    /// On native encryption, the server independently re-derives its MAC
    /// keystreams the moment it reads the client's answering reset marker,
    /// chaining forward from their current state rather than restarting from
    /// the session key - it does not wait to see anything come back first,
    /// since sending that marker is itself what moves it to the new state.
    /// The client reaches that same point the moment it, in turn, decides to
    /// answer: it rekeys immediately after sending its own reset marker,
    /// before reading anything further, mirroring go-ora's `processMarker`
    /// (which re-initialises unconditionally on every marker it reads,
    /// without inspecting that marker's type) rather than waiting to
    /// recognise a reset-typed echo on the wire - any data still in flight
    /// from before the break remains under the old keys, exactly like the
    /// server's own traffic does, and only the record the break/reset
    /// handshake is actually waiting for is read under the new ones.
    ///
    /// `processMarker` also reads exactly one more packet right after
    /// rekeying and discards it - normally the peer's own reset-marker echo.
    /// Here that slot accepts only a marker: a protected `DATA` record in it
    /// is refused, never dropped, because dropping an authenticated record
    /// would truncate the stream. **Limit:** a second, distinct marker read
    /// later in the loop below does not trigger another send/rekey/discard
    /// cycle; it only sets `reset_received`.
    fn reset(&mut self, reset_received: bool) -> Result<Packet, Error> {
        log::debug!(
            "reset: send MARKER type=reset (rekey_count={})",
            self.transport.rekey_count(),
        );
        self.send_marker(constants::MARKER_TYPE_RESET)?;
        let native = self.transport.uses_native();
        let mut reset_received = reset_received;
        if native {
            self.transport.rekey_native_protection_for_reset()?;
            let discarded = self.transport.receive_packet()?;
            log::debug!(
                "reset: recv {} (reset_marker={}, reset_received={}, {} byte record, \
                 rekey_count={})",
                crate::transport::packet_type_label(discarded.packet_type),
                discarded.has_reset_marker(),
                reset_received,
                discarded.buf.len(),
                self.transport.rekey_count(),
            );
            if discarded.has_reset_marker() {
                // This is the normal case - the peer's own answering reset marker,
                // read and discarded unconditionally, mirroring go-ora's
                // `processMarker`, which does not inspect it either.
                reset_received = true;
            } else if discarded.packet_type == constants::PACKET_TYPE_DATA {
                // go-ora drops whatever fills this slot. A protected record here is
                // either stale traffic (no reset marker seen yet) or the awaited
                // answer arriving without the echo; dropping either would truncate
                // the authenticated stream, so refuse instead.
                let expected = if reset_received {
                    "the peer's reset-marker echo"
                } else {
                    "an unprotected MARKER before the reset marker"
                };
                return Err(Error::native_encryption_refused(format!(
                    "expected {expected}, got a protected {}",
                    crate::transport::packet_type_label(constants::PACKET_TYPE_DATA),
                )));
            }
            // Any other unprotected marker here (e.g. a second, distinct marker type)
            // is also just discarded, matching go-ora's unconditional single discard.
        }
        loop {
            let packet = self.transport.receive_packet()?;
            log::debug!(
                "reset: recv {} (reset_marker={}, reset_received={}, {} byte record, \
                 rekey_count={})",
                crate::transport::packet_type_label(packet.packet_type),
                packet.has_reset_marker(),
                reset_received,
                packet.buf.len(),
                self.transport.rekey_count(),
            );
            if packet.has_reset_marker() {
                reset_received = true;
            } else if reset_received {
                if native && packet.packet_type != constants::PACKET_TYPE_DATA
                {
                    return Err(Error::native_encryption_refused(format!(
                        "expected a protected {} after the reset marker, got an unprotected {}",
                        crate::transport::packet_type_label(
                            constants::PACKET_TYPE_DATA
                        ),
                        crate::transport::packet_type_label(
                            packet.packet_type
                        ),
                    )));
                }
                return Ok(packet);
            } else if native
                && packet.packet_type == constants::PACKET_TYPE_DATA
            {
                return Err(Error::native_encryption_refused(format!(
                    "expected an unprotected {} before the reset marker, got a protected {}",
                    crate::transport::packet_type_label(
                        constants::PACKET_TYPE_MARKER
                    ),
                    crate::transport::packet_type_label(packet.packet_type),
                )));
            }
        }
    }

    /// Sends a marker packet of the specified type to the database.
    fn send_marker(&mut self, marker_type: u8) -> Result<(), Error> {
        let mut message = MarkerMessage::new(marker_type);
        self.send_message(&mut message)
    }

    /// Sends a message to the database.
    fn send_message(
        &mut self,
        message: &mut impl Message,
    ) -> Result<(), Error> {
        let mut buf = WriteBuffer::new();
        if message.get_packet_type() == constants::PACKET_TYPE_DATA {
            self.write_piggybacks(&mut buf);
        }
        message.serialize(self, &mut buf);
        self.transport.send_packets(
            message.get_packet_type(),
            message.get_packet_flags(),
            message.get_data_flags(),
            buf.get_buf(),
        )?;
        if message.extended_data_needed() {
            buf.clear();
            message.serialize_extended_data(self, &mut buf);
            self.transport.send_packets(
                constants::PACKET_TYPE_DATA,
                0,
                0,
                buf.get_buf(),
            )?;
        }
        Ok(())
    }

    /// Called when an unrecoverable error has taken place and the connection
    /// is no longer deemed usable. The connection is closed; the triggering
    /// error is returned unchanged so the caller still sees its real cause
    /// (an ORA code read from the server, or a genuine transport refusal)
    /// rather than a generic "unable to recover" message that would mask it.
    fn unrecoverable_error(&mut self, err: Error) -> Error {
        let _ = self.transport.close();
        err
    }

    /// Writes the close cursors piggyback.
    fn write_piggyback_close_cursors(&mut self, buf: &mut WriteBuffer) {
        buf.write_piggyback_header(self, constants::TTC_RPC_CLOSE_CURSORS);
        buf.write_u8(1); // pointer
        let cursors = self.statement_cache.take_cursors_to_close();
        let num_cursors: u32 = cursors.len().try_into().unwrap();
        buf.write_ub4(num_cursors);
        for cursor_id in cursors {
            buf.write_ub2(cursor_id);
        }
    }

    /// Writes the temporary LOB locators that can be freed by the server.
    fn write_piggyback_close_temp_lobs(&mut self, buf: &mut WriteBuffer) {
        let temp_lobs_to_close = mem::take(&mut self.temp_lobs_to_close);
        let total_size: usize = temp_lobs_to_close.iter().map(Vec::len).sum();
        buf.write_piggyback_header(self, constants::TTC_RPC_LOB_OP);
        buf.write_u8(1); // pointer (temporary LOB array)
        buf.write_ub4(total_size.try_into().unwrap());
        buf.write_u8(0); // destination locator pointer
        buf.write_ub4(0);
        buf.write_ub4(0); // source locator offset
        buf.write_ub4(0);
        buf.write_u8(0); // source offset pointer
        buf.write_u8(0); // destination offset pointer
        buf.write_u8(0); // character set pointer
        buf.write_ub4(
            constants::TTC_LOB_OP_FREE_TEMP | constants::TTC_LOB_OP_ARRAY,
        );
        buf.write_u8(0); // SCN pointer
        buf.write_ub4(0);
        buf.write_ub8(0);
        buf.write_ub8(0);
        buf.write_u8(0); // amount pointer
        buf.write_u8(0); // array destination locator pointer
        buf.write_ub4(0);
        buf.write_u8(0); // array source locator pointer
        buf.write_ub4(0);
        buf.write_u8(0); // array source offset pointer
        buf.write_ub4(0);
        for locator in temp_lobs_to_close {
            buf.write_bytes(&locator);
        }
    }

    /// Writes the end-to-end attributes piggyback.
    fn write_piggyback_end_to_end(&mut self, buf: &mut WriteBuffer) {
        // determine which flags to send
        let mut flags = 0;
        if self.pending_action.is_some() {
            flags |= constants::TTC_END_TO_END_FLAGS_ACTION;
        }
        if self.pending_client_identifier.is_some() {
            flags |= constants::TTC_END_TO_END_FLAGS_CLIENT_IDENTIFIER;
        }
        if self.pending_client_info.is_some() {
            flags |= constants::TTC_END_TO_END_FLAGS_CLIENT_INFO;
        }
        if self.pending_db_op.is_some() {
            flags |= constants::TTC_END_TO_END_FLAGS_DB_OP;
        }
        if self.pending_module.is_some() {
            // setting the flags for module by itself results in an error so
            // always set the flag for action as well
            flags |= constants::TTC_END_TO_END_FLAGS_MODULE
                | constants::TTC_END_TO_END_FLAGS_ACTION;
        }

        // write initial packet data
        buf.write_piggyback_header(
            self,
            constants::TTC_RPC_SET_END_TO_END_ATTR,
        );
        buf.write_u8(0); // pointer (cidnam)
        buf.write_u8(0); // pointer (cidser)
        buf.write_ub4(flags);

        // write client identifier header info
        if let Some(value) = &self.pending_client_identifier {
            buf.write_u8(1); // pointer (client identifier)
            buf.write_ub4(value.len().try_into().unwrap());
        } else {
            buf.write_u8(0);
            buf.write_ub4(0);
        }

        // write module header info
        if let Some(value) = &self.pending_module {
            buf.write_u8(1); // pointer (module)
            buf.write_ub4(value.len().try_into().unwrap());
        } else {
            buf.write_u8(0);
            buf.write_ub4(0);
        }

        // write action header info
        if let Some(value) = &self.pending_action {
            buf.write_u8(1); // pointer (action)
            buf.write_ub4(value.len().try_into().unwrap());
        } else {
            buf.write_u8(0);
            buf.write_ub4(0);
        }

        // write unsupported bits
        buf.write_u8(0); // pointer (cideci)
        buf.write_ub4(0); // length (cideci)
        buf.write_u8(0); // cidcct
        buf.write_ub4(0); // cidecs

        // write client info header info
        if let Some(value) = &self.pending_client_info {
            buf.write_u8(1); // pointer (client info)
            buf.write_ub4(value.len().try_into().unwrap());
        } else {
            buf.write_u8(0);
            buf.write_ub4(0);
        }

        // write more unsupported bits
        buf.write_u8(0); // pointer (cidkstk)
        buf.write_ub4(0); // length (cidkstk)
        buf.write_u8(0); // pointer (cidktgt)
        buf.write_ub4(0); // length (cidktgt)

        // write database operation header info
        if let Some(value) = &self.pending_db_op {
            buf.write_u8(1); // pointer (database operation)
            buf.write_ub4(value.len().try_into().unwrap());
        } else {
            buf.write_u8(0);
            buf.write_ub4(0);
        }

        // write strings (and reset pending status)
        for pending_value in [
            self.pending_client_identifier.take(),
            self.pending_module.take(),
            self.pending_action.take(),
            self.pending_client_info.take(),
            self.pending_db_op.take(),
        ] {
            if let Some(value) = pending_value
                && !value.is_empty()
            {
                buf.write_bytes_with_length(&value);
            }
        }
    }

    /// Writes the HA readiness piggyback.
    fn write_piggyback_ha_readiness(&mut self, buf: &mut WriteBuffer) {
        const NAMESPACE: &[u8] = b"ORA$HA";
        let num_pairs: u32 = if self.pool_id.is_empty() { 1 } else { 3 };
        buf.write_piggyback_header(self, constants::TTC_RPC_SET_KEY_VALUE);
        buf.write_u8(1); // pointer (namespace)
        buf.write_ub4(NAMESPACE.len().try_into().unwrap());
        buf.write_u8(1); // pointer (num key/value pairs)a
        buf.write_ub4(num_pairs);
        buf.write_ub2(0x21); // flag (set HA values)
        buf.write_u8(0); // pointer (unused)
        buf.write_bytes_with_length(NAMESPACE);
        if !self.pool_id.is_empty() {
            // key/value pair 1
            buf.write_bytes_with_double_length(Some(b"CONNECTION_POOL"));
            buf.write_bytes_with_double_length(Some(b"RUST"));
            buf.write_ub4(0);

            // key/value pair 2
            buf.write_bytes_with_double_length(Some(b"CONNECTION_POOL_ID"));
            buf.write_bytes_with_double_length(Some(self.pool_id.as_bytes()));
            buf.write_ub4(0);
        }

        // key/value pair 3
        buf.write_bytes_with_double_length(Some(b"INBAND_NOTIFICATION"));
        buf.write_bytes_with_double_length(Some(b"1"));
        buf.write_ub4(0);

        self.pending_ha_readiness = false;
    }

    /// Writes the Deep Data Security context piggyback expected by the TTC
    /// protocol.
    fn write_piggyback_end_user_security_context(
        &self,
        buf: &mut WriteBuffer,
        context: &EndUserSecurityContext,
    ) {
        let oson_bytes = context.oson_bytes();
        buf.write_piggyback_header(
            self,
            constants::TTC_RPC_END_USER_SECURITY_CONTEXT,
        );
        buf.write_ub4(TTC_SECURITY_CONTEXT_ATTACH_FLAG);
        buf.write_u8(1); // pointer
        buf.write_ub4(1); // number of key/value pairs

        buf.write_ub4(0); // flags
        buf.write_bytes_with_double_length(Some(
            TTC_END_USER_SECURITY_CONTEXT_KEY.as_bytes(),
        ));
        buf.write_bytes_with_double_length(None); // text
        buf.write_bytes_with_double_length(Some(&oson_bytes));
    }

    /// Writes the session state piggyback. This is used to let the database
    /// know when the client is beginning and ending a request. The database
    /// uses this information to optimise its resources.
    fn write_piggyback_session_state(&mut self, buf: &mut WriteBuffer) {
        let state = self.pending_session_state
            | constants::TTC_SESSION_STATE_EXPLICIT_BOUNDARY;
        buf.write_piggyback_header(self, constants::TTC_RPC_SESSION_STATE);
        buf.write_ub8(state as u64);
        self.pending_session_state = 0;
    }

    /// Writes all of the piggybacks for the given round trip.
    fn write_piggybacks(&mut self, buf: &mut WriteBuffer) {
        if let Some(context) = self.security_context.as_ref() {
            self.write_piggyback_end_user_security_context(buf, context);
        }
        if self.statement_cache.has_cursors_to_close()
            && !self.drcp_establish_session
        {
            self.write_piggyback_close_cursors(buf);
        }
        if self.pending_action.is_some()
            || self.pending_client_identifier.is_some()
            || self.pending_client_info.is_some()
            || self.pending_db_op.is_some()
            || self.pending_module.is_some()
        {
            self.write_piggyback_end_to_end(buf);
        }
        if self.pending_session_state != 0 {
            self.write_piggyback_session_state(buf);
        }
        if !self.temp_lobs_to_close.is_empty() {
            self.write_piggyback_close_temp_lobs(buf);
        }
        if self.pending_ha_readiness {
            self.write_piggyback_ha_readiness(buf);
        }
    }

    /// Adds a temporary LOB locator to the list of locators that will be
    /// freed by the server on the next round trip.
    pub(crate) fn add_lob_to_close(&mut self, locator: Vec<u8>) {
        let flags1 = locator[constants::TTC_LOB_LOC_OFFSET_FLAG_1];
        let flags4 = locator[constants::TTC_LOB_LOC_OFFSET_FLAG_4];

        if flags1 & constants::TTC_LOB_LOC_FLAGS_ABSTRACT != 0
            || flags4 & constants::TTC_LOB_LOC_FLAGS_TEMP != 0
        {
            self.temp_lobs_to_close.push(locator);
        }
    }

    /// Makes any necessary adjustment to the compile time capabilities based
    /// on the server's compile time capabilities.
    pub(crate) fn adjust_for_server_compile_caps(&mut self, caps: &[u8]) {
        self.caps.adjust_for_server_compile_caps(caps);
    }

    /// Makes any necessary adjustment to the runtime capabilities based on the
    /// server's runtime capabilities.
    pub(crate) fn adjust_for_server_runtime_caps(&mut self, caps: &[u8]) {
        self.caps.adjust_for_server_runtime_caps(caps);
    }

    /// Changes the password of the currently logged on user.
    pub(crate) fn change_password(
        &mut self,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), Error> {
        // a combo key is only available when the connection was established
        // with a password, which is required in order to change one
        let Some(combo_key) = self.combo_key else {
            return Err(Error::no_credentials());
        };
        let mut temp_config = self
            .config
            .clone()
            .set_password(old_password)
            .set_new_password(new_password);
        mem::swap(&mut temp_config, &mut self.config);
        let mut auth_message = AuthMessage::new();
        auth_message.set_combo_key(&combo_key);
        let result = self.process_message(&mut auth_message);
        mem::swap(&mut temp_config, &mut self.config);
        result.map(|_| ())
    }

    /// Removes any Deep Data Security context stored on this client/session.
    pub(crate) fn clear_end_user_security_context(&mut self) {
        self.security_context = None;
    }

    /// Closes the connection to the database.
    pub(crate) fn close(&mut self) -> Result<(), Error> {
        self.dbobject_type_cache.clear();
        self.end_request()?;
        self.process_message(&mut LogoffMessage::new())?;
        self.send_message(&mut EofMessage::new())?;
        self.transport.close()
    }

    /// Ensures that a statement is closed and the cursor number cleared for
    /// errors that require it.
    pub(crate) fn close_cursor(&mut self, statement: &mut CachedStatement) {
        self.statement_cache.close_cursor(statement);
        statement.clear_cursor();
    }

    /// Commits any pending transaction.
    pub(crate) fn commit(&mut self) -> Result<(), Error> {
        self.process_message(&mut CommitMessage::new())?;
        Ok(())
    }

    pub(crate) fn encrypted_transport(&self) -> bool {
        self.transport.uses_tls() || self.transport.uses_native()
    }

    /// Returns the configuration associated with the client.
    pub(crate) fn config(&self) -> &Config {
        &self.config
    }

    /// Establishes a connection to the database and returns the client object
    /// as well as database info.
    pub(crate) fn connect(&mut self) -> Result<DbInfo, Error> {
        let mut result = Err(Error::unexpected_result());
        let options = self.config.get_options()?;
        for option in options.iter() {
            match option.connect(self) {
                Ok(_) => {
                    return self.connect_phase_two();
                }
                Err(err) => {
                    result = Err(err);
                }
            }
        }
        result
    }

    /// Method for performing the required steps for establishing a connection
    /// within the scope of a retry. Once the accept packet has been received,
    /// no further retries are attempted.
    pub(crate) fn connect_phase_one(
        &mut self,
        sock_addr: SocketAddr,
        connect_data: &str,
        address: &Address,
        description: &Description,
    ) -> Result<(), Error> {
        if self.config.uses_external_auth() && address.protocol() != "tcps" {
            return Err(Error::external_auth_requires_tcps());
        }
        if self.config.native_encryption() && address.protocol() != "tcp" {
            return Err(Error::native_encryption_refused(
                "native mode requires TCP",
            ));
        }
        let stream =
            if let Some(timeout) = self.config.transport_connect_timeout() {
                TcpStream::connect_timeout(&sock_addr, timeout)?
            } else {
                TcpStream::connect(sock_addr)?
            };
        self.transport.connect(stream, address, &self.config)?;
        self.transport.set_max_packet_size(description.sdu() as usize);
        let mut address = address.clone();
        let mut connect_data = connect_data.to_string();
        let mut connect_message =
            ConnectMessage::new(&connect_data, &address, description);
        while !connect_message.accepted {
            self.process_message(&mut connect_message)?;
            if connect_message.redirect_data_len > 0 {
                self.config.check_redirect_allowed()?;
                let mut response = Response::new();
                self.receive_response(&mut connect_message, &mut response)?;
                let redirect_data =
                    connect_message.redirect_data.take().unwrap();
                if let Some((before, after)) =
                    redirect_data.split_once('\u{0}')
                {
                    address = parse_redirect_data(before)?;
                    if self.config.uses_external_auth()
                        && address.protocol() != "tcps"
                    {
                        return Err(Error::external_auth_requires_tcps());
                    }
                    connect_data = after.to_string();
                    let new_stream =
                        TcpStream::connect((address.host(), address.port()))?;
                    self.transport.connect(
                        new_stream,
                        &address,
                        &self.config,
                    )?;
                    connect_message = ConnectMessage::new(
                        &connect_data,
                        &address,
                        description,
                    );
                    connect_message.packet_flags =
                        constants::PACKET_FLAGS_REDIRECT;
                } else {
                    return Err(Error::invalid_redirect(&redirect_data));
                }
            }
            if connect_message.tls_renegotiation_needed {
                self.transport.negotiate_tls(address.host(), &self.config)?;
            }
        }
        self.caps.adjust_for_protocol(
            connect_message.protocol_version,
            connect_message.protocol_flags,
        );
        self.transport.set_full_packet_size();
        if self.config.native_encryption() {
            if !connect_message.native_available {
                let _ = self.transport.close();
                return Err(Error::native_encryption_refused(
                    "peer disabled native negotiation",
                ));
            }
            self.transport
                .negotiate_native(self.config.transport_connect_timeout())?;
        }

        // OCI IAM token based authentication requires a header identifying
        // the service and the address of the database, signed with the
        // private key supplied in the configuration
        if let Some(private_key) = self.config.get_private_key_bytes() {
            let peer_addr = self.transport.peer_addr()?;
            let host_info = format!("{}:{}", peer_addr.ip(), peer_addr.port());
            self.iam_auth = Some(external_auth::get_iam_auth(
                &private_key,
                description.service_name(),
                &host_info,
            )?);
        }
        Ok(())
    }

    /// Performs the second phase of connecting to the database. Any errors
    /// that take place during this phase are returned directly to the caller.
    pub(crate) fn connect_phase_two(&mut self) -> Result<DbInfo, Error> {
        // if fast authentication is possible, use it
        if self.caps.supports_fast_auth() {
            let mut fast_auth_message = FastAuthMessage::new();
            self.override_ttc_field_version =
                constants::FAST_AUTH_TTC_FIELD_VERSION;
            self.process_message(&mut fast_auth_message)?;
            let auth_message = fast_auth_message.auth_message();
            // the fast authentication message may already have carried the
            // second phase of authorization; only perform it as a separate
            // round trip when the server asked for more
            if auth_message.resend_needed() {
                self.process_message(auth_message)?;
            }
            self.post_connect(auth_message)

        // otherwise, do the normal authentication; disable end of response
        // for the first two messages as the server does not send an end of
        // response for those messages
        } else {
            let orig_value = self.caps.set_supports_end_of_response(false);
            let mut protocol_message = ProtocolMessage::new();
            let mut data_types_message = DataTypesMessage::new();
            let mut auth_message = AuthMessage::new();
            self.process_message(&mut protocol_message)?;
            self.process_message(&mut data_types_message)?;
            self.caps.set_supports_end_of_response(orig_value);
            self.process_message(&mut auth_message)?;
            self.post_connect(&mut auth_message)
        }
    }

    /// Returns the database time zone offset.
    pub(crate) fn db_timezone_offset(
        &self,
    ) -> Result<chrono::Duration, Error> {
        if self.db_timezone_offset.days() != 0 {
            Err(Error::not_implemented("named database time zone".to_string()))
        } else {
            let offset = self.db_timezone_offset;
            Ok(chrono::Duration::hours(offset.hours().into())
                + chrono::Duration::minutes(offset.minutes().into()))
        }
    }

    /// Ends the current request against the database.This clears any end user
    /// security context and warnings, rolls back any open transactions and
    /// releases any session to the DRCP pool, if applicable.
    pub(crate) fn end_request(&mut self) -> Result<(), Error> {
        self.security_context = None;
        self.last_warning = None;
        self.transport.set_timeout(None)?;
        if self.in_request && self.pending_session_state != 0 {
            self.in_request = false;
        }
        if self.in_request || self.transaction_in_progress {
            if self.in_request {
                self.pending_session_state =
                    constants::TTC_SESSION_STATE_REQUEST_END;
                self.in_request = false;
            }
            self.process_message(&mut RollbackMessage::new())?;
        }
        Ok(())
    }

    /// Executes the given statement and returns a response. At this point
    /// binds have been checked and transformed (if needed) into the sequence
    /// required by the server. Statements that require single execution are
    /// performed once first before subsequent iterations are performed as a
    /// batch.
    pub(crate) fn execute(
        &mut self,
        statement: &mut CachedStatement,
        client_ref: &ClientRef,
        params: BindParameters,
        parse_only: bool,
    ) -> Result<Response, Error> {
        if params.num_rows() > 1 && statement.requires_single_execute() {
            let mut initial_resp = self.execute(
                statement,
                client_ref,
                params.slice(0, 1),
                false,
            )?;
            let mut final_resp = self.execute(
                statement,
                client_ref,
                params.slice(1, params.num_rows() - 1),
                false,
            )?;
            final_resp.transfer_info(&mut initial_resp);
            Ok(final_resp)
        } else {
            ExecuteMessage::check_executable(statement)?;
            let mut message =
                ExecuteMessage::new(statement, params, parse_only);
            let mut response = self.process_message(&mut message)?;
            response.complete_values(self, client_ref, statement)?;
            if statement.requires_define() {
                statement.clear_requires_define();
            }
            Ok(response)
        }
    }

    /// Performs a fetch and returns the database response.
    pub(crate) fn fetch(
        &mut self,
        statement: &mut CachedStatement,
        client_ref: &ClientRef,
        last_row: Option<DbRow>,
    ) -> Result<Response, Error> {
        let mut message = FetchMessage::new(statement, last_row);
        let mut response = self.process_message(&mut message)?;
        response.complete_values(self, client_ref, statement)?;
        Ok(response)
    }

    /// Fetches all rows from the database given the initial response returned
    /// for the execute (or an empty response if using a REF cursor).
    pub(crate) fn fetch_all_rows(
        &mut self,
        client_ref: &ClientRef,
        statement: &mut CachedStatement,
        mut response: Response,
    ) -> Result<Vec<Row>, Error> {
        let metadata = Arc::new(statement.out_metadata().to_vec());
        let mut all_rows = Vec::new();
        loop {
            let rows = response.take_rows().unwrap_or_default();
            let last_row = rows.last().cloned();
            all_rows
                .extend(rows.into_iter().map(|row| Row::new(&metadata, row)));
            if response.is_end_of_fetch() {
                break;
            }
            response = self.fetch(statement, client_ref, last_row)?;
        }
        Ok(all_rows)
    }

    /// Returns the call timeout set on the connection or an error if the
    /// connection is not currently established.
    pub(crate) fn get_call_timeout(
        &self,
    ) -> Result<Option<std::time::Duration>, Error> {
        self.transport.get_read_timeout()
    }

    /// Returns the compile time capabilities.
    pub(crate) fn get_compile_caps(&self) -> &[u8] {
        self.caps.compile_caps()
    }

    /// Returns the last warning that was generated by the client.
    pub(crate) fn get_last_warning(&self) -> Option<String> {
        self.last_warning.clone()
    }

    /// Returns the database character set id used for NCHAR data.
    pub(crate) fn get_ncharset_id(&self) -> u16 {
        self.ncharset_id
    }

    /// Returns the database character set used by object images.
    pub(crate) fn get_charset_id(&self) -> u16 {
        self.charset_id
    }

    /// Returns the runtime capabilities.
    pub(crate) fn get_runtime_caps(&self) -> &[u8] {
        self.caps.runtime_caps()
    }

    /// Gets a statement from the cache or creates (and possibly caches) a new
    /// one and returns it.
    pub(crate) fn get_statement(
        &mut self,
        sql: &str,
        cache_statement: bool,
        options: &StatementOptions,
    ) -> Result<CachedStatement, Error> {
        let mut info = self.statement_cache.get_statement(
            sql,
            cache_statement,
            options,
        )?;
        if self.drcp_establish_session {
            info.clear_cursor();
        }
        Ok(info)
    }

    /// Discovers a named object type while holding the client lock.
    pub(crate) fn get_type(
        &mut self,
        client_ref: &ClientRef,
        name: &str,
    ) -> Result<Arc<crate::DbObjectType>, Error> {
        // Discovery needs both mutable cache access and the locked client for
        // SQL.
        let mut cache = mem::take(&mut self.dbobject_type_cache);
        let result = cache.get_type(self, client_ref, name);
        self.dbobject_type_cache = cache;
        result
    }

    /// Returns the header and signature required for OCI IAM token based
    /// authentication, if a private key was supplied in the configuration.
    pub(crate) fn iam_auth(&self) -> Option<(&str, &str)> {
        self.iam_auth
            .as_ref()
            .map(|(header, signature)| (header.as_str(), signature.as_str()))
    }

    /// Returns the maximum string size for the database.
    pub(crate) fn max_string_size(&self) -> u32 {
        self.caps.max_string_size()
    }

    /// Returns a client that is not connected and supports the given TTC
    /// field version, for unit tests.
    #[cfg(test)]
    pub(crate) fn with_ttc_field_version(version: u8) -> Self {
        let mut client = Self::new(Config::default(), String::new());
        client.override_ttc_field_version = version;
        client
    }

    /// Creates a new client and returns it.
    pub(crate) fn new(config: Config, pool_id: String) -> Self {
        let cache_size = config.stmtcachesize();
        Self {
            transport: Transport::new(),
            caps: Capabilities::new(),
            config,
            combo_key: None,
            iam_auth: None,
            dbobject_type_cache: DbObjectTypeCache::default(),
            charset_id: 0,
            ncharset_id: 0,
            statement_cache: StatementCache::new(cache_size),
            drcp_establish_session: false,
            in_request: false,
            override_ttc_field_version: 0,
            pending_error_num: 0,
            pending_action: None,
            pending_client_identifier: None,
            pending_client_info: None,
            pending_db_op: None,
            pending_module: None,
            pending_ha_readiness: false,
            pending_session_state: 0,
            last_warning: None,
            security_context: None,
            transaction_in_progress: false,
            pool_id,
            temp_lobs_to_close: Vec::new(),
            db_timezone_offset: OracleIntervalDS::default(),
        }
    }

    /// Commits any pending transaction.
    pub(crate) fn ping(&mut self) -> Result<(), Error> {
        self.process_message(&mut PingMessage::new())?;
        Ok(())
    }

    /// Runs activities after the auth message has been processed. The auth
    /// message contains information about the database which is retained.
    pub(crate) fn post_connect(
        &mut self,
        auth_message: &mut AuthMessage,
    ) -> Result<DbInfo, Error> {
        let db_info = DbInfo::new(self, auth_message)?;
        self.combo_key = auth_message.take_combo_key();
        if self.transport.uses_native() {
            let Some(key) = self.combo_key.as_ref() else {
                let _ = self.transport.close();
                return Err(Error::native_encryption_refused(
                    "missing authentication key folding",
                ));
            };
            self.transport.fold_native_key(key.as_bytes())?;
        }
        let max_open_cursors = db_info.get_max_open_cursors();
        if max_open_cursors < self.statement_cache.max_size() {
            self.statement_cache.resize(max_open_cursors);
        }
        if self.caps.supports_ha_readiness() {
            self.pending_ha_readiness = true;
        }
        if !self.pool_id.is_empty() && self.caps.supports_request_boundaries()
        {
            self.pending_session_state =
                constants::TTC_SESSION_STATE_REQUEST_BEGIN;
            self.in_request = true;
        }
        Ok(db_info)
    }

    /// Processes a LOB operation.
    pub(crate) fn process_lob_op(
        &mut self,
        message: &mut LobOpMessage,
    ) -> Result<(), Error> {
        self.process_message(message)?;
        Ok(())
    }

    /// Returns whether the client should be closed based on the pending error
    /// number returned by the latest control packet.
    pub(crate) fn requires_close(&self) -> bool {
        self.pending_error_num == constants::DB_ERR_NUM_SESSION_SHUTDOWN
    }

    /// Resets the TTC field version back to the default value. The necessity
    /// of a reset value is due to the intricacies surrounding fast
    /// authentication.
    pub(crate) fn reset_ttc_field_version(&mut self) {
        self.override_ttc_field_version = 0;
    }

    /// Rolls back any pending transaction.
    pub(crate) fn rollback(&mut self) -> Result<(), Error> {
        self.process_message(&mut RollbackMessage::new())?;
        Ok(())
    }

    /// Returns a statement to the statement cache for subsequent use, or
    /// adds it to the list of cursors to close.
    pub(crate) fn return_statement(&mut self, statement: &CachedStatement) {
        self.statement_cache.return_statement(statement);
    }

    /// Sets the call timeout to use on the connection or an error if the
    /// connection is not currently established.
    pub(crate) fn set_call_timeout(
        &self,
        duration: Option<std::time::Duration>,
    ) -> Result<(), Error> {
        self.transport.set_timeout(duration)
    }

    /// Sets the database time zone offset.
    pub(crate) fn set_db_timezone_offset(&mut self, value: OracleIntervalDS) {
        self.db_timezone_offset = value;
    }

    /// Sets the ids of the character sets in use by the database. The first
    /// one is the character set used for CHAR data and the second one is the
    /// character set used for NCHAR data.
    pub(crate) fn set_charset_ids(
        &mut self,
        charset_id: u16,
        ncharset_id: u16,
    ) {
        self.charset_id = charset_id;
        self.ncharset_id = ncharset_id;
    }

    /// Stores a Deep Data Security context after validating connection and
    /// server support.
    pub(crate) fn set_end_user_security_context(
        &mut self,
        context: EndUserSecurityContext,
    ) -> Result<(), Error> {
        if !self.transport.uses_tls() {
            return Err(Error::end_user_security_context_requires_tcps());
        }
        if !self.caps.supports_end_user_security_context() {
            return Err(Error::unsupported_deep_data_security_feature());
        }
        self.security_context = Some(context);
        Ok(())
    }

    /// Sets the pending end-to-end attribute (action) which will be sent on
    /// the next round trip to the database.
    pub(crate) fn set_pending_action(&mut self, value: &str) {
        self.pending_action = Some(value.as_bytes().to_vec());
    }

    /// Sets the pending end-to-end attribute (client_identifier) which will be
    /// sent on the next round trip to the database.
    pub(crate) fn set_pending_client_identifier(&mut self, value: &str) {
        self.pending_client_identifier = Some(value.as_bytes().to_vec());
    }

    /// Sets the pending end-to-end attribute (client_info) which will be sent
    /// on the next round trip to the database.
    pub(crate) fn set_pending_client_info(&mut self, value: &str) {
        self.pending_client_info = Some(value.as_bytes().to_vec());
    }

    /// Sets the pending end-to-end attribute (db_op) which will be sent on
    /// the next round trip to the database.
    pub(crate) fn set_pending_db_op(&mut self, value: &str) {
        self.pending_db_op = Some(value.as_bytes().to_vec());
    }

    /// Sets the pending end-to-end attribute (module) which will be sent on
    /// the next round trip to the database.
    pub(crate) fn set_pending_module(&mut self, value: &str) {
        self.pending_module = Some(value.as_bytes().to_vec());
    }

    /// Returns whether the database supports the "end of response" flag.
    pub(crate) fn supports_end_of_response(&self) -> bool {
        self.caps.supports_end_of_response()
    }

    /// Returns whether the client supports a particular TTC field version.
    pub(crate) fn supports_ttc_field_version(&self, version: u8) -> bool {
        if self.override_ttc_field_version != 0 {
            self.override_ttc_field_version >= version
        } else {
            self.caps.supports_ttc_field_version(version)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A failed interrupt/reset recovery must not replace the timeout that
    /// triggered it with a generic recovery error - an unconnected client's
    /// own recovery attempt fails the same way a peer that already timed out
    /// the original call commonly does, so this drives the real failure path
    /// without a live socket.
    #[test]
    fn recovery_failure_keeps_the_timeout_kind() {
        let mut client = Client::with_ttc_field_version(0);
        let timeout = Error::from(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "idle socket",
        ));
        assert!(timeout.is_call_timeout_exceeded());

        let err = match client.recover_from_error(timeout) {
            Ok(_) => panic!("an unconnected client cannot recover"),
            Err(err) => err,
        };
        assert!(
            err.is_call_timeout_exceeded(),
            "recovery failure must surface the original timeout, not a generic recovery error: {err}"
        );
    }

    use std::io::{Read, Write};

    use crate::nne;

    fn raw_packet(packet_type: u8, payload: &[u8]) -> Vec<u8> {
        let mut output = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        output.extend_from_slice(&[packet_type, 0, 0, 0]);
        output.extend_from_slice(payload);
        output
    }

    fn data_packet(payload: &[u8]) -> Vec<u8> {
        let mut output = ((payload.len() + 10) as u32).to_be_bytes().to_vec();
        output.extend_from_slice(&[
            constants::PACKET_TYPE_DATA,
            0,
            0,
            0,
            0,
            0,
        ]);
        output.extend_from_slice(payload);
        output
    }

    fn marker_packet(marker_type: u8) -> Vec<u8> {
        raw_packet(constants::PACKET_TYPE_MARKER, &[1, 0, marker_type])
    }

    fn native_client(max_packet_size: usize) -> (TcpStream, Client) {
        let (server, transport) =
            Transport::native_pair_for_test(max_packet_size, true);
        let mut client = Client::with_ttc_field_version(0);
        client.transport = transport;
        (server, client)
    }

    /// The core of the fix: a server signals a mid-session statement failure
    /// with an unprotected break marker, then an unprotected reset-ack marker,
    /// then the encrypted record carrying the actual ORA error. `reset()`
    /// admits exactly the two unprotected marker packets and returns the
    /// error record decrypted, the same as any other protected data.
    #[test]
    fn reset_reads_the_encrypted_error_record_after_a_native_break_marker() {
        let (mut server, mut client) = native_client(256);
        let mut peer = nne::protection_for_test_peer();
        let sender = std::thread::spawn(move || {
            // Drain whatever the client wrote (its own reset marker) without
            // depending on its exact bytes.
            let mut discard = [0u8; 256];
            let _ = server.read(&mut discard);
            server
                .write_all(&marker_packet(constants::MARKER_TYPE_RESET))
                .unwrap();
            // The real server also re-derives its MAC keystreams the moment it
            // answers with its reset marker - see `rekey_for_reset`.
            peer.rekey_for_reset().unwrap();
            let sealed =
                peer.seal(b"ORA-00942: table or view does not exist").unwrap();
            server.write_all(&data_packet(&sealed)).unwrap();
        });
        let packet = client.reset(true).expect(
            "a protected record after the reset marker must be admitted",
        );
        sender.join().unwrap();
        assert_eq!(packet.packet_type, constants::PACKET_TYPE_DATA);
        assert_eq!(packet.buf, b"ORA-00942: table or view does not exist");
    }

    /// Reproduces the entry path flagged in review: the marker packet that led
    /// `receive_data_packet` to call `reset` was itself the reset marker (no
    /// separate break marker preceded it), so `reset_received` starts `true`.
    /// `reset` now rekeys unconditionally right after sending its own reset
    /// marker - not only once it later recognises a reset-typed echo -
    /// matching go-ora's `processMarker`, which re-initialises as soon as it
    /// answers rather than waiting to read anything back; the mandatory
    /// discard slot right after that rekey still consumes one packet, as
    /// go-ora's own order requires (here, the peer's own reset-marker echo),
    /// before the real answer is read.
    #[test]
    fn reset_entered_with_reset_already_received_still_rekeys() {
        let (mut server, mut client) = native_client(256);
        let mut peer = nne::protection_for_test_peer();
        let sender = std::thread::spawn(move || {
            let mut discard = [0u8; 256];
            let _ = server.read(&mut discard);
            server
                .write_all(&marker_packet(constants::MARKER_TYPE_RESET))
                .unwrap();
            peer.rekey_for_reset().unwrap();
            let sealed = peer.seal(b"ORA-01456: may not perform insert/delete/update operation inside a READ ONLY transaction").unwrap();
            server.write_all(&data_packet(&sealed)).unwrap();
        });
        let packet = client
            .reset(true)
            .expect("the DATA packet must authenticate under the keys rekeyed on entry");
        sender.join().unwrap();
        assert_eq!(packet.packet_type, constants::PACKET_TYPE_DATA);
        assert_eq!(
            packet.buf,
            b"ORA-01456: may not perform insert/delete/update operation inside a READ ONLY transaction"
        );
    }

    /// A non-reset marker taking the place of the awaited protected record
    /// must never be forwarded to the caller as if it were that record - the
    /// bytes are legitimately unprotected (markers always are), so without
    /// this check they would reach `ReadBuffer::from_packets` as if they were
    /// the server's authenticated reply.
    #[test]
    fn a_non_reset_marker_after_the_reset_marker_is_refused_not_forwarded() {
        let (mut server, mut client) = native_client(256);
        let sender = std::thread::spawn(move || {
            let mut discard = [0u8; 256];
            let _ = server.read(&mut discard);
            server
                .write_all(&marker_packet(constants::MARKER_TYPE_RESET))
                .unwrap();
            server
                .write_all(&marker_packet(constants::MARKER_TYPE_INTERRUPT))
                .unwrap();
        });
        let error = match client.reset(true) {
            Ok(_) => panic!(
                "a non-reset marker must not be forwarded as the reset result"
            ),
            Err(err) => err,
        };
        sender.join().unwrap();
        assert_eq!(
            error.to_string(),
            "native encryption refused: expected a protected DATA after the reset marker, got an unprotected MARKER"
        );
    }

    /// A protected `DATA` packet arriving before the reset marker is genuine
    /// stale traffic from before the error - on a native transport it must be
    /// refused rather than silently discarded, since silent discard is how a
    /// forged or replayed record would otherwise be dropped without being
    /// noticed. `reset` rekeys and then reads exactly one more packet right
    /// after sending its own answering marker (mirroring go-ora's
    /// `processMarker`, which discards whatever comes next unconditionally,
    /// without inspecting it) - but unlike go-ora, which has no such
    /// pre-existing session traffic to worry about, this transport never
    /// lets that discard-slot double as trusting a protected record as data:
    /// the peer here rekeys too (so the record authenticates structurally,
    /// the same shape the real post-break answer would have) to prove the
    /// refusal below is reached by the type check, not by a decrypt failure.
    #[test]
    fn a_data_packet_before_the_reset_marker_is_refused_not_discarded() {
        let (mut server, mut client) = native_client(256);
        let mut peer = nne::protection_for_test_peer();
        let sender = std::thread::spawn(move || {
            let mut discard = [0u8; 256];
            let _ = server.read(&mut discard);
            peer.rekey_for_reset().unwrap();
            let sealed = peer.seal(b"stale reply").unwrap();
            server.write_all(&data_packet(&sealed)).unwrap();
        });
        let error = match client.reset(false) {
            Ok(_) => panic!(
                "a data packet before the reset marker must not be silently discarded"
            ),
            Err(err) => err,
        };
        sender.join().unwrap();
        assert_eq!(
            error.to_string(),
            "native encryption refused: expected an unprotected MARKER before the reset marker, got a protected DATA"
        );
    }

    /// Entered with the reset marker already seen, the slot after the rekey
    /// still accepts only the peer's echo: an authenticated `DATA` record there
    /// is refused, not dropped while the next packet is returned in its place.
    #[test]
    fn an_authenticated_data_packet_in_the_echo_slot_is_refused_not_dropped() {
        let (mut server, mut client) = native_client(256);
        let mut peer = nne::protection_for_test_peer();
        let sender = std::thread::spawn(move || {
            let mut discard = [0u8; 256];
            let _ = server.read(&mut discard);
            peer.rekey_for_reset().unwrap();
            for payload in [b"first reply".as_slice(), b"second reply"] {
                let sealed = peer.seal(payload).unwrap();
                server.write_all(&data_packet(&sealed)).unwrap();
            }
        });
        let error = match client.reset(true) {
            Ok(_) => panic!(
                "an authenticated record in the echo slot must not be dropped"
            ),
            Err(err) => err,
        };
        sender.join().unwrap();
        assert_eq!(
            error.to_string(),
            "native encryption refused: expected the peer's reset-marker echo, got a protected DATA"
        );
    }

    /// The rekey must not weaken integrity: a record forged after a
    /// server-side rekey (right bytes, wrong MAC) is still refused rather
    /// than authenticating just because a rekey happened.
    #[test]
    fn a_forged_record_after_a_server_side_rekey_is_still_refused() {
        let (mut server, mut client) = native_client(256);
        let mut peer = nne::protection_for_test_peer();
        let sender = std::thread::spawn(move || {
            let mut discard = [0u8; 256];
            let _ = server.read(&mut discard);
            server
                .write_all(&marker_packet(constants::MARKER_TYPE_RESET))
                .unwrap();
            peer.rekey_for_reset().unwrap();
            let mut sealed =
                peer.seal(b"ORA-00942: table or view does not exist").unwrap();
            // Flip a ciphertext byte (not the trailing fold-marker/padding-length
            // bytes, which are refused before the MAC check even runs) so this
            // is refused for failing to authenticate, not for a malformed frame.
            sealed[0] ^= 0xff;
            server.write_all(&data_packet(&sealed)).unwrap();
        });
        let error = match client.reset(true) {
            Ok(_) => panic!(
                "a forged record must not authenticate just because a rekey happened"
            ),
            Err(err) => err,
        };
        sender.join().unwrap();
        assert!(
            error.to_string().contains("did not authenticate"),
            "unexpected error: {error}"
        );
    }
}
