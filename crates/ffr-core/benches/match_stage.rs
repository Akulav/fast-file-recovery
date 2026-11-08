//! Match-stage benchmark: 256 MiB of random data, all signatures enabled.
//! Target (release, 8 cores): ≥ 3 GB/s for the whole stage across cores.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use ffr_core::scan::{build_matcher, match_chunk};
use rayon::prelude::*;
use std::collections::BTreeSet;

fn noise(n: usize) -> Vec<u8> {
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    let mut v = vec![0u8; n];
    for c in v.as_chunks_mut::<8>().0 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        c.copy_from_slice(&x.to_le_bytes());
    }
    v
}

fn bench(c: &mut Criterion) {
    const CHUNK: usize = 16 << 20;
    let data = noise(256 << 20);
    let all: BTreeSet<String> = ffr_core::signatures::all()
        .iter()
        .map(|s| s.id.to_string())
        .collect();
    let mut g = c.benchmark_group("match_stage");
    g.throughput(Throughput::Bytes(data.len() as u64));
    g.sample_size(10);
    for (name, aligned) in [("aligned_only", true), ("every_byte", false)] {
        let m = build_matcher(&all, aligned).unwrap();
        g.bench_function(name, |b| {
            b.iter(|| {
                data.par_chunks(CHUNK)
                    .enumerate()
                    .map(|(i, chunk)| {
                        let mut out = Vec::new();
                        match_chunk(&m, (i * CHUNK) as u64, chunk, chunk.len(), &mut out);
                        out.len()
                    })
                    .sum::<usize>()
            })
        });
    }
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
