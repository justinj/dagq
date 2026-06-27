use criterion::{Criterion, black_box, criterion_group, criterion_main};
use dagq::{Dag, DagBuilder, Expr, Vx};

fn parallel_chain_graph(chains: usize, chain_len: usize) -> (Dag, Vec<Vx>) {
    assert!(chains > 0, "must have at least one chain");
    assert!(chain_len > 0, "chains must have at least one vertex");

    let mut builder = DagBuilder::default();
    let root = builder.root();
    let mut vxs = Vec::with_capacity(1 + chains * chain_len);
    vxs.push(root);

    for _ in 0..chains {
        let mut prev = root;
        for _ in 0..chain_len {
            let next = builder.m([prev]);
            vxs.push(next);
            prev = next;
        }
    }

    (builder.build(), vxs)
}

fn bench_unbounded_up_chain(c: &mut Criterion) {
    let chains = 100;
    let chain_len = 1000;
    let (dag, vxs) = parallel_chain_graph(chains, chain_len);

    c.bench_function("unbounded up over 100 parallel 100-node chains", |b| {
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
