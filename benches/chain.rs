use criterion::{Criterion, black_box, criterion_group, criterion_main};
use dagq::{Dag, DagBuilder, Expr, Vx};

fn chain_graph(size: usize) -> (Dag, Vec<Vx>) {
    assert!(size > 0, "chain graph must have at least one vertex");

    let mut builder = DagBuilder::default();
    let mut vxs = Vec::with_capacity(size);
    let mut prev = builder.root();
    vxs.push(prev);

    for _ in 1..size {
        let next = builder.m([prev]);
        vxs.push(next);
        prev = next;
    }

    (builder.build(), vxs)
}

fn bench_unbounded_up_chain(c: &mut Criterion) {
    let (dag, vxs) = chain_graph(10_000);

    c.bench_function("unbounded up over 10k chain", |b| {
        let mut evaluator = dag.evaluator();

        b.iter(|| {
            let query = Expr::constant(vec![dag.root()]).up(1, None);
            let result = evaluator.eval(black_box(query));
            black_box(result.len())
        })
    });

    let mut evaluator = dag.evaluator();
    assert_eq!(
        evaluator
            .eval(Expr::constant(vec![dag.root()]).up(1, None))
            .len(),
        vxs.len() - 1
    );
}

criterion_group!(benches, bench_unbounded_up_chain);
criterion_main!(benches);
