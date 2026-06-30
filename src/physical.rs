mod reusable_iter;

use std::{collections::HashSet, marker::PhantomData};

use reusable_iter::ReusableIntoIter;

trait Ordering {
    const SORTED: bool;

    fn cmp<T: Ord>(left: &T, right: &T) -> std::cmp::Ordering;

    fn sort<T: Ord>(data: &mut [T]) {
        if Self::SORTED {
            data.sort_unstable_by(Self::cmp);
        }
    }
}

// Forwards = Child -> Parent
enum Forwards {}

impl Ordering for Forwards {
    const SORTED: bool = true;

    fn cmp<T: Ord>(left: &T, right: &T) -> std::cmp::Ordering {
        left.cmp(right)
    }
}

// Backwards = Parent -> Child
enum Backwards {}

impl Ordering for Backwards {
    const SORTED: bool = true;

    fn cmp<T: Ord>(left: &T, right: &T) -> std::cmp::Ordering {
        right.cmp(left)
    }
}

enum Unordered {}
impl Ordering for Unordered {
    const SORTED: bool = false;

    fn cmp<T: Ord>(_: &T, _: &T) -> std::cmp::Ordering {
        std::cmp::Ordering::Equal
    }
}

struct Batch<T, O: Ordering> {
    data: Vec<T>,
    _marker: PhantomData<O>,
}

impl<T, O: Ordering> Batch<T, O> {
    fn new(data: Vec<T>) -> Self {
        Batch {
            data,
            _marker: PhantomData,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct Rev(u32);

impl PartialOrd for Rev {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rev {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.cmp(&self.0)
    }
}

trait Operator<T, O: Ordering> {
    fn next(&mut self, batch: &mut Batch<T, O>);

    fn iter(&mut self) -> OperatorIter<'_, Self, T, O>
    where
        Self: Sized,
    {
        OperatorIter {
            operator: self,
            batch: Vec::new().into_iter(),
            _marker: PhantomData,
        }
    }
}

struct OperatorIter<'a, Op, T, O: Ordering>
where
    Op: Operator<T, O> + ?Sized,
{
    operator: &'a mut Op,
    batch: std::vec::IntoIter<T>,
    _marker: PhantomData<O>,
}

impl<Op, T, O: Ordering> Iterator for OperatorIter<'_, Op, T, O>
where
    Op: Operator<T, O> + ?Sized,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(item) = self.batch.next() {
                return Some(item);
            }

            let mut batch = Batch::new(Vec::new());
            self.operator.next(&mut batch);
            if batch.data.is_empty() {
                return None;
            }
            self.batch = batch.data.into_iter();
        }
    }
}

struct Constant<T, O: Ordering> {
    data: Vec<T>,
    _marker: PhantomData<O>,
}

impl<T: Ord, O: Ordering> Constant<T, O> {
    fn new(mut data: Vec<T>) -> Self {
        O::sort(&mut data);
        Self {
            data,
            _marker: PhantomData,
        }
    }
}

impl<T, O: Ordering> Operator<T, O> for Constant<T, O> {
    fn next(&mut self, batch: &mut Batch<T, O>) {
        std::mem::swap(&mut self.data, &mut batch.data)
    }
}

struct Index<T, O: Ordering> {
    data: Vec<T>,
    _marker: PhantomData<O>,
}

impl<T: Ord, O: Ordering> Index<T, O> {
    fn new(mut data: Vec<T>) -> Self {
        O::sort(&mut data);
        Self {
            data,
            _marker: PhantomData,
        }
    }

    fn cursor(&self) -> Scan<'_, T, O> {
        Scan {
            index: self,
            i: 0,
            end: None,
        }
    }
}

struct Scan<'a, T, O: Ordering> {
    index: &'a Index<T, O>,
    i: usize,
    end: Option<T>,
}

impl<'a, T: Ord, O: Ordering> Scan<'a, T, O> {
    fn with_start(mut self, start: T) -> Self {
        self.i = match self
            .index
            .data
            .binary_search_by(|item| O::cmp(item, &start))
        {
            Ok(i) => i,
            Err(i) => i,
        };
        self
    }

    fn with_end(mut self, end: T) -> Self {
        self.end = Some(end);
        self
    }
}

const SCAN_BATCH_SIZE: usize = 1024;
const CLOSURE_BATCH_SIZE: usize = 1024;
const SET_BATCH_SIZE: usize = 1024;

