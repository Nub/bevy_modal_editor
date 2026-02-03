//! Shared fuzzy search palette widget for consistent UI patterns.
//!
//! This module provides a reusable fuzzy search palette that can be used
//! for command palettes, object finders, component browsers, etc.

use bevy_egui::egui;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use super::theme::colors;

/// An item that can be displayed in a fuzzy palette
pub trait PaletteItem {
    /// The primary text used for display and fuzzy matching
    fn label(&self) -> &str;

    /// Optional keywords for additional fuzzy matching (lower priority)
    fn keywords(&self) -> &[String] {
        &[]
    }

    /// Optional category for grouping items
    fn category(&self) -> Option<&str> {
        None
    }

    /// Whether this item is selectable (grayed out if false)
    fn is_enabled(&self) -> bool {
        true
    }

    /// Optional suffix shown after the label (e.g., "(no default)")
    fn suffix(&self) -> Option<&str> {
        None
    }
}

/// Result of filtering items with scores
pub struct FilteredItem<'a, T> {
    /// Original index in the source list
    pub index: usize,
    /// Reference to the item
    pub item: &'a T,
    /// Fuzzy match score (higher is better)
    pub score: i64,
}

/// Filter items using fuzzy matching
pub fn fuzzy_filter<'a, T: PaletteItem>(items: &'a [T], query: &str) -> Vec<FilteredItem<'a, T>> {
    let matcher = SkimMatcherV2::default();

    if query.is_empty() {
        return items
            .iter()
            .enumerate()
            .map(|(index, item)| FilteredItem {
                index,
                item,
                score: 0,
            })
            .collect();
    }

    let mut results: Vec<FilteredItem<T>> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            // Match against label first (highest priority)
            if let Some(score) = matcher.fuzzy_match(item.label(), query) {
                return Some(FilteredItem {
                    index,
                    item,
                    score,
                });
            }

            // Match against keywords (lower priority)
            let best_keyword_score = item
                .keywords()
                .iter()
                .filter_map(|kw| matcher.fuzzy_match(kw, query))
                .max();

            if let Some(score) = best_keyword_score {
                return Some(FilteredItem {
                    index,
                    item,
                    score: score / 2, // Penalty for keyword-only match
                });
            }

            None
        })
        .collect();

    // Sort by score (higher is better)
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results
}

/// State for a fuzzy palette
#[derive(Default)]
pub struct PaletteState {
    pub query: String,
    pub selected_index: usize,
    pub just_opened: bool,
}

impl PaletteState {
    pub fn reset(&mut self) {
        self.query.clear();
        self.selected_index = 0;
        self.just_opened = true;
    }
}

/// Configuration for the palette appearance
pub struct PaletteConfig<'a> {
    /// Title shown in the mode indicator
    pub title: &'a str,
    /// Color for the title
    pub title_color: egui::Color32,
    /// Subtitle/description
    pub subtitle: &'a str,
    /// Hint text in the search input
    pub hint_text: &'a str,
    /// Action label for Enter key (e.g., "select", "add", "edit")
    pub action_label: &'a str,
    /// Window size
    pub size: [f32; 2],
    /// Whether to show categories
    pub show_categories: bool,
}

impl Default for PaletteConfig<'_> {
    fn default() -> Self {
        Self {
            title: "Search",
            title_color: colors::ACCENT_BLUE,
            subtitle: "",
            hint_text: "Type to search...",
            action_label: "select",
            size: [400.0, 300.0],
            show_categories: false,
        }
    }
}

/// Result of drawing the palette
pub enum PaletteResult<T> {
    /// User selected an item
    Selected(T),
    /// User closed the palette
    Closed,
    /// Palette is still open
    Open,
}

