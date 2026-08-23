//! The handful of controls every studio panel is built from.
//!
//! Three panels had each grown their own copy of "a row that lights up under
//! the cursor", and the three had already drifted apart in padding and in which
//! colour counts as idle. One constructor and one colouring system means a
//! control looks and feels the same wherever it appears, and a change to how
//! pressing feels is made once.

use bevy::{
    input::mouse::AccumulatedMouseScroll,
    input_focus::tab_navigation::TabIndex,
    prelude::*,
    text::{EditableText, TextCursorStyle},
    ui::RelativeCursorPosition,
};

use crate::{orbit::PointerOverUi, theme};

/// Pixels a notch of the wheel moves a list.
const SCROLL_STEP: f32 = 28.0;

/// Marks a node that should swallow camera drags.
///
/// The centre of the window is deliberately free of UI so that a drag there
/// orbits the camera; every panel that is *not* the viewport carries this so
/// the orbit system knows to stay out of the way.
///
/// It requires [`RelativeCursorPosition`] because that is what makes the test
/// reliable. `Interaction` is set only on the *topmost* node under the cursor,
/// so a column whose body covers it entirely never reports as hovered and a
/// drag on the panel orbits the camera behind it — which is exactly the bug
/// this fixes. `RelativeCursorPosition` is written for every node that carries
/// it whether or not something else is drawn on top, so one marker per panel
/// answers "is the pointer over UI" without a marker per widget inside it.
#[derive(Component)]
#[require(RelativeCursorPosition)]
pub struct UiPanel;

/// A column the wheel scrolls while the pointer is over it.
///
/// Two panels grew a scrolling list — the browser and the metadata column —
/// and only the first of them handled the wheel. One marker and one system
/// means a list scrolls wherever it appears, and the metadata panel stops
/// being a column you can see the top of and nothing else.
///
/// # Why "is the pointer over it" is geometric here too
///
/// This gated on the container's `Interaction`, and the wheel then worked only
/// over the ten pixels of padding around the rows — over a row itself, nothing
/// happened. A row is a [`Button`], which brings `FocusPolicy::Block`, and
/// Bevy's `ui_focus_system` stops its walk at the first blocking node under the
/// cursor and resets `Interaction::None` on everything above it, this container
/// included. [`RelativeCursorPosition`] is written for every node that carries
/// it whether or not something is drawn on top, which is the same property
/// [`UiPanel`] leans on.
#[derive(Component)]
#[require(RelativeCursorPosition)]
pub struct ScrollList;

/// A control that lights up under the cursor.
///
/// `idle` is the colour it returns to, which is not the same everywhere: a
/// button sits at [`theme::HOVER`] so it reads as a raised chip, while a list
/// row sits at [`theme::IDLE`] so a long list is not a wall of blocks.
#[derive(Component)]
pub struct Clickable {
    /// Background when the pointer is elsewhere.
    pub idle: Color,
}

impl Clickable {
    /// A control that rests at `idle`.
    #[must_use]
    pub const fn new(idle: Color) -> Self {
        Self { idle }
    }
}

/// Paint every [`Clickable`] according to what the pointer is doing to it.
///
/// Filtered on `Changed<Interaction>` because the colour only ever needs
/// writing on a transition, and an unfiltered write would dirty every button's
/// background every frame.
pub fn colour_clickables(
    mut controls: Query<(&Interaction, &Clickable, &mut BackgroundColor), Changed<Interaction>>,
) {
    for (interaction, clickable, mut background) in &mut controls {
        background.0 = match interaction {
            Interaction::Pressed => theme::PRESSED,
            Interaction::Hovered => theme::HOVER,
            Interaction::None => clickable.idle,
        };
    }
}

/// True while the pointer is over UI that should swallow drags.
pub fn track_pointer_over_ui(
    panels: Query<&RelativeCursorPosition, With<UiPanel>>,
    mut over: ResMut<PointerOverUi>,
) {
    over.0 = panels.iter().any(RelativeCursorPosition::cursor_over);
}

