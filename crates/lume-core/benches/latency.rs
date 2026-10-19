use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lume_core::{Editor, RopeBuffer, TextBuffer};

fn create_100k_lines_text() -> String {
    let mut s = String::with_capacity(100_000 * 35);
    for i in 0..100_000 {
        use std::fmt::Write;
        let _ = writeln!(s, "fn test_function_{}() {{ return {}; }}", i, i * 2);
    }
    s
}

fn bench_buffer_insertion_100k(c: &mut Criterion) {
    let text = create_100k_lines_text();
    let base_buffer = RopeBuffer::from_str(&text);

    let mut group = c.benchmark_group("buffer_100k_latency");

    // Middle byte index
    let mid_byte = base_buffer.line_to_byte(50_000);
    let end_byte = base_buffer.len_bytes();

    group.bench_function("insert_char_middle", |b| {
        b.iter_batched(
            || base_buffer.clone(),
            |mut buf| {
                let _ = buf.insert(mid_byte, black_box("x"));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("insert_char_end", |b| {
        b.iter_batched(
            || base_buffer.clone(),
            |mut buf| {
                let _ = buf.insert(end_byte, black_box("x"));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("insert_char_beginning", |b| {
        b.iter_batched(
            || base_buffer.clone(),
            |mut buf| {
                let _ = buf.insert(0, black_box("x"));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("delete_char_middle", |b| {
        b.iter_batched(
            || base_buffer.clone(),
            |mut buf| {
                let _ = buf.delete(mid_byte, mid_byte + 1);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_startup_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("startup_latency");

    group.bench_function("editor_new_empty", |b| {
        b.iter(|| {
            let ed = Editor::new();
            black_box(ed)
        });
    });

    let small_file = "fn main() {\n    println!(\"Hello, world!\");\n}\n";
    group.bench_function("editor_from_str_small", |b| {
        b.iter(|| {
            let ed = Editor::from_str(black_box(small_file));
            black_box(ed)
        });
    });

    group.finish();
}

fn bench_indexing_latency_100k(c: &mut Criterion) {
    let text = create_100k_lines_text();
    let buffer = RopeBuffer::from_str(&text);

    let mut group = c.benchmark_group("indexing_100k_latency");

    group.bench_function("line_to_byte_middle", |b| {
        b.iter(|| black_box(buffer.line_to_byte(black_box(50_000))));
    });

    group.bench_function("byte_to_line_middle", |b| {
        let byte = buffer.line_to_byte(50_000);
        b.iter(|| black_box(buffer.byte_to_line(black_box(byte))));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_buffer_insertion_100k,
    bench_startup_latency,
    bench_indexing_latency_100k
);
criterion_main!(benches);
