mod reusable_iter;

use std::collections::HashSet;

use reusable_iter::ReusableIntoIter;

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

trait Operator<T> {
    fn next(&mut self, batch: &mut Vec<T>);

    fn iter(&mut self) -> OperatorIter<'_, Self, T>
    where
        Self: Sized,
    {
        OperatorIter {
            operator: self,
            batch: Vec::new().into_iter(),
        }
    }
}

struct OperatorIter<'a, O, T>
where
    O: Operator<T> + ?Sized,
{
    operator: &'a mut O,
    batch: std::vec::IntoIter<T>,
}

impl<O, T> Iterator for OperatorIter<'_, O, T>
where
    O: Operator<T> + ?Sized,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(item) = self.batch.next() {
                return Some(item);
            }

            let mut batch = Vec::new();
            self.operator.next(&mut batch);
            if batch.is_empty() {
                return None;
            }
            self.batch = batch.into_iter();
        }
    }
}

struct Constant<T> {
    data: Vec<T>,
}

impl<T: Ord> Constant<T> {
    fn new(mut data: Vec<T>) -> Self {
        data.sort_unstable();
        Self { data }
    }
}

impl<T> Operator<T> for Constant<T> {
    fn next(&mut self, batch: &mut Vec<T>) {
        std::mem::swap(&mut self.data, batch)
    }
}

struct Index<T> {
    data: Vec<T>,
}

impl<T: Ord> Index<T> {
    fn new(mut data: Vec<T>) -> Self {
        data.sort_unstable();
        Self { data }
    }

    fn cursor(&self) -> Scan<'_, T> {
        Scan {
            index: self,
            i: 0,
            end: None,
        }
    }
}

struct Scan<'a, T> {
    index: &'a Index<T>,
    i: usize,
    end: Option<T>,
}

impl<'a, T: Ord> Scan<'a, T> {
    fn with_start(mut self, start: T) -> Self {
        self.i = match self.index.data.binary_search(&start) {
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

impl<'a, T: Ord + Clone> Operator<T> for Scan<'a, T> {
    fn next(&mut self, batch: &mut Vec<T>) {
        while batch.len() < SCAN_BATCH_SIZE && self.i < self.index.data.len() {
            if let Some(end) = &self.end {
                if &self.index.data[self.i] > end {
                    break;
                }
            }

            batch.push(self.index.data[self.i].clone());
            self.i += 1;
        }
    }
}

struct Closure<T, U, V>
where
    U: Operator<T>,
    V: Operator<(T, T)>,
{
    u: Option<U>,
    v: Buffered<(T, T), V>,
    set: HashSet<T>,
}

impl<T, U, V> Closure<T, U, V>
where
    U: Operator<T>,
    V: Operator<(T, T)>,
{
    fn new(u: U, v: V) -> Self {
        Self {
            u: Some(u),
            v: Buffered::new(v),
            set: HashSet::new(),
        }
    }
}

impl<T, U, V> Operator<T> for Closure<T, U, V>
where
    T: Eq + std::hash::Hash + Clone,
    U: Operator<T>,
    V: Operator<(T, T)>,
{
    fn next(&mut self, batch: &mut Vec<T>) {
        if let Some(mut start) = self.u.take() {
            self.set.extend(start.iter());
        }

        while batch.len() < CLOSURE_BATCH_SIZE {
            let Some((from, to)) = self.v.next_item() else {
                break;
            };

            if self.set.contains(&from) && self.set.insert(to.clone()) {
                batch.push(to);
            }
        }
    }
}

struct Buffered<T, O>
where
    O: Operator<T>,
{
    operator: O,
    batch: ReusableIntoIter<T>,
}

impl<T, O> Buffered<T, O>
where
    O: Operator<T>,
{
    fn new(operator: O) -> Self {
        Self {
            operator,
            batch: ReusableIntoIter::new(),
        }
    }

    #[inline]
    fn next_item(&mut self) -> Option<T> {
        loop {
            if let Some(item) = self.batch.next() {
                return Some(item);
            }

            let mut batch = std::mem::take(&mut self.batch).into_vec();
            self.operator.next(&mut batch);
            if batch.is_empty() {
                self.batch = ReusableIntoIter::from_vec(batch);
                return None;
            }
            self.batch = ReusableIntoIter::from_vec(batch);
        }
    }
}

struct Map<T, U, O, F>
where
    O: Operator<T>,
{
    input: Buffered<T, O>,
    f: F,
    _output: std::marker::PhantomData<U>,
}

impl<T, U, O, F> Map<T, U, O, F>
where
    O: Operator<T>,
{
    fn new(input: O, f: F) -> Self {
        Self {
            input: Buffered::new(input),
            f,
            _output: std::marker::PhantomData,
        }
    }
}

impl<T, U, O, F> Operator<U> for Map<T, U, O, F>
where
    O: Operator<T>,
    F: FnMut(T) -> U,
{
    fn next(&mut self, batch: &mut Vec<U>) {
        while batch.len() < SET_BATCH_SIZE {
            let Some(item) = self.input.next_item() else {
                break;
            };

            batch.push((self.f)(item));
        }
    }
}

struct Filter<T, O, F>
where
    O: Operator<T>,
{
    input: Buffered<T, O>,
    predicate: F,
}

impl<T, O, F> Filter<T, O, F>
where
    O: Operator<T>,
{
    fn new(input: O, predicate: F) -> Self {
        Self {
            input: Buffered::new(input),
            predicate,
        }
    }
}

impl<T, O, F> Operator<T> for Filter<T, O, F>
where
    O: Operator<T>,
    F: FnMut(&T) -> bool,
{
    fn next(&mut self, batch: &mut Vec<T>) {
        while batch.len() < SET_BATCH_SIZE {
            let Some(item) = self.input.next_item() else {
                break;
            };

            if (self.predicate)(&item) {
                batch.push(item);
            }
        }
    }
}

struct MergeJoin<T, U, V, L, R>
where
    L: Operator<(T, U)>,
    R: Operator<(T, V)>,
{
    left: Buffered<(T, U), L>,
    right: Buffered<(T, V), R>,
    left_item: Option<(T, U)>,
    right_item: Option<(T, V)>,
    pending: Vec<(T, U, V)>,
    pending_i: usize,
}

impl<T, U, V, L, R> MergeJoin<T, U, V, L, R>
where
    L: Operator<(T, U)>,
    R: Operator<(T, V)>,
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

impl<T, U, V, L, R> MergeJoin<T, U, V, L, R>
where
    T: Ord + Clone,
    U: Clone,
    V: Clone,
    L: Operator<(T, U)>,
    R: Operator<(T, V)>,
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

            match left_key.cmp(right_key) {
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
                            self.pending.push((key.clone(), left_value.clone(), right_value.clone()));
                        }
                    }
                    return !self.pending.is_empty();
                }
            }
        }
    }
}