/// Wheel over a [`ScrollList`] scrolls it.
///
/// # Why the far end is clamped here
///
/// Bevy clamps only what it *draws* (`ComputedNode::scroll_position`) and never
/// writes that back, so wheeling past the bottom drove [`ScrollPosition`]
/// arbitrarily high: the list looked stuck until as many notches had been
/// unwound as had been spent overscrolling, and [`scroll_into_view`] — which
/// nudges relative to where the offset says the list is — stopped bringing
/// anything into view without saying so.
pub fn scroll_lists(
    scroll: Res<AccumulatedMouseScroll>,
    mut lists: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        With<ScrollList>,
    >,
) {
    if scroll.delta.y == 0.0 {
        return;
    }
    for (cursor, node, mut position) in &mut lists {
        // A list hidden with `Display::None` lays out at zero size and keeps
        // whatever cursor position it held when it was last on screen, so
        // without this it answers for a wheel over whatever replaced it.
        if node.size().y <= 0.0 || !cursor.cursor_over() {
            continue;
        }
        position.0.y = clamp_scroll(
            position.0.y - scroll.delta.y * SCROLL_STEP,
            node.content_size().y - node.size().y,
            node.inverse_scale_factor(),
        );
    }
}

/// Where a notch of the wheel leaves a list that can only scroll so far.
///
/// `overflow` is how much taller the content is than the box that holds it, in
/// the physical pixels the layout computes; [`ScrollPosition`] counts logical
/// ones, and `inverse_scale_factor` is the conversion — the same one
/// [`scroll_into_view`] makes at the end.
fn clamp_scroll(wanted: f32, overflow: f32, inverse_scale_factor: f32) -> f32 {
    wanted.clamp(0.0, (overflow * inverse_scale_factor).max(0.0))
}

/// Scroll `list` by the least that brings `row` fully into view.
///
/// Selecting an asset with the arrow keys is useless if the row stays below
/// the fold. Everything here is in the physical pixels the layout computes,
/// converted once at the end because [`ScrollPosition`] is in logical ones.
///
/// Written as a nudge — "how far off screen is it, in which direction" — rather
/// than as an absolute offset, because rows are not all the same height: the
/// browser interleaves section headers and group headings with its rows, and a
/// row index multiplied by an assumed height would drift down a long list.
pub fn scroll_into_view(
    list: (&ComputedNode, &UiGlobalTransform),
    row: (&ComputedNode, &UiGlobalTransform),
    scroll: &mut ScrollPosition,
) {
    let (list_node, list_at) = list;
    let (row_node, row_at) = row;
    if list_node.size().y <= 0.0 || row_node.size().y <= 0.0 {
        // Nothing has been laid out yet; the caller tries again next frame.
        return;
    }
    // The padded interior, which is what is actually visible: a row flush with
    // the frame's edge is still half hidden under the padding.
    let view_top = list_at.translation.y - list_node.size().y * 0.5 + list_node.padding.min_inset.y;
    let view_bottom =
        list_at.translation.y + list_node.size().y * 0.5 - list_node.padding.max_inset.y;
    let row_top = row_at.translation.y - row_node.size().y * 0.5;
    let row_bottom = row_at.translation.y + row_node.size().y * 0.5;

    let correction = if row_top < view_top {
        row_top - view_top
    } else if row_bottom > view_bottom {
        row_bottom - view_bottom
    } else {
        return;
    };
    scroll.0.y = (scroll.0.y + correction * list_node.inverse_scale_factor).max(0.0);
}

// ----------------------------------------------------------- constructors ---

/// An opaque column that hosts a panel's contents.
///
/// Opaque on purpose: the 3D stage carries on rendering behind it, and a
/// translucent panel makes a moving character read as UI noise.
#[must_use]
pub fn panel_frame(width: f32) -> impl Bundle {
    (
        Node {
            width: Val::Px(width),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            // A column is as wide as it says. Without this the centre slot
            // could take the width from the panels beside it when its plot
            // asked for more than the window had, and the metadata panel
            // left the window entirely.
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(theme::PANEL),
        UiPanel,
    )
}

/// A column that scrolls, wheel handling included.
///
/// `flex_basis: 0` with `min_height: 0` is what lets the list shrink below its
/// content height. Without both, the row sizes to its tallest child — a clip
/// list longer than the window — and shoves the transport bar off the screen.
///
/// [`ScrollList`] comes with it rather than being added per call site, because
/// a column that overflows and cannot be scrolled is not a design choice
/// anybody made — it is what happens when the marker is forgotten.
#[must_use]
pub fn scroll_column() -> impl Bundle {
    (
        ScrollList,
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(10.0)),
            row_gap: Val::Px(6.0),
            overflow: Overflow::scroll_y(),
            flex_grow: 1.0,
            flex_basis: Val::Px(0.0),
            min_height: Val::Px(0.0),
            ..default()
        },
    )
}

