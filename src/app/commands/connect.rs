use crate::app::App;

pub fn handle(app: &mut App) -> bool {
    // Open provider picker
    app.show_provider_picker = true;
    // Find the position of the current provider in the options
    if let Some(pos) = app
        .provider_options
        .iter()
        .position(|p| p == &app.config.llm.provider)
    {
        app.provider_selected = pos;
    }
    true
}
