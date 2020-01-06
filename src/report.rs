#[derive(Debug)]
pub struct Report<Key, Update, Error, Content = ()> {
    pub no_updates: Vec<(Key, Content)>,
    pub compatible_updates: Vec<(Key, Update)>,
    pub breaking_updates: Vec<(Key, Update)>,
    pub failures: Vec<(Key, Error)>,
}

impl<K, U, E, C> Report<K, U, E, C> {
    pub fn update_level(&self) -> UpdateLevel {
        use UpdateLevel::*;

        if !self.failures.is_empty() {
            Failure
        } else if !self.breaking_updates.is_empty() {
            BreakingUpdate
        } else if !self.compatible_updates.is_empty() {
            CompatibleUpdate
        } else {
            NoUpdates
        }
    }
}

pub enum UpdateLevel {
    NoUpdates,
    CompatibleUpdate,
    BreakingUpdate,
    Failure,
}