/// The heading over a group of controls: small, dim, all caps.
#[must_use]
pub fn section_header(text: impl Into<String>) -> impl Bundle {
    theme::label(text, theme::FONT_SMALL, theme::TEXT_DIM)
}

/// The same heading, clickable, over a group that can be folded away.
///
/// The caret is ASCII on purpose: the default font ships no ▸/▾, and a missing
/// glyph draws as a blank box — which reads as a broken header rather than a
/// closed one. The count beside it is what lets a folded group still say what
/// it holds; a fold that hides thirty rows and says nothing is a fold nobody
/// opens again.
///
/// `node` is taken for the reason [`button`] takes one — a section sits at the
/// panel's edge and a subgroup is indented under it — and what the header
/// *folds* is left to the caller to mark, because the widget layer has no
/// business knowing what a browser groups by.
#[must_use]
pub fn collapse_header(
    label: impl Into<String>,
    count: usize,
    collapsed: bool,
    node: Node,
) -> impl Bundle {
    (
        Button,
        Clickable::new(theme::IDLE),
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(4.0),
            ..node
        },
        BackgroundColor(theme::IDLE),
        children![
            (
                // Fixed width so the labels line up whichever way it points.
                Node {
                    width: Val::Px(7.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                TextLayout::no_wrap(),
                theme::label(
                    if collapsed { ">" } else { "v" },
                    theme::FONT_SMALL,
                    theme::TEXT_DIM
                ),
            ),
            (
                Node {
                    flex_shrink: 1.0,
                    min_width: Val::Px(0.0),
                    overflow: Overflow::clip_x(),
                    ..default()
                },
                TextLayout::no_wrap(),
                theme::label(label, theme::FONT_SMALL, theme::TEXT_DIM),
            ),
            (
                Node {
                    flex_shrink: 0.0,
                    ..default()
                },
                TextLayout::no_wrap(),
                theme::label(count.to_string(), theme::FONT_SMALL, theme::TEXT_DIM),
            ),
        ],
    )
}

/// One `key: value` line of read-only metadata.
#[must_use]
pub fn field(key: &str, value: &str) -> impl Bundle {
    theme::label(
        format!("{key}: {value}"),
        theme::FONT_SMALL,
        theme::TEXT_DIM,
    )
}

/// The padding a button chip uses.
#[must_use]
pub fn chip() -> Node {
    Node {
        padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
        ..default()
    }
}

/// A labelled button.
///
/// `node` is taken rather than baked in because the same control appears as a
/// fixed-width chip, as a centred stretch across a row, and as a bare label —
/// and a button whose shape cannot be stated at the call site grows a flag per
/// caller instead.
#[must_use]
pub fn button(label: impl Into<String>, size: f32, colour: Color, node: Node) -> impl Bundle {
    (
        Button,
        Clickable::new(theme::HOVER),
        node,
        BackgroundColor(theme::HOVER),
        children![theme::label(label, size, colour)],
    )
}

/// A full-width row in a selectable list, ready to be filled with children.
///
/// Rests transparent, so a list of thirty reads as text rather than as thirty
/// buttons; the highlight under the cursor is what says it is clickable. Laid
/// out as a row with the space pushed to the middle, so a name on the left and
/// a measurement on the right need no widths stated.
#[must_use]
pub fn list_row_frame(vertical_pad: f32) -> impl Bundle {
    (
        Button,
        Clickable::new(theme::IDLE),
        Node {
            width: Val::Percent(100.0),
            padding: UiRect::axes(Val::Px(6.0), Val::Px(vertical_pad)),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: Val::Px(6.0),
            ..default()
        },
        BackgroundColor(theme::IDLE),
    )
}

/// A list row that is only a label.
#[must_use]
pub fn list_row(label: impl Into<String>, size: f32, vertical_pad: f32) -> impl Bundle {
    (
        list_row_frame(vertical_pad),
        children![theme::label(label, size, theme::TEXT)],
    )
}

/// A word on a tinted background: a tag, a state, a marker.
///
/// Chips rather than a comma-separated line because tags are looked *for* —
/// the eye finds one in a row of blocks far faster than in prose.
#[must_use]
pub fn tag_chip(label: impl Into<String>) -> impl Bundle {
    (
        Node {
            padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(theme::HOVER),
        children![theme::label(label, theme::FONT_SMALL, theme::TEXT_DIM)],
    )
}

/// A [`tag_chip`] that is also a switch: click it to filter by that tag.
///
/// Separate from the plain chip rather than a flag on it, because the two say
/// different things: the metadata panel's chips are a *statement* about the
/// selected asset, and these are a control over the whole list. An active one
/// carries the accent on its text and its border and rests a shade brighter, so
/// that it reads as on even with the pointer nowhere near it — a filter you
/// cannot see is on is a filter that makes the library look empty.
#[must_use]
pub fn toggle_chip(label: impl Into<String>, active: bool) -> impl Bundle {
    let (text, edge, rest) = if active {
        (theme::ACCENT, theme::ACCENT, theme::PRESSED)
    } else {
        (theme::TEXT_DIM, theme::BORDER, theme::HOVER)
    };
    (
        Button,
        Clickable::new(rest),
        Node {
            padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(edge),
        BackgroundColor(rest),
        children![(
            TextLayout::no_wrap(),
            theme::label(label, theme::FONT_SMALL, text)
        )],
    )
}

/// A block of text that wraps rather than being cut off.
///
/// The opposite call to [`text_field`]'s, and deliberately so: a prompt is the
/// most useful thing in a record and reading half of it is reading none of it.
/// Safe here because the metadata panel is rebuilt only when the selection
/// moves, so nothing under the cursor shifts while it is being used.
#[must_use]
pub fn paragraph(text: impl Into<String>, size: f32, colour: Color) -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            ..default()
        },
        theme::label(text, size, colour),
    )
}

