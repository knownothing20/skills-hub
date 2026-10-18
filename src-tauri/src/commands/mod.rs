pub mod auto_update_commands;
pub mod common;
pub mod install_commands;
pub mod onboarding_commands;
pub mod setting_commands;
pub mod skill_commands;
pub mod sync_commands;
pub mod tool_commands;

#[allow(unused_imports)]
pub use auto_update_commands::*;
#[allow(unused_imports)]
pub use common::*;
#[allow(unused_imports)]
pub use install_commands::*;
#[allow(unused_imports)]
pub use onboarding_commands::*;
#[allow(unused_imports)]
pub use setting_commands::*;
#[allow(unused_imports)]
pub use skill_commands::*;
#[allow(unused_imports)]
pub use sync_commands::*;
#[allow(unused_imports)]
pub use tool_commands::*;

#[cfg(test)]
#[path = "tests/commands.rs"]
mod tests;
