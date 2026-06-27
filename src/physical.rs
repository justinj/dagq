use std::collections::HashSet;

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
    v: V,
    set: HashSet<T>,
    edge_batch: Vec<(T, T)>,
    edge_i: usize,
}

impl<T, U, V> Closure<T, U, V>
where
    U: Operator<T>,
    V: Operator<(T, T)>,
{
    fn new(u: U, v: V) -> Self {
        Self {
            u: Some(u),
            v,
            set: HashSet::new(),
            edge_batch: Vec::new(),
            edge_i: 0,
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
            if self.edge_i == self.edge_batch.len() {
                self.edge_batch.clear();
                self.v.next(&mut self.edge_batch);
                self.edge_i = 0;

                if self.edge_batch.is_empty() {
                    break;
                }
            }

            let (from, to) = &self.edge_batch[self.edge_i];
            self.edge_i += 1;

            if self.set.contains(from) && self.set.insert(to.clone()) {
                batch.push(to.clone());
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
}
