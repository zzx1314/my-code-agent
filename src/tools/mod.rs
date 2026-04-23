pub mod code_search;
pub mod file_read;
pub mod file_update;
pub mod file_write;
pub mod shell_exec;

pub use code_search::CodeSearch;
pub use file_read::FileRead;
pub use file_update::FileUpdate;
pub use file_write::FileWrite;
pub use shell_exec::ShellExec;

use rig::tool::ToolDyn;

/// Returns all tools boxed as `Box<dyn ToolDyn>` for registration with the agent builder.
pub fn all_tools() -> Vec<Box<dyn ToolDyn>> {
    vec![
        Box::new(FileRead),
        Box::new(FileWrite),
        Box::new(FileUpdate),
        Box::new(ShellExec),
        Box::new(CodeSearch),
    ]
}
