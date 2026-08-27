//! Free-function views over `IntervalApp` (echo's architecture: one entity, no child
//! views). Each function takes `(&mut IntervalApp, &mut Window, &mut Context<IntervalApp>)`
//! and returns an element tree drawn from the app state.

mod replay_controls;
mod session_selector;
mod settings_menu;
mod side_panels;
mod stint_timeline;
mod timing_tower;
mod titlebar;
mod track_map;
pub mod ui;
mod window_frame;

pub use replay_controls::replay_controls;
pub use session_selector::session_selector;
pub use settings_menu::{SettingsUi, settings_menu};
pub use side_panels::side_panels;
pub use stint_timeline::stint_timeline;
pub use timing_tower::timing_tower;
pub use titlebar::titlebar;
pub use track_map::{MapAnimation, track_map};
pub use window_frame::{ClientCorners, client_corners, round_client_corners, window_frame};
