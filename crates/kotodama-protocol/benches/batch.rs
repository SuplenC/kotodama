//! Encodes and decodes a batch of 10k Indexed items in each Wire format: about
//! the size of an apps Extension's first message on a busy desktop.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use kotodama_protocol::{Action, FromExtension, Item, ItemKind, WireFormat};

const ITEMS: usize = 10_000;

fn batch() -> FromExtension {
    let items = (0..ITEMS)
        .map(|n| Item {
            id: format!("org.example.App{n}.desktop"),
            kind: ItemKind::App {
                desktop_id: format!("org.example.App{n}.desktop"),
            },
            title: format!("Example App {n}"),
            subtitle: Some("A typical application with a one-line description".into()),
            icon: Some(format!("org.example.App{n}")),
            keyword: None,
            identity: None,
            keywords: vec!["example".into(), "utility".into(), format!("app{n}")],
            hidden: false,
            actions: vec![
                Action {
                    id: "open".into(),
                    title: "Open".into(),
                    shortcut: None,
                    arguments: vec![],
                },
                Action {
                    id: "new-window".into(),
                    title: "Open a new window".into(),
                    shortcut: Some("ctrl+n".into()),
                    arguments: vec![],
                },
            ],
        })
        .collect();
    FromExtension::AddItems { items }
}

fn bench(c: &mut Criterion) {
    let message = batch();
    let mut group = c.benchmark_group("10k_items");
    group.throughput(Throughput::Elements(ITEMS as u64));
    for format in WireFormat::ALL {
        let name = format!("{format:?}").to_lowercase();
        let mut buf = Vec::new();
        group.bench_function(BenchmarkId::new("encode", &name), |b| {
            b.iter(|| {
                buf.clear();
                format.encode_into(black_box(&message), &mut buf).unwrap();
            })
        });
        let bytes = format.encode(&message).unwrap();
        group.bench_function(BenchmarkId::new("decode", &name), |b| {
            b.iter(|| format.decode::<FromExtension>(black_box(&bytes)).unwrap())
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
