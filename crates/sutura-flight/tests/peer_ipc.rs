#![forbid(unsafe_code)]

//! A malicious peer controls every byte of [`arrow_flight::FlightData::data_header`] and
//! `data_body`. These cells drive [`arrow_flight::decode::FlightRecordBatchStream`] directly
//! (not [`sutura_flight::receive_request`], which redacts every decode error into
//! [`sutura_flight::ReceiveError::Parameters`] and so cannot show which site a payload hits).
//!
//! Limit: each cell's registry 59.2.0 outcome is traced from source, not run here; CI's run is
//! the proof.

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch};
use arrow_flight::{FlightData, decode::FlightRecordBatchStream, error::FlightError};
use arrow_ipc::{
    BodyCompression, BodyCompressionArgs, BodyCompressionMethod, Buffer, CompressionType, Decimal, DecimalArgs, Endianness,
    Field, FieldArgs, FieldNode, FixedSizeBinary, FixedSizeBinaryArgs, Message, MessageArgs, MessageHeader, MetadataVersion,
    Null, NullArgs, RecordBatch as IpcRecordBatch, RecordBatchArgs, Schema, SchemaArgs, Type, Union, UnionArgs,
};
use flatbuffers::FlatBufferBuilder;
use futures_util::{TryStreamExt as _, stream};
use sutura_domain::plan::LegWire;
use sutura_flight::request_stream;

#[cfg(test)]
mod tests {
    use super::*;

    fn leg() -> LegWire {
        serde_json::from_value(serde_json::json!({
            "leg": "lookup",
            "source": "sales",
            "metric": {"metric": "revenue", "digest": "0000000000000000000000000000000000000000000000000000000000000000"},
            "table": "orders",
            "joins": [],
            "keys": [],
            "filters": []
        }))
        .expect("the fixture is a typed leg")
    }

    fn batch() -> RecordBatch {
        RecordBatch::try_from_iter([("value", Arc::new(Int64Array::from(vec![7])) as ArrayRef)])
            .expect("the parameter batch is valid")
    }

    fn run<F, T>(fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a test runtime")
            .block_on(fut)
    }

    /// A legitimate first frame (descriptor + schema), reused unmodified ahead of a corrupted
    /// `RecordBatch` frame for the recordbatch-route cells (D, F-lz4, F-zstd).
    fn legitimate_schema_frame() -> FlightData {
        run(async {
            let encoded = request_stream(&leg(), stream::iter(vec![Ok(batch())])).expect("the leg is encodable");
            let frames: Vec<FlightData> = encoded.try_collect().await.expect("encoding succeeds");
            frames.into_iter().next().expect("a schema frame was produced")
        })
    }

    /// Wraps a hand-built flatbuffer `Message` body in a [`FlightData`] frame, mimicking the shape
    /// [`arrow_flight::encode::FlightDataEncoder`] produces: a non-size-prefixed `data_header` and a
    /// raw `data_body`.
    fn frame(data_header: Vec<u8>, data_body: Vec<u8>) -> FlightData {
        FlightData {
            flight_descriptor: None,
            data_header: data_header.into(),
            app_metadata: Vec::new().into(),
            data_body: data_body.into(),
        }
    }