impl<'a, T: Ord + Clone, O: Ordering> Operator<T, O> for Scan<'a, T, O> {
    fn next(&mut self, batch: &mut Batch<T, O>) {
        while batch.data.len() < SCAN_BATCH_SIZE && self.i < self.index.data.len() {
            if let Some(end) = &self.end {
                if O::cmp(&self.index.data[self.i], end) == std::cmp::Ordering::Greater {
                    break;
                }
            }

            batch.data.push(self.index.data[self.i].clone());
            self.i += 1;
        }
    }
}

struct Closure<T, U, V, O: Ordering>
where
    U: Operator<T, O>,
    V: Operator<(T, T), O>,
{
    u: Option<U>,
    v: Buffered<(T, T), V, O>,
    set: HashSet<T>,
}

impl<T, U, V, O: Ordering> Closure<T, U, V, O>
where
    U: Operator<T, O>,
    V: Operator<(T, T), O>,
{
    fn new(u: U, v: V) -> Self {
        Self {
            u: Some(u),
            v: Buffered::new(v),
            set: HashSet::new(),
        }
    }
}

impl<T, U, V, O: Ordering> Operator<T, O> for Closure<T, U, V, O>
where
    T: Eq + std::hash::Hash + Clone,
    U: Operator<T, O>,
    V: Operator<(T, T), O>,
{
    fn next(&mut self, batch: &mut Batch<T, O>) {
        if let Some(mut start) = self.u.take() {
            self.set.extend(start.iter());
        }

        while batch.data.len() < CLOSURE_BATCH_SIZE {
            let Some((from, to)) = self.v.next_item() else {
                break;
            };

            if self.set.contains(&from) && self.set.insert(to.clone()) {
                batch.data.push(to);
            }
        }
    }
}

struct Buffered<T, Op, O: Ordering>
where
    Op: Operator<T, O>,
{
    operator: Op,
    batch: ReusableIntoIter<T>,
    _marker: PhantomData<O>,
}

impl<T, Op, O: Ordering> Buffered<T, Op, O>
where
    Op: Operator<T, O>,
{
    fn new(operator: Op) -> Self {
        Self {
            operator,
            batch: ReusableIntoIter::new(),
            _marker: PhantomData,
        }
    }

    #[inline]
    fn next_item(&mut self) -> Option<T> {
        loop {
            if let Some(item) = self.batch.next() {
                return Some(item);
            }

            let mut batch = Batch::new(std::mem::take(&mut self.batch).into_vec());
            self.operator.next(&mut batch);
            if batch.data.is_empty() {
                self.batch = ReusableIntoIter::from_vec(batch.data);
                return None;
            }
            self.batch = ReusableIntoIter::from_vec(batch.data);
        }
    }
}

struct Map<T, U, Op, F, O: Ordering>
where
    Op: Operator<T, O>,
{
    input: Buffered<T, Op, O>,
    f: F,
    _output: PhantomData<U>,
}

impl<T, U, Op, F, O: Ordering> Map<T, U, Op, F, O>
where
    Op: Operator<T, O>,
{
    fn new(input: Op, f: F) -> Self {
        Self {
            input: Buffered::new(input),
            f,
            _output: PhantomData,
        }
    }
}

impl<T, U, Op, F, O: Ordering> Operator<U, O> for Map<T, U, Op, F, O>
where
    Op: Operator<T, O>,
    F: FnMut(T) -> U,
{
    fn next(&mut self, batch: &mut Batch<U, O>) {
        while batch.data.len() < SET_BATCH_SIZE {
            let Some(item) = self.input.next_item() else {
                break;
            };

            batch.data.push((self.f)(item));
        }
    }
}

struct Filter<T, Op, F, O: Ordering>
where
    Op: Operator<T, O>,
{
    input: Buffered<T, Op, O>,
    predicate: F,
}

impl<T, Op, F, O: Ordering> Filter<T, Op, F, O>
where
    Op: Operator<T, O>,
{
    fn new(input: Op, predicate: F) -> Self {
        Self {
            input: Buffered::new(input),
            predicate,
        }
    }
}

impl<T, Op, F, O: Ordering> Operator<T, O> for Filter<T, Op, F, O>
where
    Op: Operator<T, O>,
    F: FnMut(&T) -> bool,
{
    fn next(&mut self, batch: &mut Batch<T, O>) {
        while batch.data.len() < SET_BATCH_SIZE {
            let Some(item) = self.input.next_item() else {
                break;
            };

            if (self.predicate)(&item) {
                batch.data.push(item);
            }
        }
    }
}