/// Draw a fuzzy search palette
///
/// Returns `PaletteResult::Selected(index)` when user selects an item,
/// `PaletteResult::Closed` when user closes the palette,
/// or `PaletteResult::Open` when the palette should stay open.
pub fn draw_fuzzy_palette<T: PaletteItem>(
    ctx: &egui::Context,
    state: &mut PaletteState,
    items: &[T],
    config: &PaletteConfig,
) -> PaletteResult<usize> {
    // Filter items
    let filtered = fuzzy_filter(items, &state.query);

    // Clamp selected index
    if !filtered.is_empty() {
        state.selected_index = state.selected_index.min(filtered.len() - 1);
    }

    // Check for keyboard input
    let enter_pressed = ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let escape_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let down_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown));
    let up_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowUp));

    // Handle Escape
    if escape_pressed {
        return PaletteResult::Closed;
    }

    // Handle Enter
    if enter_pressed && !filtered.is_empty() {
        if let Some(filtered_item) = filtered.get(state.selected_index) {
            if filtered_item.item.is_enabled() {
                return PaletteResult::Selected(filtered_item.index);
            }
        }
    }

    // Handle arrow keys
    if down_pressed && !filtered.is_empty() {
        state.selected_index = (state.selected_index + 1).min(filtered.len() - 1);
    }
    if up_pressed {
        state.selected_index = state.selected_index.saturating_sub(1);
    }

    let mut result = PaletteResult::Open;

    egui::Window::new(config.title)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size(config.size)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // Mode indicator
            if !config.title.is_empty() || !config.subtitle.is_empty() {
                ui.horizontal(|ui| {
                    if !config.title.is_empty() {
                        ui.label(
                            egui::RichText::new(config.title)
                                .small()
                                .strong()
                                .color(config.title_color),
                        );
                    }
                    if !config.subtitle.is_empty() {
                        ui.label(
                            egui::RichText::new(format!("- {}", config.subtitle))
                                .small()
                                .color(colors::TEXT_MUTED),
                        );
                    }
                });
                ui.add_space(4.0);
            }

            // Search input
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text(config.hint_text)
                    .desired_width(f32::INFINITY),
            );

            // Focus when just opened
            if state.just_opened {
                response.request_focus();
                state.just_opened = false;
            }

            ui.separator();

            // Item list
            let scroll_height = config.size[1] - 100.0; // Account for header and footer
            egui::ScrollArea::vertical()
                .max_height(scroll_height)
                .show(ui, |ui| {
                    if filtered.is_empty() {
                        ui.label(
                            egui::RichText::new("No matches found")
                                .color(colors::TEXT_MUTED)
                                .italics(),
                        );
                    } else {
                        let mut current_category: Option<&str> = None;

                        for (display_idx, filtered_item) in filtered.iter().enumerate() {
                            let item = filtered_item.item;

                            // Category header
                            if config.show_categories {
                                if let Some(category) = item.category() {
                                    if current_category != Some(category) {
                                        current_category = Some(category);
                                        ui.add_space(4.0);
                                        ui.label(
                                            egui::RichText::new(category)
                                                .small()
                                                .color(colors::TEXT_MUTED),
                                        );
                                    }
                                }
                            }

                            let is_selected = display_idx == state.selected_index;
                            let is_enabled = item.is_enabled();

                            let text_color = if !is_enabled {
                                colors::TEXT_MUTED
                            } else if is_selected {
                                colors::TEXT_PRIMARY
                            } else {
                                colors::TEXT_SECONDARY
                            };

                            // Build label text
                            let label_text = if let Some(suffix) = item.suffix() {
                                format!("{} {}", item.label(), suffix)
                            } else {
                                item.label().to_string()
                            };

                            let response = ui.selectable_label(
                                is_selected,
                                egui::RichText::new(&label_text).color(text_color),
                            );

                            if response.clicked() && is_enabled {
                                result = PaletteResult::Selected(filtered_item.index);
                            }

                            if is_selected {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                        }
                    }
                });

            ui.separator();

            // Help text
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Enter")
                        .small()
                        .strong()
                        .color(colors::ACCENT_BLUE),
                );
                ui.label(
                    egui::RichText::new(format!("to {}", config.action_label))
                        .small()
                        .color(colors::TEXT_MUTED),
                );
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("Esc")
                        .small()
                        .strong()
                        .color(colors::ACCENT_BLUE),
                );
                ui.label(
                    egui::RichText::new("to close")
                        .small()
                        .color(colors::TEXT_MUTED),
                );
            });
        });

    result
}

/// Simple wrapper for items that just have a label
pub struct SimpleItem {
    pub label: String,
}

impl PaletteItem for SimpleItem {
    fn label(&self) -> &str {
        &self.label
    }
}

/// Item with label and category
pub struct CategorizedItem {
    pub label: String,
    pub category: String,
    pub enabled: bool,
    pub suffix: Option<String>,
}

impl PaletteItem for CategorizedItem {
    fn label(&self) -> &str {
        &self.label
    }

    fn category(&self) -> Option<&str> {
        Some(&self.category)
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn suffix(&self) -> Option<&str> {
        self.suffix.as_deref()
    }
}

/// Item with label and keywords
pub struct KeywordItem {
    pub label: String,
    pub keywords: Vec<String>,
    pub category: Option<String>,
}

impl PaletteItem for KeywordItem {
    fn label(&self) -> &str {
        &self.label
    }

    fn keywords(&self) -> &[String] {
        &self.keywords
    }

    fn category(&self) -> Option<&str> {
        self.category.as_deref()
    }
}
