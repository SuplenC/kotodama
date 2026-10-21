//! Encodes and decodes the Daemon's real messages (a query, 50 Results, a
//! batch of 1,000 Items) in every Wire format, and in the binary formats that
//! were weighed against postcard: prost (protobuf with derive), müsli's `wire`
//! format and bilrost, on copies of the types their derives want
//! (`mirror.rs`). Postcard is kept between Rust peers unless one of those
//! decodes faster.

use std::hint::black_box;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use kotodama_protocol::FromExtension;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::ToFrontEnd;
use kotodama_protocol::WireFormat;
use serde::Serialize;
use serde::de::DeserializeOwned;

#[path = "../tests/support/messages.rs"]
mod messages;
#[path = "../tests/support/mirror.rs"]
mod mirror;

fn wire<T: Serialize + DeserializeOwned>(c: &mut Criterion, group: &str, message: &T) {
    let mut group = c.benchmark_group(group);
    for format in WireFormat::ALL {
        let name = format!("{format:?}").to_lowercase();
        let mut buf = Vec::new();
        group.bench_function(BenchmarkId::new("encode", &name), |b| {
            b.iter(|| {
                buf.clear();
                format.encode_into(black_box(message), &mut buf).unwrap();
            });
        });
        let bytes = format.encode(message).unwrap();
        group.bench_function(BenchmarkId::new("decode", &name), |b| {
            b.iter(|| format.decode::<T>(black_box(&bytes)).unwrap());
        });
    }
    group.finish();
}

fn prost<T: prost::Message + Default>(c: &mut Criterion, group: &str, message: &T) {
    let mut group = c.benchmark_group(group);
    let mut buf = Vec::new();
    group.bench_function(BenchmarkId::new("encode", "prost"), |b| {
        b.iter(|| {
            buf.clear();
            black_box(message).encode(&mut buf).unwrap();
        });
    });
    let bytes = message.encode_to_vec();
    group.bench_function(BenchmarkId::new("decode", "prost"), |b| {
        b.iter(|| T::decode(black_box(bytes.as_slice())).unwrap());
    });
    group.finish();
}

fn musli<T>(c: &mut Criterion, group: &str, message: &T)
where
    T: musli::Encode<musli::mode::Binary> + for<'de> musli::Decode<'de, musli::mode::Binary, musli::alloc::Global>,
{
    let mut group = c.benchmark_group(group);
    let mut buf = Vec::new();
    group.bench_function(BenchmarkId::new("encode", "musli-wire"), |b| {
        b.iter(|| {
            buf.clear();
            musli::wire::to_writer(&mut buf, black_box(message)).unwrap();
        });
    });
    let bytes = musli::wire::to_vec(message).unwrap();
    group.bench_function(BenchmarkId::new("decode", "musli-wire"), |b| {
        b.iter(|| musli::wire::from_slice::<T>(black_box(&bytes)).unwrap());
    });
    group.finish();
}

/// Postcard and the protobuf Wire format on the müsli copies, so that they
/// decode the same types as müsli and prost: the protocol's own types check
/// each Item as they decode it.
fn serde_on_copies<T: Serialize + DeserializeOwned>(c: &mut Criterion, group: &str, message: &T) {
    let mut group = c.benchmark_group(group);
    for format in [WireFormat::Postcard, WireFormat::Protobuf] {
        let name = format!("{format:?}-copies").to_lowercase();
        let mut buf = Vec::new();
        group.bench_function(BenchmarkId::new("encode", &name), |b| {
            b.iter(|| {
                buf.clear();
                format.encode_into(black_box(message), &mut buf).unwrap();
            });
        });
        let bytes = format.encode(message).unwrap();
        group.bench_function(BenchmarkId::new("decode", &name), |b| {
            b.iter(|| format.decode::<T>(black_box(&bytes)).unwrap());
        });
    }
    group.finish();
}

fn bilrost<T: bilrost::Message + bilrost::OwnedMessage>(c: &mut Criterion, group: &str, message: &T) {
    let mut group = c.benchmark_group(group);
    let mut buf = Vec::new();
    group.bench_function(BenchmarkId::new("encode", "bilrost"), |b| {
        b.iter(|| {
            buf.clear();
            black_box(message).encode(&mut buf).unwrap();
        });
    });
    let bytes = message.encode_to_vec();
    group.bench_function(BenchmarkId::new("decode", "bilrost"), |b| {
        b.iter(|| T::decode(black_box(bytes.as_slice())).unwrap());
    });
    group.finish();
}

fn query(c: &mut Criterion) {
    let message: FromFrontEnd = messages::query();
    wire(c, "query", &message);
    prost(c, "query", &mirror::prost::query(&message));
    musli(c, "query", &mirror::musli::query(&message));
    serde_on_copies(c, "query", &mirror::musli::query(&message));
    bilrost(c, "query", &mirror::bilrost::query(&message));
}

fn results(c: &mut Criterion) {
    let message: ToFrontEnd = messages::results();
    wire(c, "50_results", &message);
    prost(c, "50_results", &mirror::prost::results(&message));
    musli(c, "50_results", &mirror::musli::results(&message));
    serde_on_copies(c, "50_results", &mirror::musli::results(&message));
    bilrost(c, "50_results", &mirror::bilrost::results(&message));
}

fn batch(c: &mut Criterion) {
    let message: FromExtension = messages::batch();
    wire(c, "1k_items", &message);
    prost(c, "1k_items", &mirror::prost::batch(&message));
    musli(c, "1k_items", &mirror::musli::batch(&message));
    serde_on_copies(c, "1k_items", &mirror::musli::batch(&message));
    bilrost(c, "1k_items", &mirror::bilrost::batch(&message));
}

criterion_group!(benches, query, results, batch);
criterion_main!(benches);
