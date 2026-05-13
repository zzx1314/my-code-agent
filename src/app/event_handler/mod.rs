mod key_event;
mod mouse;
mod paste;

pub use key_event::handle_key_event;
pub use mouse::handle_mouse_event;
pub use paste::handle_paste_event;
