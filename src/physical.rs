mod reusable_iter;

use std::{
    collections::{HashSet, VecDeque},
    marker::PhantomData,
};

use reusable_iter::ReusableIntoIter;

// TODO: there should be another trait that only Forwards and Backwards implement.

pub(super) trait Ordering {
    type Reversed: Ordering;

    const SORTED: bool;

    fn cmp<T: Ord>(left: &T, right: &T) -> std::cmp::Ordering;

    fn min<T: Ord>(left: T, right: T) -> T {
        match Self::cmp(&left, &right) {
            std::cmp::Ordering::Greater => right,
            std::cmp::Ordering::Less | std::cmp::Ordering::Equal => left,
        }
    }

    fn max<T: Ord>(left: T, right: T) -> T {
        match Self::cmp(&left, &right) {
            std::cmp::Ordering::Greater => left,
            std::cmp::Ordering::Less | std::cmp::Ordering::Equal => right,
        }
    }

    fn sort<T: Ord>(data: &mut [T]) {
        if Self::SORTED {
            data.sort_unstable_by(Self::cmp);
        }
    }
}

// Forwards = Child -> Parent
#[derive(Debug)]
pub(super) enum Forwards {}

impl Ordering for Forwards {
    type Reversed = Backwards;

    const SORTED: bool = true;

    fn cmp<T: Ord>(left: &T, right: &T) -> std::cmp::Ordering {
        left.cmp(right)
    }
}

// Backwards = Parent -> Child
#[derive(Debug)]
pub(super) enum Backwards {}

impl Ordering for Backwards {
    type Reversed = Forwards;

    const SORTED: bool = true;

    fn cmp<T: Ord>(left: &T, right: &T) -> std::cmp::Ordering {
        right.cmp(left)
    }
}

pub(super) enum Unordered {}
impl Ordering for Unordered {
    type Reversed = Unordered;

    const SORTED: bool = false;

    fn cmp<T: Ord>(_: &T, _: &T) -> std::cmp::Ordering {
        std::cmp::Ordering::Equal
    }
}

pub(super) struct Batch<T, O: Ordering> {
    pub(super) data: Vec<T>,
    _marker: PhantomData<O>,
}

impl<T, O: Ordering> Batch<T, O> {
    pub(super) fn new(data: Vec<T>) -> Self {
        Batch {
            data,
            _marker: PhantomData,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Rev(pub(super) u32);

impl Rev {
    pub const MIN: Self = Rev(u32::MAX);
    pub const MAX: Self = Rev(u32::MIN);
}

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

pub(super) type BoxOperator<'a, T, O> = Box<dyn Operator<T, O> + 'a>;

pub(super) trait Operator<T, O: Ordering> {
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

    fn closure<V>(self, edges: V) -> Closure<T, Self, V, O>
    where
        Self: Sized,
        V: Operator<(T, T), O>,
    {
        Closure::new(self, edges)
    }

    fn map<U, F>(self, f: F) -> Map<T, U, Self, F, O>
    where
        Self: Sized,
    {
        Map::new(self, f)
    }

    fn filter<F>(self, predicate: F) -> Filter<T, Self, F, O>
    where
        Self: Sized,
    {
        Filter::new(self, predicate)
    }

    fn union<V>(self, right: V) -> Union<T, Self, V, O>
    where
        Self: Sized,
        V: Operator<T, O>,
    {
        Union::new(self, right)
    }

    fn intersection<V>(self, right: V) -> Intersection<T, Self, V, O>
    where
        Self: Sized,
        V: Operator<T, O>,
    {
        Intersection::new(self, right)
    }

    fn difference<V>(self, right: V) -> Difference<T, Self, V, O>
    where
        Self: Sized,
        V: Operator<T, O>,
    {
        Difference::new(self, right)
    }

    fn ancestors<'a>(
        self,
        edges: &'a Index<(Rev, Rev), Forwards>,
    ) -> Closure<Rev, Self, Scan<'a, (Rev, Rev), Forwards>, Forwards>
    where
        Self: Sized + Operator<Rev, Forwards>,
    {
        Closure::new(self, edges.scan())
    }

    fn descendants<'a>(
        self,
        edges: &'a Index<(Rev, Rev), Backwards>,
    ) -> Closure<Rev, Self, Scan<'a, (Rev, Rev), Backwards>, Backwards>
    where
        Self: Sized + Operator<Rev, Backwards>,
    {
        Closure::new(self, edges.scan())
    }

    fn range<'a, Y, YO: Ordering>(
        self,
        y: Y,
        descendants: &'a Index<(Rev, Rev), Backwards>,
        ancestors: &'a Index<(Rev, Rev), Forwards>,
    ) -> DagRange<'a, Self, Y, O, YO>
    where
        Self: Sized + Operator<Rev, O>,
        Y: Operator<Rev, YO>,
    {
        DagRange::new(self, y, descendants, ancestors)
    }

