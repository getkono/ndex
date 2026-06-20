//! Performance-regression benches (advisory, non-blocking — PRD §18.1).
//!
//! Seeded with the real Phase-3 classification helpers; extend with walk/diff/extract/embed/search
//! micro-benchmarks over the fixture corpus as those paths are implemented.

use std::hint::black_box;
use std::io;

use criterion::{Criterion, criterion_group, criterion_main};
use ndex_reconcile::classify_io_error;

fn bench_classify(c: &mut Criterion) {
    let not_found = io::Error::from(io::ErrorKind::NotFound);
    c.bench_function("classify_io_error", |b| {
        b.iter(|| classify_io_error(black_box(&not_found)));
    });
}

criterion_group!(benches, bench_classify);
criterion_main!(benches);
