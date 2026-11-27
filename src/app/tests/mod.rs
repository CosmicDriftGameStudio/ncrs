// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod render_timing;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod perf_numbers;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod prompt_routing;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod prompt_end_to_end;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod abort_tests;
#[cfg(test)]
mod conflict_keys;
#[cfg(test)]
mod conflict_tests;
#[cfg(test)]
mod delete_tests;
#[cfg(test)]
mod folder_change_tests;
#[cfg(test)]
mod mouse_tagging;
#[cfg(test)]
mod progress_bar;
#[cfg(test)]
mod reload_keeps_place;
#[cfg(test)]
mod reload_order;
#[cfg(test)]
mod transfer_tests;
#[cfg(test)]
mod volume_menu_tests;
// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[cfg(test)]
mod copy_to_clipboard_tests;
#[cfg(test)]
mod scroll_tests;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod context_menu_tests;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod command_line_tests;
#[cfg(test)]
mod config_status_tests;
#[cfg(test)]
mod notice_tests;
#[cfg(test)]
mod path_field_tests;
#[cfg(test)]
mod type_ahead_tests;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod configured_keys_tests;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod search_tests;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod command_palette_tests;
#[cfg(test)]
mod filing_status_tests;
#[cfg(test)]
mod filing_tests;
#[cfg(test)]
mod hermes_apply_tests;
#[cfg(test)]
mod quick_open_tests;
#[cfg(test)]
mod theme_fetch_tests;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod ocr_models_tests;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod ai_model_tests;
#[cfg(test)]
mod archive_panel_tests;
#[cfg(test)]
mod archive_view_tests;
#[cfg(test)]
mod favourites_and_history_tests;
#[cfg(test)]
mod favourites_bar_tests;
#[cfg(test)]
mod quick_open_documents_tests;
#[cfg(test)]
mod session_state_tests;
#[cfg(test)]
mod settings_in_command_list_tests;
#[cfg(test)]
mod settings_tab_tests;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod about_tests;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod grant_tests;
#[cfg(test)]
mod open_file_tests;
#[cfg(test)]
mod remote_panel_tests;
#[cfg(test)]
mod repos_tests;
mod space_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod terminal_tests;
#[cfg(test)]
mod updates_tests;
#[cfg(test)]
mod window_focus_tests;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod clipboard_tests;
#[cfg(test)]
mod file_drop_tests;
#[cfg(test)]
mod folder_size_tests;
#[cfg(test)]
mod handoff_tests;
#[cfg(test)]
mod hermes_chat_ai_tests;
#[cfg(test)]
mod hermes_model_order_tests;
#[cfg(test)]
mod hermes_roles_and_off_tests;
#[cfg(test)]
mod receiver_stream_tests;

#[cfg(test)]
#[cfg(test)]
mod hermes_chat_areas_ui_tests;
#[cfg(test)]
mod hermes_chat_ui_tests;
mod hermes_flow_matrix_shots;
#[cfg(test)]
mod hermes_protection_tests;
mod hermes_real_model_tests;
#[cfg(test)]
mod hit_list_tests;
#[cfg(test)]
mod monitor_tests;
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod conflict_folder_ui_tests;
#[cfg(test)]
mod hermes_tab_tests;
#[cfg(test)]
mod hermes_tab_ui_tests;
#[cfg(test)]
mod one_concept_ui_tests;
#[cfg(test)]
mod open_file_ui_tests;
#[cfg(test)]
mod panel_columns_ui_tests;
#[cfg(test)]
mod remote_stages_ui_tests;
#[cfg(test)]
mod rename_tests;
#[cfg(test)]
mod search_folder_ui_tests;
#[cfg(test)]
mod settings_pages_tests;
#[cfg(test)]
mod settings_pages_ui_tests;
pub(in crate::app) mod support;