impl<T, U, V, L, R> Operator<(T, U, V)> for MergeJoin<T, U, V, L, R>
where
    T: Ord + Clone,
    U: Clone,
    V: Clone,
    L: Operator<(T, U)>,
    R: Operator<(T, V)>,
{
    fn next(&mut self, batch: &mut Vec<(T, U, V)>) {
        while batch.len() < SET_BATCH_SIZE {
            if self.pending_i == self.pending.len() && !self.fill_pending() {
                break;
            }

            batch.push(self.pending[self.pending_i].clone());
            self.pending_i += 1;
        }
    }
}

struct Union<T, U, V>
where
    U: Operator<T>,
    V: Operator<T>,
{
    left: Buffered<T, U>,
    right: Buffered<T, V>,
    left_item: Option<T>,
    right_item: Option<T>,
    last: Option<T>,
}

impl<T, U, V> Union<T, U, V>
where
    U: Operator<T>,
    V: Operator<T>,
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

impl<T, U, V> Operator<T> for Union<T, U, V>
where
    T: Ord + Clone,
    U: Operator<T>,
    V: Operator<T>,
{
    fn next(&mut self, batch: &mut Vec<T>) {
        while batch.len() < SET_BATCH_SIZE {
            if self.left_item.is_none() {
                self.left_item = self.left.next_item();
            }
            if self.right_item.is_none() {
                self.right_item = self.right.next_item();
            }

            let item = match (&self.left_item, &self.right_item) {
                (Some(left), Some(right)) => match left.cmp(right) {
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
                batch.push(item);
            }
        }
    }
}

struct Intersection<T, U, V>
where
    U: Operator<T>,
    V: Operator<T>,
{
    left: Buffered<T, U>,
    right: Buffered<T, V>,
    left_item: Option<T>,
    right_item: Option<T>,
    last: Option<T>,
}

impl<T, U, V> Intersection<T, U, V>
where
    U: Operator<T>,
    V: Operator<T>,
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

impl<T, U, V> Operator<T> for Intersection<T, U, V>
where
    T: Ord + Clone,
    U: Operator<T>,
    V: Operator<T>,
{
    fn next(&mut self, batch: &mut Vec<T>) {
        while batch.len() < SET_BATCH_SIZE {
            if self.left_item.is_none() {
                self.left_item = self.left.next_item();
            }
            if self.right_item.is_none() {
                self.right_item = self.right.next_item();
            }

            match (&self.left_item, &self.right_item) {
                (Some(left), Some(right)) => match left.cmp(right) {
                    std::cmp::Ordering::Less => self.left_item = None,
                    std::cmp::Ordering::Greater => self.right_item = None,
                    std::cmp::Ordering::Equal => {
                        let item = self.left_item.take().unwrap();
                        self.right_item = None;
                        if self.last.as_ref() != Some(&item) {
                            self.last = Some(item.clone());
                            batch.push(item);
                        }
                    }
                },
                _ => break,
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn constant_returns_sorted_data_once() {
        let mut constant = Constant::new(vec![Rev(2), Rev(5), Rev(3)]);
        assert_eq!(
            constant.iter().collect::<Vec<_>>(),
            vec![Rev(5), Rev(3), Rev(2)]
        );
        assert!(constant.iter().next().is_none());
    }

    #[test]
    fn scan_reads_descending_range() {
        let index = Index {
            data: vec![Rev(5), Rev(4), Rev(3), Rev(2), Rev(1)],
        };
        let mut scan = index.cursor().with_start(Rev(4)).with_end(Rev(2));
        assert_eq!(
            scan.iter().collect::<Vec<_>>(),
            vec![Rev(4), Rev(3), Rev(2)]
        );
    }

    #[test]
    fn closure_follows_reachable_edges() {
        let start = Constant::new(vec![Rev(3)]);
        let edges = Constant::new(vec![
            (Rev(1), Rev(0)),
            (Rev(2), Rev(1)),
            (Rev(3), Rev(2)),
            (Rev(4), Rev(5)),
        ]);
        let mut closure = Closure::new(start, edges);

        assert_eq!(
            closure.iter().collect::<Vec<_>>(),
            vec![Rev(2), Rev(1), Rev(0)]
        );
    }

    #[test]
    fn closure_limits_output_batches() {
        let start = Constant::new(vec![Rev(2000)]);
        let edges = Constant::new((1..=2000).map(|i| (Rev(i), Rev(i - 1))).collect());
        let mut closure = Closure::new(start, edges);

        let mut batch = Vec::new();
        closure.next(&mut batch);
        assert_eq!(batch.len(), CLOSURE_BATCH_SIZE);
        assert_eq!(batch.first(), Some(&Rev(1999)));
        assert_eq!(batch.last(), Some(&Rev(976)));

        batch.clear();
        closure.next(&mut batch);
        assert_eq!(batch.len(), 2000 - CLOSURE_BATCH_SIZE);
        assert_eq!(batch.first(), Some(&Rev(975)));
        assert_eq!(batch.last(), Some(&Rev(0)));
    }

    #[test]
    fn union_merges_sorted_inputs() {
        let left = Constant::new(vec![Rev(5), Rev(3), Rev(1)]);
        let right = Constant::new(vec![Rev(4), Rev(3), Rev(2)]);
        let mut union = Union::new(left, right);

        assert_eq!(
            union.iter().collect::<Vec<_>>(),
            vec![Rev(5), Rev(4), Rev(3), Rev(2), Rev(1)]
        );
    }

    #[test]
    fn intersection_returns_common_items() {
        let left = Constant::new(vec![Rev(5), Rev(4), Rev(3), Rev(1)]);
        let right = Constant::new(vec![Rev(6), Rev(4), Rev(3), Rev(2)]);
        let mut intersection = Intersection::new(left, right);

        assert_eq!(
            intersection.iter().collect::<Vec<_>>(),
            vec![Rev(4), Rev(3)]
        );
    }

    #[test]
    fn filter_keeps_matching_items() {
        let input = Constant::new(vec![Rev(5), Rev(4), Rev(3), Rev(2), Rev(1)]);
        let mut filter = Filter::new(input, |rev: &Rev| rev.0 % 2 == 0);

        assert_eq!(filter.iter().collect::<Vec<_>>(), vec![Rev(4), Rev(2)]);
    }

    #[test]
    fn merge_join_matches_equal_keys() {
        let left = Constant::new(vec![
            (Rev(3), "a"),
            (Rev(2), "b"),
            (Rev(2), "c"),
            (Rev(1), "d"),
        ]);
        let right = Constant::new(vec![
            (Rev(2), 10),
            (Rev(2), 20),
            (Rev(1), 30),
            (Rev(0), 40),
        ]);
        let mut join = MergeJoin::new(left, right);

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
        let input = Constant::new(vec![Rev(3), Rev(2), Rev(1)]);
        let mut map = Map::new(input, |rev: Rev| rev.0);

        assert_eq!(map.iter().collect::<Vec<_>>(), vec![3, 2, 1]);
    }
}
