use crate::app::App;

/// Handle the "connect" command — open the LLM provider picker.
///
/// This function shows the provider picker and automatically highlights
/// the provider that is currently set in the configuration.
///
/// # Arguments
/// - `app`: Mutable reference to the application state.
///
/// # Returns
/// Always returns `true`, signaling that the command was handled and the app should continue running.
pub fn handle(app: &mut App) -> bool {
    // Show the provider picker (popup / panel)
    app.show_provider_picker = true;

    // Find the index of the currently configured provider and pre-select it
    if let Some(pos) = app
        .provider_options
        .iter()
        .position(|p| p == &app.config.llm.provider)
    {
        app.provider_selected = pos;
    }

    // Return true to keep the application running
    true
}