    fn reverse(self) -> Reverse<T, Self, O>
    where
        Self: Sized,
    {
        Reverse::new(self)
    }
}

impl<T, O: Ordering> Operator<T, O> for Box<dyn Operator<T, O> + '_> {
    fn next(&mut self, batch: &mut Batch<T, O>) {
        self.as_mut().next(batch)
    }
}

pub(super) struct OperatorIter<'a, Op, T, O: Ordering>
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

pub(super) struct Constant<T, O: Ordering> {
    data: Vec<T>,
    _marker: PhantomData<O>,
}

impl<T: Ord, O: Ordering> Constant<T, O> {
    pub(super) fn new(mut data: Vec<T>) -> Self {
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

pub(super) struct Index<T, O: Ordering> {
    pub(super) data: Vec<T>,
    _marker: PhantomData<O>,
}

impl<T: Ord, O: Ordering> Index<T, O> {
    pub(super) fn new(mut data: Vec<T>) -> Self {
        O::sort(&mut data);
        Self {
            data,
            _marker: PhantomData,
        }
    }

    pub(super) fn scan(&self) -> Scan<'_, T, O> {
        Scan {
            index: self,
            i: 0,
            end: None,
        }
    }
}

// RevRange represents a subarray of a Rev-ordered index. It is inclusive lower
// and upper.
#[derive(Debug)]
struct RevRange<O: Ordering> {
    lower: Rev,
    upper: Rev,
    _marker: PhantomData<O>,
}

impl<O: Ordering> Default for RevRange<O> {
    fn default() -> Self {
        RevRange {
            lower: O::min(Rev::MIN, Rev::MAX),
            upper: O::max(Rev::MIN, Rev::MAX),
            _marker: PhantomData,
        }
    }
}

impl<O: Ordering> RevRange<O> {
    fn empty() -> Self {
        RevRange {
            lower: O::max(Rev::MIN, Rev::MAX),
            upper: O::min(Rev::MIN, Rev::MAX),
            _marker: PhantomData,
        }
    }

    fn union(mut self, other: RevRange<O>) -> Self {
        self.lower = O::min(self.lower, other.lower);
        self.upper = O::max(self.upper, other.upper);
        self
    }

    fn intersect(mut self, other: RevRange<O>) -> Self {
        self.lower = O::max(self.lower, other.lower);
        self.upper = O::min(self.upper, other.upper);
        self
    }

    fn is_empty(&self) -> bool {
        O::cmp(&self.lower, &self.upper) == std::cmp::Ordering::Greater
    }

    fn open_upper(r: Rev) -> Self {
        Self {
            lower: r,
            upper: O::max(Rev::MIN, Rev::MAX),
            _marker: PhantomData,
        }
    }

    fn open_lower(r: Rev) -> Self {
        Self {
            lower: O::min(Rev::MIN, Rev::MAX),
            upper: r,
            _marker: PhantomData,
        }
    }
}

pub(super) struct Scan<'a, T, O: Ordering> {
    index: &'a Index<T, O>,
    i: usize,
    end: Option<T>,
}

