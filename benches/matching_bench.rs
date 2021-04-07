use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use order_matching_engine::engine::MatchingEngine;
use order_matching_engine::order::Order;
use order_matching_engine::types::*;
use rand::prelude::*;

fn bench_simple_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_match");

    group.bench_function("match_100_orders", |b| {
        b.iter(|| {
            let mut engine = MatchingEngine::new();

            // Add resting liquidity
            for i in 0..50 {
                let sell = Order::new_limit(i, Side::Sell, Price::new(100 + (i % 10)), 100).unwrap();
                let _ = engine.process_order(sell);
                let buy = Order::new_limit(
                    1000 + i,
                    Side::Buy,
                    Price::new(99 - (i % 10)),
                    100,
                )
                .unwrap();
                let _ = engine.process_order(buy);
            }

            // Aggressive orders that match
            for i in 0..100 {
                let order = if i % 2 == 0 {
                    Order::new_limit(2000 + i, Side::Buy, Price::new(100), 10).unwrap()
                } else {
                    Order::new_limit(3000 + i, Side::Sell, Price::new(99), 10).unwrap()
                };
                let _ = engine.process_order(order);
            }
        })
    });

    group.finish();
}

fn bench_random_order_flow(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_order_flow");
    let mut rng = StdRng::seed_from_u64(42);

    for &num_orders in &[100, 500, 1000] {
        group.bench_with_input(
            BenchmarkId::from_parameter(num_orders),
            &num_orders,
            |b, &n| {
                // Pre-generate orders for consistent benchmarking
                let orders: Vec<Order> = (0..n)
                    .map(|i| {
                        let side = if rng.gen_bool(0.5) {
                            Side::Buy
                        } else {
                            Side::Sell
                        };
                        let price = Price::new(rng.gen_range(90, 110));
                        let qty = rng.gen_range(1, 100);
                        Order::new_limit(i as u64, side, price, qty).unwrap()
                    })
                    .collect();

                b.iter(|| {
                    let mut engine = MatchingEngine::new();
                    for order in orders.iter().cloned() {
                        let _ = engine.process_order(order);
                    }
                })
            },
        );
    }

    group.finish();
}

fn bench_cancel_orders(c: &mut Criterion) {
    let mut group = c.benchmark_group("cancel_orders");

    group.bench_function("cancel_100_orders", |b| {
        b.iter(|| {
            let mut engine = MatchingEngine::new();

            // Add 100 orders
            for i in 0..100 {
                let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
                let order =
                    Order::new_limit(i, side, Price::new(100 + (i % 20) as i64), 100).unwrap();
                let _ = engine.process_order(order);
            }

            // Cancel all of them
            for i in 0..100 {
                let _ = engine.cancel_order(i);
            }
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_simple_match,
    bench_random_order_flow,
    bench_cancel_orders
);
criterion_main!(benches);