    fn finish_schema_message<'a>(fbb: &mut FlatBufferBuilder<'a>, field: flatbuffers::WIPOffset<Field<'a>>) -> FlightData {
        let fields = fbb.create_vector(&[field]);
        let schema = Schema::create(
            fbb,
            &SchemaArgs {
                endianness: Endianness::Little,
                fields: Some(fields),
                custom_metadata: None,
                features: None,
            },
        );
        let message = Message::create(
            fbb,
            &MessageArgs {
                version: MetadataVersion::V5,
                header_type: MessageHeader::Schema,
                header: Some(schema.as_union_value()),
                bodyLength: 0,
                custom_metadata: None,
            },
        );
        fbb.finish(message, None);
        frame(fbb.finished_data().to_vec(), Vec::new())
    }

    async fn decode_all(frames: Vec<FlightData>) -> Result<Vec<RecordBatch>, FlightError> {
        FlightRecordBatchStream::new_from_flight_data(stream::iter(frames.into_iter().map(Ok)))
            .try_collect()
            .await
    }

    /// Cell A: arrow-rs#10647's convert.rs-only part makes `get_data_type`'s `Decimal` arm fallible,
    /// turning the `panic!` into `Err(ArrowError::ParseError(...))` with the same message text.
    ///
    /// Corrupted field: `Decimal.bitWidth = 999` (only 32/64/128/256 are valid).
    /// Registry 59.2.0 panic: `arrow-ipc-59.2.0/src/convert.rs`, `get_data_type`'s `Type::Decimal`
    /// arm, `panic!("Unexpected decimal bit width {bit_width}")`. Upstream #10647 keeps this exact
    /// wording (`ParseError("Unexpected decimal bit width {bit_width}")`), so this cell's fragment
    /// is the panic's own text, not a sutura-chosen rewording - the base-red signal here is the
    /// panic itself, not a text mismatch.
    #[test]
    fn a_peer_decimal_bit_width_is_refused_not_panicked() {
        let mut fbb = FlatBufferBuilder::new();
        let name = fbb.create_string("poison");
        let decimal = Decimal::create(
            &mut fbb,
            &DecimalArgs {
                precision: 10,
                scale: 2,
                bitWidth: 999,
            },
        );
        let field = Field::create(
            &mut fbb,
            &FieldArgs {
                name: Some(name),
                nullable: true,
                type_type: Type::Decimal,
                type_: Some(decimal.as_union_value()),
                dictionary: None,
                children: None,
                custom_metadata: None,
            },
        );
        let bad_schema = finish_schema_message(&mut fbb, field);

        let error = run(decode_all(vec![bad_schema])).expect_err("an invalid decimal bit width is refused, not panicked");
        assert!(
            matches!(&error, FlightError::DecodeError(message) if message.contains("Unexpected decimal bit width 999")),
            "{error}"
        );
    }

    /// Cell B: arrow-rs#11882's `UnionFields::try_from_fields` replaces the `UnionFields::from_fields`
    /// call that panics past 128 children when a `Union` carries no explicit `typeIds`.
    ///
    /// Corrupted field: a `Union` with default `UnionArgs` (sparse, no `typeIds`) and 129
    /// `Type::Null` children, mirroring arrow-rs's own upstream #11882 regression test.
    /// Registry 59.2.0 panic: `arrow-schema-59.2.0/src/fields.rs:476`,
    /// `UnionFields::from_fields`'s
    /// `i8::try_from(i).expect("UnionFields cannot contain more than 128 fields")`.
    /// Fixed behaviour's error text comes from `arrow-schema-59.2.0/src/fields.rs:527`,
    /// `UnionFields::try_from_fields`'s own `Err` arm, which #11882's call site in
    /// `arrow-ipc`'s `get_data_type` must propagate with `?` rather than swallow - a kill mutation
    /// that replaces that `?` with
    /// `.unwrap_or_else(|_| UnionFields::empty())` keeps this cell's assertion from ever firing
    /// without changing any text, so this cell's kill is that swallow, not a wording edit.
    #[test]
    fn a_peer_union_of_129_children_is_refused_not_panicked() {
        let mut fbb = FlatBufferBuilder::new();
        let children: Vec<_> = (0..129)
            .map(|i| {
                let child_name = fbb.create_string(&format!("c{i}"));
                let null_type = Null::create(&mut fbb, &NullArgs::default());
                Field::create(
                    &mut fbb,
                    &FieldArgs {
                        name: Some(child_name),
                        nullable: true,
                        type_type: Type::Null,
                        type_: Some(null_type.as_union_value()),
                        dictionary: None,
                        children: None,
                        custom_metadata: None,
                    },
                )
            })
            .collect();
        let children = fbb.create_vector(&children);
        let union_ = Union::create(&mut fbb, &UnionArgs::default());
        let name = fbb.create_string("poison");
        let field = Field::create(
            &mut fbb,
            &FieldArgs {
                name: Some(name),
                nullable: true,
                type_type: Type::Union,
                type_: Some(union_.as_union_value()),
                dictionary: None,
                children: Some(children),
                custom_metadata: None,
            },
        );
        let bad_schema = finish_schema_message(&mut fbb, field);

        let error = run(decode_all(vec![bad_schema])).expect_err("a 129-child union is refused, not panicked");
        assert!(
            matches!(&error, FlightError::DecodeError(message) if message.contains("cannot contain more than 128 fields")),
            "{error}"
        );
    }

    /// Cell C: arrow-rs#11883, reject a negative `FixedSizeBinary` byte width in `get_data_type`
    /// itself (depends on fix A's fallible refactor) rather than let it reach array construction in
    /// `arrow-data`/`arrow-array` (crates this fix cannot touch, since they are not vendored).
    ///
    /// Corrupted field: `FixedSizeBinary.byteWidth = -1`.
    /// Registry 59.2.0 behaviour: `arrow-ipc-59.2.0/src/convert.rs:346`,
    /// `DataType::FixedSizeBinary(fsb.byteWidth())` accepts the negative width unchecked; the
    /// schema parses with `Ok` today, so this cell is red via its own `expect_err`, not a caught
    /// panic.
    #[test]
    fn a_peer_negative_fixed_size_binary_width_is_refused() {
        let mut fbb = FlatBufferBuilder::new();
        let name = fbb.create_string("poison");
        let fsb = FixedSizeBinary::create(&mut fbb, &FixedSizeBinaryArgs { byteWidth: -1 });
        let field = Field::create(
            &mut fbb,
            &FieldArgs {
                name: Some(name),
                nullable: true,
                type_type: Type::FixedSizeBinary,
                type_: Some(fsb.as_union_value()),
                dictionary: None,
                children: None,
                custom_metadata: None,
            },
        );
        let bad_schema = finish_schema_message(&mut fbb, field);

        let error = run(decode_all(vec![bad_schema])).expect_err("a negative byte width is refused once fixed");
        assert!(
            matches!(&error, FlightError::DecodeError(message) if message.contains("byte width must be non-negative")),
            "{error}"
        );
    }

    /// Builds a corrupted `RecordBatch` message frame for a single `Int64` column, with
    /// attacker-controlled buffer metadata and body bytes.
    fn record_batch_frame(
        node_len: i64,
        buffer_offset: i64,
        buffer_len: i64,
        data_body: Vec<u8>,
        compression: Option<(CompressionType, BodyCompressionMethod)>,
    ) -> FlightData {
        let mut fbb = FlatBufferBuilder::new();
        let nodes = fbb.create_vector(&[FieldNode::new(node_len, 0)]);
        let buffers = fbb.create_vector(&[Buffer::new(0, 0), Buffer::new(buffer_offset, buffer_len)]);
        let compression_offset =
            compression.map(|(codec, method)| BodyCompression::create(&mut fbb, &BodyCompressionArgs { codec, method }));
        let mut record_batch_args = RecordBatchArgs {
            length: node_len,
            nodes: Some(nodes),
            buffers: Some(buffers),
            compression: None,
            variadicBufferCounts: None,
        };
        record_batch_args.compression = compression_offset;
        let record_batch = IpcRecordBatch::create(&mut fbb, &record_batch_args);
        let body_length = i64::try_from(data_body.len()).expect("a test body fits in i64");
        let message = Message::create(
            &mut fbb,
            &MessageArgs {
                version: MetadataVersion::V5,
                header_type: MessageHeader::RecordBatch,
                header: Some(record_batch.as_union_value()),
                bodyLength: body_length,
                custom_metadata: None,
            },
        );
        fbb.finish(message, None);
        frame(fbb.finished_data().to_vec(), data_body)
    }

    /// Cell D: #11881's inlined bounds check on a `Buffer`'s `offset`/`length` against the actual
    /// `data_body` length, equivalent to a `try_slice_with_length`.
    ///
    /// Corrupted field: the data buffer claims `offset = 0, length = 999_999` against a `data_body`
    /// of 8 bytes.
    /// Registry 59.2.0 panic: `arrow-ipc-59.2.0/src/reader.rs:66`'s `slice_with_length` call
    /// reaches `arrow-buffer-59.2.0/src/buffer/immutable.rs:287-292`'s
    /// `assert!(offset + length <= self.len(), "the offset of the new Buffer cannot exceed the
    /// existing length: ...")`.
    #[test]
    fn a_peer_oversized_buffer_length_is_refused_not_panicked() {
        let schema_frame = legitimate_schema_frame();
        let tiny_body = vec![0u8; 8];
        let bad_batch = record_batch_frame(1, 0, 999_999, tiny_body, None);

        let error = run(decode_all(vec![schema_frame, bad_batch])).expect_err("an oversized buffer length is refused");
        assert!(
            matches!(&error, FlightError::DecodeError(message) if message.contains("buffer length exceeds the record batch body")),
            "{error}"
        );
    }

    /// Shared payload for cells F-lz4 and F-zstd: an 8-byte little-endian claimed decompressed
    /// length of `i64::MAX`, followed by 8 bytes that are not valid compressed data for either
    /// codec (irrelevant, since the unguarded `Vec::with_capacity(claimed_len)` aborts before any
    /// byte of it is read). `i64::MAX` is used instead of a smaller "generous" bound such as 10 GiB
    /// because a smaller claim can be satisfied lazily by overcommit on some allocators, in which
    /// case the subsequent decompression attempt fails gracefully instead of aborting the process -
    /// defeating the point of this cell.
    #[expect(
        clippy::little_endian_bytes,
        reason = "Arrow IPC's wire format mandates little-endian explicitly"
    )]
    fn compression_bomb_body() -> Vec<u8> {
        let claimed_len = i64::MAX;
        let mut body = claimed_len.to_le_bytes().to_vec();
        body.extend_from_slice(&[0u8; 8]);
        body
    }

    /// Cell F-lz4 (sutura-original): bound the claimed decompressed length against the actual
    /// compressed input length before allocating.
    ///
    /// Corrupted field: an LZ4-compressed buffer whose embedded length prefix claims `i64::MAX`
    /// from 8 bytes of compressed input.
    /// Registry 59.2.0 panic: `arrow-ipc-59.2.0/src/compression.rs:313`'s `decompress_lz4` calls
    /// `Vec::with_capacity(decompressed_size)` using the peer-controlled prefix with no upper
    /// bound, aborting the process on allocation failure.
    #[test]
    fn a_peer_lz4_decompression_bomb_is_refused_not_aborted() {
        let schema_frame = legitimate_schema_frame();
        let bad_batch = record_batch_frame(
            1,
            0,
            16,
            compression_bomb_body(),
            Some((CompressionType::LZ4_FRAME, BodyCompressionMethod::BUFFER)),
        );

        let error = run(decode_all(vec![schema_frame, bad_batch])).expect_err("an LZ4 decompression bomb is refused");
        assert!(
            matches!(&error, FlightError::DecodeError(message) if message.contains("decompressed size exceeds the lz4 expansion bound")),
            "{error}"
        );
    }

    /// Cell F-zstd (sutura-original): same bound for the zstd codec, plus not trusting a zstd
    /// frame's own content-size header as a second avenue to the same allocation.
    ///
    /// Corrupted field: a ZSTD-tagged buffer whose embedded length prefix claims `i64::MAX` from 8
    /// bytes of compressed input.
    /// Registry 59.2.0 panic: `arrow-ipc-59.2.0/src/compression.rs`'s `decompress_zstd` delegates
    /// to `zstd-0.13.3/src/bulk/decompressor.rs:107`'s `Vec::with_capacity(capacity)`, using the
    /// peer-controlled prefix with no upper bound, aborting the process on allocation failure.
    #[test]
    fn a_peer_zstd_decompression_bomb_is_refused_not_aborted() {
        let schema_frame = legitimate_schema_frame();
        let bad_batch = record_batch_frame(
            1,
            0,
            16,
            compression_bomb_body(),
            Some((CompressionType::ZSTD, BodyCompressionMethod::BUFFER)),
        );

        let error = run(decode_all(vec![schema_frame, bad_batch])).expect_err("a zstd decompression bomb is refused");
        assert!(
            matches!(&error, FlightError::DecodeError(message) if message.contains("decompressed size exceeds the zstd expansion bound")),
            "{error}"
        );
    }
}