// TODO: idk this sort of sucks
impl<'a, O: Ordering> Scan<'a, (Rev, Rev), O> {
    fn constrain(mut self, range: RevRange<O>) -> Self {
        self.with_start((range.lower, Rev::MIN))
            .with_end((range.upper, Rev::MAX))
    }
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

pub(super) struct Closure<T, U, V, O: Ordering>
where
    U: Operator<T, O>,
    V: Operator<(T, T), O>,
{
    u: Option<U>,
    pending: VecDeque<T>,
    pending_item: Option<T>,
    edge_item: Option<T>,
    v: Buffered<(T, T), V, O>,
    set: HashSet<T>,
}

impl<T, U, V, O: Ordering> Closure<T, U, V, O>
where
    U: Operator<T, O>,
    V: Operator<(T, T), O>,
{
    pub(super) fn new(u: U, v: V) -> Self {
        Self {
            u: Some(u),
            pending: VecDeque::new(),
            pending_item: None,
            edge_item: None,
            v: Buffered::new(v),
            set: HashSet::new(),
        }
    }
}

impl<T, U, V, O: Ordering> Operator<T, O> for Closure<T, U, V, O>
where
    T: Eq + std::hash::Hash + Clone + Ord,
    U: Operator<T, O>,
    V: Operator<(T, T), O>,
{
    fn next(&mut self, batch: &mut Batch<T, O>) {
        if let Some(mut start) = self.u.take() {
            for item in start.iter() {
                self.set.insert(item.clone());
                self.pending.push_back(item);
            }
        }

        while batch.data.len() < CLOSURE_BATCH_SIZE {
            if self.pending_item.is_none() {
                self.pending_item = self.pending.pop_front();
            }

            if self.edge_item.is_none() {
                loop {
                    let Some((from, to)) = self.v.next_item() else {
                        break;
                    };
                    if self.set.contains(&from) && self.set.insert(to.clone()) {
                        self.edge_item = Some(to);
                        break;
                    }
                }
            }

            let item = match (&self.pending_item, &self.edge_item) {
                (Some(p), Some(e)) => match O::cmp(p, e) {
                    std::cmp::Ordering::Less | std::cmp::Ordering::Equal => {
                        self.pending_item.take().unwrap()
                    }
                    std::cmp::Ordering::Greater => self.edge_item.take().unwrap(),
                },
                (Some(_), None) => self.pending_item.take().unwrap(),
                (None, Some(_)) => self.edge_item.take().unwrap(),
                (None, None) => break,
            };
            batch.data.push(item);
        }
    }
}

pub(super) struct Reverse<T, Op, O: Ordering>
where
    Op: Operator<T, O>,
{
    input: Option<Op>,
    _marker: PhantomData<(T, O)>,
}

impl<T, Op, O: Ordering> Reverse<T, Op, O>
where
    Op: Operator<T, O>,
{
    pub(super) fn new(input: Op) -> Self {
        Self {
            input: Some(input),
            _marker: PhantomData,
        }
    }
}

impl<T, Op, O: Ordering> Operator<T, O::Reversed> for Reverse<T, Op, O>
where
    Op: Operator<T, O>,
{
    fn next(&mut self, batch: &mut Batch<T, O::Reversed>) {
        let Some(mut input) = self.input.take() else {
            return;
        };

        let mut data: Vec<_> = input.iter().collect();
        if O::SORTED {
            data.reverse();
        }
        std::mem::swap(&mut batch.data, &mut data);
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
    pub(super) fn new(operator: Op) -> Self {
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

pub(super) struct Map<T, U, Op, F, O: Ordering>
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
    pub(super) fn new(input: Op, f: F) -> Self {
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

pub(super) struct Filter<T, Op, F, O: Ordering>
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
    pub(super) fn new(input: Op, predicate: F) -> Self {
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

pub(super) struct MergeJoin<T, U, V, L, R, O: Ordering>
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
    pub(super) fn new(left: L, right: R) -> Self {
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

// TODO: optimization for set ops: when you get a new batch, peek at the last
// thing to see if you can just directly emit that batch (or nothing).
pub(super) struct Union<T, U, V, O: Ordering>
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
    pub(super) fn new(left: U, right: V) -> Self {
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

pub(super) struct Intersection<T, U, V, O: Ordering>
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
    pub(super) fn new(left: U, right: V) -> Self {
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

pub(super) struct Difference<T, U, V, O: Ordering>
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

impl<T, U, V, O: Ordering> Difference<T, U, V, O>
where
    U: Operator<T, O>,
    V: Operator<T, O>,
{
    pub(super) fn new(left: U, right: V) -> Self {
        Self {
            left: Buffered::new(left),
            right: Buffered::new(right),
            left_item: None,
            right_item: None,
            last: None,
        }
    }
}

impl<T, U, V, O: Ordering> Operator<T, O> for Difference<T, U, V, O>
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
                    std::cmp::Ordering::Less => self.left_item.take(),
                    std::cmp::Ordering::Equal => {
                        self.right_item = None;
                        self.left_item = None;
                        None
                    }
                    std::cmp::Ordering::Greater => {
                        self.right_item = None;
                        None
                    }
                },
                (Some(_), None) => self.left_item.take(),
                (None, _) => break,
            };

            if let Some(item) = item {
                if self.last.as_ref() != Some(&item) {
                    self.last = Some(item.clone());
                    batch.data.push(item);
                }
            }
        }
    }
}

pub(super) struct DagRange<'a, X, Y, XO: Ordering, YO: Ordering>
where
    X: Operator<Rev, XO>,
    Y: Operator<Rev, YO>,
{
    x: Option<X>,
    y: Option<Y>,
    descendants: &'a Index<(Rev, Rev), Backwards>,
    ancestors: &'a Index<(Rev, Rev), Forwards>,
    _marker: PhantomData<(XO, YO)>,
}

// x::y. roots::heads.
impl<'a, X, Y, XO: Ordering, YO: Ordering> DagRange<'a, X, Y, XO, YO>
where
    X: Operator<Rev, XO>,
    Y: Operator<Rev, YO>,
{
    pub(super) fn new(
        x: X,
        y: Y,
        descendants: &'a Index<(Rev, Rev), Backwards>,
        ancestors: &'a Index<(Rev, Rev), Forwards>,
    ) -> Self {
        Self {
            x: Some(x),
            y: Some(y),
            descendants,
            ancestors,
            _marker: PhantomData,
        }
    }

    fn compute(&mut self) -> Batch<Rev, Backwards> {
        let Some(mut x) = self.x.take() else {
            return Batch::new(Vec::new());
        };
        let Some(mut y) = self.y.take() else {
            return Batch::new(Vec::new());
        };

        let mut root_vec: Vec<_> = x.iter().collect();
        let heads: HashSet<_> = y.iter().collect();
        if root_vec.is_empty() || heads.is_empty() {
            return Batch::new(Vec::new());
        }

        let mut rev_index = Vec::new();

        // We want to constrain the scan such that we only look at the range
        // that possibly matches the revs, so we constrain the scan to:
        //  [latest descendant, earliest ancestor]
        let mut roots: HashSet<_> = root_vec.iter().copied().collect();

        // To figure out the range of the index we need to scan, we need to
        // first get every head and all of its potential ancestors (via
        // RevRange::open_upper) and union those, then do the same for every
        // root and its potential descendants (via RevRange::open_lower), then
        // intersect the results.

        let head_range = heads.iter().fold(RevRange::<Forwards>::empty(), |r, &n| {
            r.union(RevRange::open_upper(n))
        });

        let root_range = roots.iter().fold(RevRange::<Forwards>::empty(), |r, &n| {
            r.union(RevRange::open_lower(n))
        });

        let scan_range = head_range.intersect(root_range);

        let mut ancestry = heads;

        for (child, parent) in self.ancestors.scan().constrain(scan_range).iter() {
            rev_index.push((parent, child));
            if ancestry.contains(&child) {
                ancestry.insert(parent);
            }
        }

        root_vec.retain(|rev| ancestry.contains(rev));
        Backwards::sort(&mut root_vec);
        if root_vec.is_empty() {
            return Batch::new(Vec::new());
        }

        let mut result = Vec::new();
        let mut xi = 0;

        // Now we need to build a reverse index of parent -> child.
        rev_index.sort_unstable_by(|a, b| b.cmp(a));

        for &(parent, child) in &rev_index {
            if ancestry.contains(&child) && roots.contains(&parent) {
                if roots.insert(child) {
                    while xi < root_vec.len()
                        && Backwards::cmp(&root_vec[xi], &child) != std::cmp::Ordering::Greater
                    {
                        result.push(root_vec[xi]);
                        xi += 1;
                    }
                    result.push(child);
                }
            }
        }

        result.extend_from_slice(&root_vec[xi..]);
        Batch::<Rev, Backwards>::new(result)
    }
}

impl<X, Y, XO: Ordering, YO: Ordering> Operator<Rev, Backwards> for DagRange<'_, X, Y, XO, YO>
where
    X: Operator<Rev, XO>,
    Y: Operator<Rev, YO>,
{
    fn next(&mut self, batch: &mut Batch<Rev, Backwards>) {
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
        let mut scan = index.scan().with_start(Rev(4)).with_end(Rev(2));
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
        let mut closure = start.closure(edges);

        assert_eq!(
            closure.iter().collect::<Vec<_>>(),
            vec![Rev(3), Rev(2), Rev(1), Rev(0)]
        );
    }

    #[test]
    fn closure_limits_output_batches() {
        let start = Constant::<_, Forwards>::new(vec![Rev(2000)]);
        let edges =
            Constant::<_, Forwards>::new((1..=2000).map(|i| (Rev(i), Rev(i - 1))).collect());
        let mut closure = start.closure(edges);

        let mut batch = Batch::new(Vec::new());
        closure.next(&mut batch);
        assert_eq!(batch.data.len(), CLOSURE_BATCH_SIZE);
        assert_eq!(batch.data.first(), Some(&Rev(2000)));
        assert_eq!(batch.data.last(), Some(&Rev(977)));

        batch.data.clear();
        closure.next(&mut batch);
        assert_eq!(batch.data.len(), 2001 - CLOSURE_BATCH_SIZE);
        assert_eq!(batch.data.first(), Some(&Rev(976)));
        assert_eq!(batch.data.last(), Some(&Rev(0)));
    }

    #[test]
    fn reverse_flips_ordering() {
        let input = Constant::<_, Forwards>::new(vec![Rev(5), Rev(3), Rev(1)]);
        let mut reversed = input.reverse();

        assert_eq!(
            reversed.iter().collect::<Vec<_>>(),
            vec![Rev(1), Rev(3), Rev(5)]
        );
    }

    #[test]
    fn union_merges_sorted_inputs() {
        let left = Constant::<_, Forwards>::new(vec![Rev(5), Rev(3), Rev(1)]);
        let right = Constant::<_, Forwards>::new(vec![Rev(4), Rev(3), Rev(2)]);
        let mut union = left.union(right);

        assert_eq!(
            union.iter().collect::<Vec<_>>(),
            vec![Rev(5), Rev(4), Rev(3), Rev(2), Rev(1)]
        );
    }

    #[test]
    fn intersection_returns_common_items() {
        let left = Constant::<_, Forwards>::new(vec![Rev(5), Rev(4), Rev(3), Rev(1)]);
        let right = Constant::<_, Forwards>::new(vec![Rev(6), Rev(4), Rev(3), Rev(2)]);
        let mut intersection = left.intersection(right);

        assert_eq!(
            intersection.iter().collect::<Vec<_>>(),
            vec![Rev(4), Rev(3)]
        );
    }

    #[test]
    fn intersection_preserves_backwards_ordering() {
        let left = Constant::<_, Backwards>::new(vec![Rev(5), Rev(4), Rev(3), Rev(1)]);
        let right = Constant::<_, Backwards>::new(vec![Rev(6), Rev(4), Rev(3), Rev(2)]);
        let mut intersection = left.intersection(right);

        assert_eq!(
            intersection.iter().collect::<Vec<_>>(),
            vec![Rev(3), Rev(4)]
        );
    }

    #[test]
    fn difference_returns_left_items_missing_from_right() {
        let left = Constant::<_, Forwards>::new(vec![Rev(5), Rev(4), Rev(3), Rev(2), Rev(1)]);
        let right = Constant::<_, Forwards>::new(vec![Rev(6), Rev(4), Rev(2)]);
        let mut difference = left.difference(right);

        assert_eq!(
            difference.iter().collect::<Vec<_>>(),
            vec![Rev(5), Rev(3), Rev(1)]
        );
    }

    #[test]
    fn difference_preserves_backwards_ordering() {
        let left = Constant::<_, Backwards>::new(vec![Rev(5), Rev(4), Rev(3), Rev(2), Rev(1)]);
        let right = Constant::<_, Backwards>::new(vec![Rev(6), Rev(4), Rev(2)]);
        let mut difference = left.difference(right);

        assert_eq!(
            difference.iter().collect::<Vec<_>>(),
            vec![Rev(1), Rev(3), Rev(5)]
        );
    }

    #[test]
    fn filter_keeps_matching_items() {
        let input = Constant::<_, Forwards>::new(vec![Rev(5), Rev(4), Rev(3), Rev(2), Rev(1)]);
        let mut filter = input.filter(|rev: &Rev| rev.0 % 2 == 0);

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
        let mut map = input.map(|rev: Rev| rev.0);

        assert_eq!(map.iter().collect::<Vec<_>>(), vec![3, 2, 1]);
    }

    #[test]
    fn dagrange_returns_descendants_of_x_that_reach_y() {
        let edges = Index::<_, Backwards>::new(vec![
            (Rev(1), Rev(2)),
            (Rev(2), Rev(3)),
            (Rev(2), Rev(4)),
            (Rev(3), Rev(5)),
            (Rev(4), Rev(5)),
            (Rev(8), Rev(9)),
        ]);
        let ancestors = Index::<_, Forwards>::new(vec![
            (Rev(2), Rev(1)),
            (Rev(3), Rev(2)),
            (Rev(4), Rev(2)),
            (Rev(5), Rev(3)),
            (Rev(5), Rev(4)),
            (Rev(9), Rev(8)),
        ]);
        let x = Constant::<_, Forwards>::new(vec![Rev(2)]);
        let y = Constant::<_, Forwards>::new(vec![Rev(5)]);
        let mut range = x.range(y, &edges, &ancestors);

        assert_eq!(
            range.iter().collect::<Vec<_>>(),
            vec![Rev(2), Rev(3), Rev(4), Rev(5)]
        );
    }

    #[test]
    fn dagrange_accepts_any_input_ordering() {
        let edges = Index::<_, Backwards>::new(vec![
            (Rev(1), Rev(2)),
            (Rev(2), Rev(3)),
            (Rev(2), Rev(4)),
            (Rev(3), Rev(5)),
            (Rev(4), Rev(5)),
        ]);
        let ancestors = Index::<_, Forwards>::new(vec![
            (Rev(2), Rev(1)),
            (Rev(3), Rev(2)),
            (Rev(4), Rev(2)),
            (Rev(5), Rev(3)),
            (Rev(5), Rev(4)),
        ]);
        let x = Constant::<_, Backwards>::new(vec![Rev(2)]);
        let y = Constant::<_, Unordered>::new(vec![Rev(5)]);
        let mut range = x.range(y, &edges, &ancestors);

        assert_eq!(
            range.iter().collect::<Vec<_>>(),
            vec![Rev(2), Rev(3), Rev(4), Rev(5)]
        );
    }

    #[test]
    fn dagrange_preserves_backwards_order_with_multiple_starts() {
        let descendants =
            Index::<_, Backwards>::new(vec![(Rev(1), Rev(2)), (Rev(2), Rev(4)), (Rev(3), Rev(4))]);
        let ancestors =
            Index::<_, Forwards>::new(vec![(Rev(2), Rev(1)), (Rev(4), Rev(2)), (Rev(4), Rev(3))]);
        let x = Constant::<_, Backwards>::new(vec![Rev(1), Rev(3)]);
        let y = Constant::<_, Backwards>::new(vec![Rev(4)]);
        let mut range = x.range(y, &descendants, &ancestors);

        assert_eq!(
            range.iter().collect::<Vec<_>>(),
            vec![Rev(1), Rev(2), Rev(3), Rev(4)]
        );
    }

    #[test]
    fn dagrange_is_empty_when_y_is_not_below_x() {
        let edges = Index::<_, Backwards>::new(vec![(Rev(4), Rev(5)), (Rev(2), Rev(3))]);
        let ancestors = Index::<_, Forwards>::new(vec![(Rev(5), Rev(4)), (Rev(3), Rev(2))]);
        let x = Constant::<_, Forwards>::new(vec![Rev(5)]);
        let y = Constant::<_, Forwards>::new(vec![Rev(2)]);
        let mut range = x.range(y, &edges, &ancestors);

        assert_eq!(range.iter().collect::<Vec<_>>(), vec![]);
    }
}
