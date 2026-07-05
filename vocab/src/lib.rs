#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rev(pub u32);

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
