use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Tabs;

use crate::styles;

/// The top-level tab that names the visible screen.
///
/// Declaration order is the left-to-right order of the rendered tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TopTab {
    Active,
    Archived,
    Worklogs,
    Reports,
}

/// Renders the top tabs over the first row of `area`, inside its border.
pub(crate) fn render(frame: &mut Frame, area: Rect, selected: TopTab, focused: bool) {
    const LABELS: [&str; 4] = ["Active", "Archived", "Worklogs", "Reports"];
    let tabs = Tabs::new(LABELS)
        .select(selected as usize)
        .highlight_style(if focused {
            styles::selected()
        } else {
            styles::unfocused_tab()
        })
        .divider("│");
    frame.render_widget(
        tabs,
        Rect {
            x: area.x.saturating_add(1),
            y: area.y,
            width: area.width.saturating_sub(2),
            height: 1,
        },
    );
}