/// A single-line editable text field.
///
/// [`TextLayout::no_wrap`] is not cosmetic: a wrapped filter grows the field
/// and shifts everything below it every time something longer is typed, so
/// the controls move out from under the cursor.
#[must_use]
pub fn text_field(initial: &str, tab: i32, node: Node) -> impl Bundle {
    (
        EditableText::new(initial),
        TextCursorStyle::default(),
        TextLayout::no_wrap(),
        TextFont {
            font_size: theme::FONT.into(),
            ..default()
        },
        TextColor(theme::TEXT),
        TabIndex(tab),
        Node {
            padding: UiRect::axes(Val::Px(6.0), Val::Px(5.0)),
            border: UiRect::all(Val::Px(1.0)),
            overflow: Overflow::clip_x(),
            ..node
        },
        BorderColor::all(theme::BORDER),
        BackgroundColor(Color::srgb(0.13, 0.14, 0.17)),
    )
}

// --------------------------------------------------------------- updating ---

/// Write `value` into the text child of a control, if it changed.
///
/// Buttons carry their label as a child entity, so the whole panel's text is
/// reachable through one `&mut Text` query — two overlapping ones panic on the
/// aliasing conflict, however narrowly each is filtered.
pub fn set_child_text(texts: &mut Query<&mut Text>, children: Option<&Children>, value: &str) {
    let Some(children) = children else {
        return;
    };
    for child in children {
        if let Ok(mut text) = texts.get_mut(*child)
            && text.0 != value
        {
            value.clone_into(&mut text.0);
        }
    }
}

/// Write `value` into a node that *is* the text, if it changed.
pub fn set_text(texts: &mut Query<&mut Text>, entity: Option<Entity>, value: &str) {
    let Some(entity) = entity else {
        return;
    };
    if let Ok(mut text) = texts.get_mut(entity)
        && text.0 != value
    {
        value.clone_into(&mut text.0);
    }
}

/// Everything an [`EditableText`] widget currently shows, as one string.
#[must_use]
pub fn read_field(field: &EditableText) -> String {
    let mut text = String::new();
    for part in field.value() {
        text.push_str(part);
    }
    text
}

