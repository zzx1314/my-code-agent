// Plan tracking and detection
pub mod detect;
pub mod tracker;

// Re-export primary types at the plan module level
pub use detect::detect_task_plan;
pub use tracker::{PlanConfirmationResult, PlanStepStatus, PlanTracker};
