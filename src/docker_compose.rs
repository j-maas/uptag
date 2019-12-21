use std::path::PathBuf;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct DockerCompose {
    pub services: IndexMap<String, Service>, // IndexMap preserves order.
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Service {
    pub build: PathBuf,
}
