pub mod model;
pub mod storage;

pub use model::{Config, RecoveryConfig};
pub use storage::{load_config, save_config, config_path};
