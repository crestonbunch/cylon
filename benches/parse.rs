use cylon::Compiler;

use criterion::async_executor::FuturesExecutor;
use criterion::{criterion_group, criterion_main, Criterion};

const SMALL_FILE: &[u8] = r#"
User-agent: *
Disallow: /
Allow: /a
Allow: /abc
Allow: /b
Crawl-Delay: 20
"#
.as_bytes();

const LARGE_FILE: &[u8] = r#"
User-agent: *
Allow: /
Disallow: /a$
Disallow: /abc
Allow: /abc/*
Disallow: /foo/bar
Allow /*/bar
Disallow: /www/*/images
Allow: /www/public/images
"#
.as_bytes();

fn bench(c: &mut Criterion) {
    c.bench_function("compile small", |b| {
        b.to_async(FuturesExecutor).iter(|| async {
            let parser = Compiler::new("ImABot");
            parser.compile(SMALL_FILE).await.unwrap();
        })
    });

    c.bench_function("compile large", |b| {
        b.to_async(FuturesExecutor).iter(|| async {
            let parser = Compiler::new("ImABot");
            parser.compile(LARGE_FILE).await.unwrap();
        })
    });

    let parser = Compiler::new("ImABot");
    let small_machine = &tokio_test::block_on(parser.compile(SMALL_FILE)).unwrap();
    c.bench_function("allow small A", move |b| {
        b.iter(|| {
            small_machine.allow("/abc");
        });
    });
    c.bench_function("allow small B", move |b| {
        b.iter(|| {
            small_machine.allow("/www/cat/images");
        });
    });

    let large_machine = &tokio_test::block_on(parser.compile(LARGE_FILE)).unwrap();
    c.bench_function("allow large A", move |b| {
        b.iter(|| {
            large_machine.allow("/abc");
        });
    });
    c.bench_function("allow large B", move |b| {
        b.iter(|| {
            large_machine.allow("/www/cat/images");
        });
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