struct MergeJoin<T, U, V, L, R, O: Ordering>
where
    L: Operator<(T, U), O>,
    R: Operator<(T, V), O>,
{
    left: Buffered<(T, U), L, O>,
    right: Buffered<(T, V), R, O>,
    left_item: Option<(T, U)>,
    right_item: Option<(T, V)>,
    pending: Vec<(T, U, V)>,
    pending_i: usize,
}

impl<T, U, V, L, R, O: Ordering> MergeJoin<T, U, V, L, R, O>
where
    L: Operator<(T, U), O>,
    R: Operator<(T, V), O>,
{
    fn new(left: L, right: R) -> Self {
        Self {
            left: Buffered::new(left),
            right: Buffered::new(right),
            left_item: None,
            right_item: None,
            pending: Vec::new(),
            pending_i: 0,
        }
    }
}

impl<T, U, V, L, R, O: Ordering> MergeJoin<T, U, V, L, R, O>
where
    T: Ord + Clone,
    U: Clone,
    V: Clone,
    L: Operator<(T, U), O>,
    R: Operator<(T, V), O>,
{
    fn fill_pending(&mut self) -> bool {
        loop {
            if self.left_item.is_none() {
                self.left_item = self.left.next_item();
            }
            if self.right_item.is_none() {
                self.right_item = self.right.next_item();
            }

            let (Some((left_key, _)), Some((right_key, _))) = (&self.left_item, &self.right_item)
            else {
                return false;
            };

            match O::cmp(left_key, right_key) {
                std::cmp::Ordering::Less => self.left_item = None,
                std::cmp::Ordering::Greater => self.right_item = None,
                std::cmp::Ordering::Equal => {
                    let key = left_key.clone();
                    let mut left_group = Vec::new();
                    let mut right_group = Vec::new();

                    while let Some((left_key, left_value)) = self.left_item.take() {
                        if left_key != key {
                            self.left_item = Some((left_key, left_value));
                            break;
                        }
                        left_group.push(left_value);
                        self.left_item = self.left.next_item();
                    }

                    while let Some((right_key, right_value)) = self.right_item.take() {
                        if right_key != key {
                            self.right_item = Some((right_key, right_value));
                            break;
                        }
                        right_group.push(right_value);
                        self.right_item = self.right.next_item();
                    }

                    self.pending.clear();
                    self.pending_i = 0;
                    for left_value in &left_group {
                        for right_value in &right_group {
                            self.pending.push((
                                key.clone(),
                                left_value.clone(),
                                right_value.clone(),
                            ));
                        }
                    }
                    return !self.pending.is_empty();
                }
            }
        }
    }
}

impl<T, U, V, L, R, O: Ordering> Operator<(T, U, V), O> for MergeJoin<T, U, V, L, R, O>
where
    T: Ord + Clone,
    U: Clone,
    V: Clone,
    L: Operator<(T, U), O>,
    R: Operator<(T, V), O>,
{
    fn next(&mut self, batch: &mut Batch<(T, U, V), O>) {
        while batch.data.len() < SET_BATCH_SIZE {
            if self.pending_i == self.pending.len() && !self.fill_pending() {
                break;
            }

            batch.data.push(self.pending[self.pending_i].clone());
            self.pending_i += 1;
        }
    }
}

struct Union<T, U, V, O: Ordering>
where
    U: Operator<T, O>,
    V: Operator<T, O>,
{
    left: Buffered<T, U, O>,
    right: Buffered<T, V, O>,
    left_item: Option<T>,
    right_item: Option<T>,
    last: Option<T>,
}

impl<T, U, V, O: Ordering> Union<T, U, V, O>
where
    U: Operator<T, O>,
    V: Operator<T, O>,
{
    fn new(left: U, right: V) -> Self {
        Self {
            left: Buffered::new(left),
            right: Buffered::new(right),
            left_item: None,
            right_item: None,
            last: None,
        }
    }
}

