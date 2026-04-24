pub mod code_search;
pub mod file_delete;
pub mod file_read;
pub mod file_update;
pub mod file_write;
pub mod glob;
pub mod list_dir;
pub mod safety;
pub mod shell_exec;

pub use code_search::CodeSearch;
pub use file_delete::FileDelete;
pub use file_read::FileRead;
pub use file_update::FileUpdate;
pub use file_update::build_diff;
pub use file_write::FileWrite;
pub use glob::GlobSearch;
pub use list_dir::ListDir;
pub use safety::{is_dangerous_deletion, is_dangerous_shell_command, is_dangerous_snippet_deletion};
pub use shell_exec::ShellExec;

use rig::tool::ToolDyn;

/// Returns all tools boxed as `Box<dyn ToolDyn>` for registration with the agent builder.
pub fn all_tools() -> Vec<Box<dyn ToolDyn>> {
    vec![
        Box::new(FileRead),
        Box::new(FileWrite),
        Box::new(FileUpdate),
        Box::new(FileDelete),
        Box::new(ShellExec),
        Box::new(CodeSearch),
        Box::new(ListDir),
        Box::new(GlobSearch),
    ]
}