/// Keep a text field and the string behind it in step, in both directions.
///
/// One-way mirroring is not enough. Something else can set the string — a
/// filter cleared by a keyboard shortcut, say — and without a way to push
/// that into the widget the field keeps showing whatever was typed before
/// and, worse, overwrites the string again on the next frame.
///
/// Which side wins is decided by what changed since last frame: if the widget's
/// text moved, the user typed and `value` follows; otherwise something else set
/// `value` and the widget follows. `seen` is the caller's memory of last
/// frame's text and must be kept per field.
pub fn bind_field(field: &mut EditableText, value: &mut String, seen: &mut String) {
    let shown = read_field(field);
    if shown != *seen {
        seen.clone_from(&shown);
        *value = shown;
    } else if *value != shown {
        field.editor_mut().set_text(value);
        seen.clone_from(value);
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::mouse::MouseScrollUnit;

    use super::*;

    /// Bevy clamps the offset it *draws* and never writes that back, so the
    /// stored one used to climb without limit: ten notches past the bottom of
    /// the list meant ten notches that appeared to do nothing on the way back.
    #[test]
    fn a_list_cannot_be_scrolled_past_the_end_of_its_content() {
        let lands_at = |wanted: f32, overflow: f32, scale: f32, expected: f32| {
            let got = clamp_scroll(wanted, overflow, scale);
            assert!(
                (got - expected).abs() < f32::EPSILON,
                "wanted {wanted} of {overflow} at {scale}: got {got}, expected {expected}"
            );
        };

        // 900px of content in a 300px box: 600px of overflow, at scale 1.
        lands_at(1_000.0, 600.0, 1.0, 600.0);
        lands_at(250.0, 600.0, 1.0, 250.0);
        lands_at(-40.0, 600.0, 1.0, 0.0);
        // The overflow arrives in physical pixels and the offset is kept in
        // logical ones, so on a 2x display the same content is half as far to
        // scroll through.
        lands_at(1_000.0, 600.0, 0.5, 300.0);
        // Content that fits reports its overflow as a negative number, which
        // must read as "cannot scroll" rather than as a floor below zero.
        lands_at(80.0, -120.0, 1.0, 0.0);
    }

    /// The wheel over a *row* did nothing, and only the ten pixels of padding
    /// around the rows scrolled: a row is a button, and Bevy's focus walk stops
    /// at the first blocking node and clears `Interaction` on everything above
    /// it — including the list this used to ask. Asking the geometry instead
    /// costs one guard, for a list that is currently hidden: it lays out at
    /// zero size and keeps the cursor position it had when it was last on
    /// screen.
    #[test]
    fn the_wheel_scrolls_the_list_under_the_cursor_and_nothing_else() {
        let mut app = App::new();
        app.insert_resource(AccumulatedMouseScroll {
            unit: MouseScrollUnit::Line,
            delta: Vec2::new(0.0, -1.0),
        })
        .add_systems(Update, scroll_lists);

        let laid_out = |height: f32| ComputedNode {
            size: Vec2::new(240.0, height),
            content_size: Vec2::new(240.0, 900.0),
            ..ComputedNode::DEFAULT
        };
        let cursor = |cursor_over: bool| RelativeCursorPosition {
            cursor_over,
            normalized: None,
        };

        let under_cursor = app
            .world_mut()
            .spawn((
                ScrollList,
                cursor(true),
                laid_out(300.0),
                ScrollPosition::default(),
            ))
            .id();
        let elsewhere = app
            .world_mut()
            .spawn((
                ScrollList,
                cursor(false),
                laid_out(300.0),
                ScrollPosition::default(),
            ))
            .id();
        let hidden = app
            .world_mut()
            .spawn((
                ScrollList,
                cursor(true),
                laid_out(0.0),
                ScrollPosition::default(),
            ))
            .id();

        app.update();

        let offset =
            |app: &App, list: Entity| app.world().get::<ScrollPosition>(list).map(|s| s.0.y);
        assert_eq!(offset(&app, under_cursor), Some(SCROLL_STEP));
        assert_eq!(offset(&app, elsewhere), Some(0.0));
        assert_eq!(offset(&app, hidden), Some(0.0));
    }

    /// The orbit camera is off limits while the pointer is over a panel, and
    /// which *component* answers that question is the whole fix: `Interaction`
    /// is set only on the topmost node under the cursor, so a panel whose body
    /// covers it reported `None` and a drag on the panel spun the model behind
    /// it. `RelativeCursorPosition` is written for every node that carries it,
    /// on top or not.
    #[test]
    fn a_panel_under_the_cursor_swallows_the_drag() {
        let mut app = App::new();
        app.init_resource::<PointerOverUi>()
            .add_systems(Update, track_pointer_over_ui);

        // A panel the cursor is nowhere near, and — as when a scrolling list is
        // drawn over the column that hosts it — a second node on top of it.
        let panel = app
            .world_mut()
            .spawn((
                UiPanel,
                RelativeCursorPosition {
                    cursor_over: false,
                    normalized: None,
                },
            ))
            .id();
        app.world_mut().spawn(RelativeCursorPosition {
            cursor_over: true,
            normalized: None,
        });
        app.update();
        assert!(
            !app.world().resource::<PointerOverUi>().0,
            "a node that is not a panel must not block the camera"
        );

        app.world_mut()
            .entity_mut(panel)
            .insert(RelativeCursorPosition {
                cursor_over: true,
                normalized: None,
            });
        app.update();
        assert!(app.world().resource::<PointerOverUi>().0);
    }
}