impl<T, U, V, O: Ordering> Operator<T, O> for Union<T, U, V, O>
where
    T: Ord + Clone,
    U: Operator<T, O>,
    V: Operator<T, O>,
{
    fn next(&mut self, batch: &mut Batch<T, O>) {
        while batch.data.len() < SET_BATCH_SIZE {
            if self.left_item.is_none() {
                self.left_item = self.left.next_item();
            }
            if self.right_item.is_none() {
                self.right_item = self.right.next_item();
            }

            let item = match (&self.left_item, &self.right_item) {
                (Some(left), Some(right)) => match O::cmp(left, right) {
                    std::cmp::Ordering::Less => self.left_item.take().unwrap(),
                    std::cmp::Ordering::Equal => {
                        self.right_item = None;
                        self.left_item.take().unwrap()
                    }
                    std::cmp::Ordering::Greater => self.right_item.take().unwrap(),
                },
                (Some(_), None) => self.left_item.take().unwrap(),
                (None, Some(_)) => self.right_item.take().unwrap(),
                (None, None) => break,
            };

            if self.last.as_ref() != Some(&item) {
                self.last = Some(item.clone());
                batch.data.push(item);
            }
        }
    }
}

struct Intersection<T, U, V, O: Ordering>
where
    U: Operator<T, O>,
    V: Operator<T, O>,
{
    left: Buffered<T, U, O>,
    right: Buffered<T, V, O>,
    left_item: Option<T>,
    right_item: Option<T>,
    last: Option<T>,
}

impl<T, U, V, O: Ordering> Intersection<T, U, V, O>
where
    U: Operator<T, O>,
    V: Operator<T, O>,
{
    fn new(left: U, right: V) -> Self {
        Self {
            left: Buffered::new(left),
            right: Buffered::new(right),
            left_item: None,
            right_item: None,
            last: None,
        }
    }
}

impl<T, U, V, O: Ordering> Operator<T, O> for Intersection<T, U, V, O>
where
    T: Ord + Clone,
    U: Operator<T, O>,
    V: Operator<T, O>,
{
    fn next(&mut self, batch: &mut Batch<T, O>) {
        while batch.data.len() < SET_BATCH_SIZE {
            if self.left_item.is_none() {
                self.left_item = self.left.next_item();
            }
            if self.right_item.is_none() {
                self.right_item = self.right.next_item();
            }

            match (&self.left_item, &self.right_item) {
                (Some(left), Some(right)) => match O::cmp(left, right) {
                    std::cmp::Ordering::Less => self.left_item = None,
                    std::cmp::Ordering::Greater => self.right_item = None,
                    std::cmp::Ordering::Equal => {
                        let item = self.left_item.take().unwrap();
                        self.right_item = None;
                        if self.last.as_ref() != Some(&item) {
                            self.last = Some(item.clone());
                            batch.data.push(item);
                        }
                    }
                },
                _ => break,
            }
        }
    }
}

struct DagRange<'a, X, Y, XO: Ordering, YO: Ordering>
where
    X: Operator<Rev, XO>,
    Y: Operator<Rev, YO>,
{
    x: Option<X>,
    y: Option<Y>,
    edges: &'a Index<(Rev, Rev), Forwards>,
    _marker: PhantomData<(XO, YO)>,
}

impl<'a, X, Y, XO: Ordering, YO: Ordering> DagRange<'a, X, Y, XO, YO>
where
    X: Operator<Rev, XO>,
    Y: Operator<Rev, YO>,
{
    fn new(x: X, y: Y, edges: &'a Index<(Rev, Rev), Forwards>) -> Self {
        Self {
            x: Some(x),
            y: Some(y),
            edges,
            _marker: PhantomData,
        }
    }

    fn compute(&mut self) -> Batch<Rev, Forwards> {
        let Some(mut x) = self.x.take() else {
            return Batch::new(Vec::new());
        };
        let Some(mut y) = self.y.take() else {
            return Batch::new(Vec::new());
        };

        let x_revs = Batch::<Rev, Forwards>::new(x.iter().collect());
        let y_revs = Batch::<Rev, Forwards>::new(y.iter().collect());
        if x_revs.data.is_empty() || y_revs.data.is_empty() {
            return Batch::new(Vec::new());
        }

        let scan_start = *x_revs.data.iter().min().unwrap();
        let scan_end = *y_revs.data.iter().max().unwrap();

        let mut descendants: HashSet<Rev> = x_revs.data.iter().copied().collect();
        let mut scan = self
            .edges
            .cursor()
            .with_start((scan_start, Rev(u32::MAX)))
            .with_end((scan_end, Rev(0)));
        for (from, to) in
            <Scan<'_, (Rev, Rev), Forwards> as Operator<(Rev, Rev), Forwards>>::iter(&mut scan)
        {
            if descendants.contains(&from) {
                descendants.insert(to);
            }
        }

        let mut reverse_edges = Batch::<(Rev, Rev), Backwards>::new(Vec::new());
        for &(from, to) in &self.edges.data {
            if descendants.contains(&from) && descendants.contains(&to) {
                reverse_edges.data.push((to, from));
            }
        }
        reverse_edges.data.sort_unstable();
        reverse_edges.data.reverse();

        let mut seen = HashSet::new();
        let mut result = Batch::<Rev, Forwards>::new(Vec::new());
        for rev in y_revs
            .data
            .into_iter()
            .filter(|rev| descendants.contains(rev))
        {
            if seen.insert(rev) {
                result.data.push(rev);
            }
        }

        for (from, to) in reverse_edges.data {
            if seen.contains(&from) && seen.insert(to) {
                result.data.push(to);
            }
        }

        result.data.reverse();
        result
    }
}

