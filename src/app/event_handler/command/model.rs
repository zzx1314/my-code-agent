use crate::app::App;

pub(super) fn handle(app: &mut App) -> bool {
    // Open model picker, ensuring the model list corresponds to the current provider
    app.model_options = crate::app::get_model_options_for_provider(&app.config.llm.provider);
    app.show_model_picker = true;
    // Find the position of the current model in the options
    if let Some(current_model) = &app.config.llm.model {
        if let Some(pos) = app.model_options.iter().position(|m| m == current_model) {
            app.model_selected = pos;
        }
    }
    true
}
