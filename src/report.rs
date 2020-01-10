#[derive(Debug)]
pub struct Report<NoUpdate, Update, Error> {
    pub no_updates: Vec<NoUpdate>,
    pub compatible_updates: Vec<Update>,
    pub breaking_updates: Vec<Update>,
    pub failures: Vec<Error>,
}

impl<N, U, E> Report<N, U, E> {
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
