use crate::app::App;

pub(super) fn handle(app: &mut App) -> bool {
    app.should_exit = true;
    true
}
