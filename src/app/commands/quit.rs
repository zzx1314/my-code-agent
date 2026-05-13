use crate::app::App;

pub fn handle(app: &mut App) -> bool {
    app.should_exit = true;
    true
}