impl<X, Y, XO: Ordering, YO: Ordering> Operator<Rev, Forwards> for DagRange<'_, X, Y, XO, YO>
where
    X: Operator<Rev, XO>,
    Y: Operator<Rev, YO>,
{
    fn next(&mut self, batch: &mut Batch<Rev, Forwards>) {
        if self.x.is_none() {
            return;
        }

        let mut result = self.compute();
        std::mem::swap(&mut batch.data, &mut result.data);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn constant_returns_sorted_data_once() {
        let mut constant = Constant::<_, Forwards>::new(vec![Rev(2), Rev(5), Rev(3)]);
        assert_eq!(
            <Constant<Rev, Forwards> as Operator<Rev, Forwards>>::iter(&mut constant)
                .collect::<Vec<_>>(),
            vec![Rev(5), Rev(3), Rev(2)]
        );
        assert!(
            <Constant<Rev, Forwards> as Operator<Rev, Forwards>>::iter(&mut constant)
                .next()
                .is_none()
        );
    }

    #[test]
    fn scan_reads_descending_range() {
        let index = Index::<_, Forwards>::new(vec![Rev(5), Rev(4), Rev(3), Rev(2), Rev(1)]);
        let mut scan = index.cursor().with_start(Rev(4)).with_end(Rev(2));
        assert_eq!(
            scan.iter().collect::<Vec<_>>(),
            vec![Rev(4), Rev(3), Rev(2)]
        );
    }

    #[test]
    fn closure_follows_reachable_edges() {
        let start = Constant::<_, Forwards>::new(vec![Rev(3)]);
        let edges = Constant::<_, Forwards>::new(vec![
            (Rev(1), Rev(0)),
            (Rev(2), Rev(1)),
            (Rev(3), Rev(2)),
            (Rev(4), Rev(5)),
        ]);
        let mut closure: Closure<_, _, _, Forwards> = Closure::new(start, edges);

        assert_eq!(
            closure.iter().collect::<Vec<_>>(),
            vec![Rev(2), Rev(1), Rev(0)]
        );
    }

    #[test]
    fn closure_limits_output_batches() {
        let start = Constant::<_, Forwards>::new(vec![Rev(2000)]);
        let edges =
            Constant::<_, Forwards>::new((1..=2000).map(|i| (Rev(i), Rev(i - 1))).collect());
        let mut closure: Closure<_, _, _, Forwards> = Closure::new(start, edges);

        let mut batch = Batch::new(Vec::new());
        closure.next(&mut batch);
        assert_eq!(batch.data.len(), CLOSURE_BATCH_SIZE);
        assert_eq!(batch.data.first(), Some(&Rev(1999)));
        assert_eq!(batch.data.last(), Some(&Rev(976)));

        batch.data.clear();
        closure.next(&mut batch);
        assert_eq!(batch.data.len(), 2000 - CLOSURE_BATCH_SIZE);
        assert_eq!(batch.data.first(), Some(&Rev(975)));
        assert_eq!(batch.data.last(), Some(&Rev(0)));
    }

    #[test]
    fn union_merges_sorted_inputs() {
        let left = Constant::<_, Forwards>::new(vec![Rev(5), Rev(3), Rev(1)]);
        let right = Constant::<_, Forwards>::new(vec![Rev(4), Rev(3), Rev(2)]);
        let mut union: Union<_, _, _, Forwards> = Union::new(left, right);

        assert_eq!(
            union.iter().collect::<Vec<_>>(),
            vec![Rev(5), Rev(4), Rev(3), Rev(2), Rev(1)]
        );
    }

    #[test]
    fn intersection_returns_common_items() {
        let left = Constant::<_, Forwards>::new(vec![Rev(5), Rev(4), Rev(3), Rev(1)]);
        let right = Constant::<_, Forwards>::new(vec![Rev(6), Rev(4), Rev(3), Rev(2)]);
        let mut intersection: Intersection<_, _, _, Forwards> = Intersection::new(left, right);

        assert_eq!(
            intersection.iter().collect::<Vec<_>>(),
            vec![Rev(4), Rev(3)]
        );
    }

    #[test]
    fn intersection_preserves_backwards_ordering() {
        let left = Constant::<_, Backwards>::new(vec![Rev(5), Rev(4), Rev(3), Rev(1)]);
        let right = Constant::<_, Backwards>::new(vec![Rev(6), Rev(4), Rev(3), Rev(2)]);
        let mut intersection: Intersection<_, _, _, Backwards> = Intersection::new(left, right);

        assert_eq!(
            intersection.iter().collect::<Vec<_>>(),
            vec![Rev(3), Rev(4)]
        );
    }

    #[test]
    fn filter_keeps_matching_items() {
        let input = Constant::<_, Forwards>::new(vec![Rev(5), Rev(4), Rev(3), Rev(2), Rev(1)]);
        let mut filter: Filter<_, _, _, Forwards> = Filter::new(input, |rev: &Rev| rev.0 % 2 == 0);

        assert_eq!(filter.iter().collect::<Vec<_>>(), vec![Rev(4), Rev(2)]);
    }

    #[test]
    fn merge_join_matches_equal_keys() {
        let left = Constant::<_, Forwards>::new(vec![
            (Rev(3), "a"),
            (Rev(2), "b"),
            (Rev(2), "c"),
            (Rev(1), "d"),
        ]);
        let right = Constant::<_, Forwards>::new(vec![
            (Rev(2), 10),
            (Rev(2), 20),
            (Rev(1), 30),
            (Rev(0), 40),
        ]);
        let mut join: MergeJoin<_, _, _, _, _, Forwards> = MergeJoin::new(left, right);

        assert_eq!(
            join.iter().collect::<Vec<_>>(),
            vec![
                (Rev(2), "b", 10),
                (Rev(2), "b", 20),
                (Rev(2), "c", 10),
                (Rev(2), "c", 20),
                (Rev(1), "d", 30),
            ]
        );
    }

    #[test]
    fn map_transforms_items() {
        let input = Constant::<_, Forwards>::new(vec![Rev(3), Rev(2), Rev(1)]);
        let mut map: Map<_, _, _, _, Forwards> = Map::new(input, |rev: Rev| rev.0);

        assert_eq!(map.iter().collect::<Vec<_>>(), vec![3, 2, 1]);
    }

    #[test]
    fn dagrange_returns_descendants_of_x_that_reach_y() {
        let edges = Index::<_, Forwards>::new(vec![
            (Rev(5), Rev(4)),
            (Rev(4), Rev(3)),
            (Rev(4), Rev(2)),
            (Rev(3), Rev(1)),
            (Rev(2), Rev(1)),
            (Rev(9), Rev(8)),
        ]);
        let x = Constant::<_, Forwards>::new(vec![Rev(4)]);
        let y = Constant::<_, Forwards>::new(vec![Rev(1)]);
        let mut range = DagRange::new(x, y, &edges);

        assert_eq!(
            range.iter().collect::<Vec<_>>(),
            vec![Rev(4), Rev(3), Rev(2), Rev(1)]
        );
    }

    #[test]
    fn dagrange_accepts_any_input_ordering() {
        let edges = Index::<_, Forwards>::new(vec![
            (Rev(5), Rev(4)),
            (Rev(4), Rev(3)),
            (Rev(4), Rev(2)),
            (Rev(3), Rev(1)),
            (Rev(2), Rev(1)),
        ]);
        let x = Constant::<_, Backwards>::new(vec![Rev(4)]);
        let y = Constant::<_, Unordered>::new(vec![Rev(1)]);
        let mut range = DagRange::new(x, y, &edges);

        assert_eq!(
            range.iter().collect::<Vec<_>>(),
            vec![Rev(4), Rev(3), Rev(2), Rev(1)]
        );
    }

    #[test]
    fn dagrange_is_empty_when_y_is_not_below_x() {
        let edges = Index::<_, Forwards>::new(vec![(Rev(5), Rev(4)), (Rev(3), Rev(2))]);
        let x = Constant::<_, Forwards>::new(vec![Rev(5)]);
        let y = Constant::<_, Forwards>::new(vec![Rev(2)]);
        let mut range = DagRange::new(x, y, &edges);

        assert_eq!(range.iter().collect::<Vec<_>>(), vec![]);
    }
}
