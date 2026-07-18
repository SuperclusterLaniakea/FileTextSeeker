/// Search panel - search bar and filter controls

use eframe::egui::{self, Key};
use crate::file_seeker::types::SearchOptions;

/// Actions from the search panel
pub enum SearchAction {
    ExecuteSearch(String),
    ToggleOptions,
    ToggleView,
    Reindex,
    ClearHistory,
}

/// Render the search bar UI. Returns an action if triggered.
pub fn render_search_bar(
    ui: &mut egui::Ui,
    search_text: &mut String,
    options: &mut SearchOptions,
    search_history: &[String],
    show_history: &mut bool,
) -> Option<SearchAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        // Settings button
        if ui.button("⚙").clicked() {
            action = Some(SearchAction::ToggleOptions);
        }

        // Search input
        let search_response = ui.add_sized(
            [ui.available_width() - 90.0, 22.0],
            egui::TextEdit::singleline(search_text)
                .hint_text("Search Everything...")
        );

        // Handle Enter key
        if search_response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            action = Some(SearchAction::ExecuteSearch(search_text.clone()));
        }

        // Search button
        if ui.button("🔍 Search").clicked() {
            action = Some(SearchAction::ExecuteSearch(search_text.clone()));
        }

        // Filters dropdown
        ui.menu_button("Filters", |ui| {
            ui.checkbox(&mut options.match_case, "Match case");
            ui.checkbox(&mut options.match_whole_word, "Whole word");
            ui.checkbox(&mut options.match_path, "Match path");
            ui.checkbox(&mut options.regex, "Regex");
            ui.separator();

            if ui.button("Toggle View").clicked() {
                action = Some(SearchAction::ToggleView);
                ui.close_menu();
            }
            if ui.button("Reindex").clicked() {
                action = Some(SearchAction::Reindex);
                ui.close_menu();
            }
            if ui.button("Clear Search History").clicked() {
                action = Some(SearchAction::ClearHistory);
                ui.close_menu();
            }
        });

        // History dropdown
        if !search_history.is_empty() {
            ui.menu_button("🕒", |ui| {
                for entry in search_history.iter().take(20) {
                    let entry_clone = entry.clone();
                    if ui.button(entry).clicked() {
                        *search_text = entry_clone.clone();
                        action = Some(SearchAction::ExecuteSearch(entry_clone));
                        ui.close_menu();
                    }
                }
                if search_history.len() > 20 {
                    ui.label("...");
                }
                if ui.button("Clear").clicked() {
                    action = Some(SearchAction::ClearHistory);
                }
            });
        }
    });

    action
}