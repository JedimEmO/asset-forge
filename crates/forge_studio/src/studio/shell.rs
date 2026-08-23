//! The window's furniture: the columns, the transport and the status line.
//!
//! # The shape, and why
//!
//! Left is the library column. Right is the metadata column. Between them is
//! *nothing at all*: a UI node there would swallow the drag that orbits the
//! camera, and the whole point of the window is turning the subject around to
//! look at it.
//!
//! The left column is one untabbed list: bodies, models, clips and sounds
//! are one browsable library, because the window has exactly one job. Every
//! panel is built once and only ever shown or hidden by flipping `Display`,
//! never despawned, so a control comes back exactly as it was left.

use bevy::{ecs::hierarchy::ChildSpawnerCommands, prelude::*, ui::RelativeCursorPosition};

use crate::{
    studio::{
        audio_view,
        library::{AssetKey, Selection},
        playback::Playback,
        widgets,
    },
    theme,
};

/// One line of [`theme::FONT_SMALL`], reserved whether or not it is used.
///
/// Fixed so the hint cannot resize anything by appearing: a status area that
/// grows pushes what is under it out from under the cursor.
const STATUS_HEIGHT: f32 = 18.0;
/// Height of the heading over the library column.
const HEADING_HEIGHT: f32 = 28.0;

/// The one-line hint under the viewport.
///
/// A resource rather than a string written straight into the label, because the
/// panel that knows what is worth saying (the metadata panel, which is what
/// discovers a clip binds to nothing) is not the panel that owns the text.
#[derive(Resource, Default)]
pub struct StatusLine(pub String);

// ---------------------------------------------------------------- markers ---

/// Where [`audio_view`] builds the waveform, over the stage.
///
/// Empty and collapsed for a clip: the middle of the window has to be free of
/// UI so a drag there orbits the camera.
#[derive(Component)]
pub struct CentreSlot;

/// The body of the library column, which the browser fills.
#[derive(Component)]
pub(super) struct LibraryPanel;
/// Metadata for whatever is selected.
#[derive(Component)]
pub(super) struct MetaPanel;
#[derive(Component)]
pub(super) struct PlayButton;
#[derive(Component)]
pub(super) struct LoopButton;
#[derive(Component)]
pub(super) struct SpeedButton;
#[derive(Component)]
pub(super) struct ScrubBar;
#[derive(Component)]
pub(super) struct ScrubFill;
#[derive(Component)]
pub(super) struct TimeLabel;
#[derive(Component)]
pub(super) struct StatusLabel;
/// The transport controls that only mean something for a clip.
#[derive(Component)]
pub(super) struct ClipCluster;
/// The transport controls that only mean something for a sound.
#[derive(Component)]
pub(super) struct AudioCluster;

// ------------------------------------------------------------------ build ---

/// Spawn the whole layout, once.
pub(crate) fn build(mut commands: Commands) {
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|root| {
            root.spawn(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                // Without these the row sizes to its tallest child — a clip
                // list longer than the window — and shoves the transport bar
                // off the bottom of the screen. flex_basis 0 plus min_height 0
                // is what lets a flex item shrink below its content height.
                flex_basis: Val::Px(0.0),
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|row| {
                row.spawn(widgets::panel_frame(theme::LEFT_PANEL_WIDTH))
                    .with_children(build_left_column);

                // The middle is empty for a clip — no UI node means nothing to
                // swallow the drag that orbits the camera — and becomes the
                // waveform for a sound, which has nothing to orbit.
                row.spawn((
                    CentreSlot,
                    Node {
                        display: Display::None,
                        ..default()
                    },
                ));

                row.spawn(widgets::panel_frame(theme::RIGHT_PANEL_WIDTH))
                    .with_children(|column| {
                        column.spawn((
                            MetaPanel,
                            BackgroundColor(theme::PANEL),
                            widgets::scroll_column(),
                        ));
                    });
            });

            build_transport(root);

            // Hint line, bottom-left over the viewport.
            root.spawn((
                StatusLabel,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(theme::LEFT_PANEL_WIDTH + 12.0),
                    bottom: Val::Px(theme::TRANSPORT_HEIGHT + 10.0),
                    height: Val::Px(STATUS_HEIGHT),
                    overflow: Overflow::clip(),
                    ..default()
                },
                TextLayout::no_wrap(),
                theme::label("loading...", theme::FONT_SMALL, theme::TEXT_DIM),
            ));
        });
}

fn build_left_column(column: &mut ChildSpawnerCommands) {
    column.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(HEADING_HEIGHT),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::left(Val::Px(10.0)),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(theme::BORDER),
        children![theme::label("LIBRARY", theme::FONT_SMALL, theme::ACCENT)],
    ));

    // The body needs no `UiPanel` of its own: the column it sits in carries
    // one, and that test is geometric rather than hover-based precisely so a
    // body covering its parent cannot hide the parent from it. It is filled
    // by the panel that owns it — the browser — rather than here.
    column.spawn((
        LibraryPanel,
        Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            flex_basis: Val::Px(0.0),
            min_height: Val::Px(0.0),
            ..default()
        },
    ));
}

fn build_transport(root: &mut ChildSpawnerCommands) {
    root.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(theme::TRANSPORT_HEIGHT),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(10.0),
            padding: UiRect::all(Val::Px(10.0)),
            border: UiRect::top(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(theme::PANEL),
        BorderColor::all(theme::BORDER),
        Interaction::default(),
        widgets::UiPanel,
    ))
    .with_children(|bar| {
        bar.spawn((
            PlayButton,
            widgets::button(
                "PAUSE",
                theme::FONT,
                theme::TEXT,
                Node {
                    width: Val::Px(64.0),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
            ),
        ));

        bar.spawn((
            ScrubBar,
            Button,
            RelativeCursorPosition::default(),
            Node {
                flex_grow: 1.0,
                height: Val::Px(16.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.16, 0.17, 0.20)),
        ))
        .with_child((
            ScrubFill,
            Node {
                width: Val::Percent(0.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(theme::ACCENT),
        ));

        bar.spawn((
            TimeLabel,
            TextLayout::no_wrap(),
            theme::label("0.00 / 0.00s", theme::FONT, theme::TEXT),
        ));

        // One transport, two contexts. Both clusters are built now and only
        // ever hidden, so switching between a clip and a sound cannot lose the
        // gain a bus was left at.
        bar.spawn((ClipCluster, cluster()))
            .with_children(|cluster| {
                cluster.spawn((
                    SpeedButton,
                    widgets::button("1.0x", theme::FONT, theme::TEXT, widgets::chip()),
                ));
                cluster.spawn((
                    LoopButton,
                    widgets::button("LOOP", theme::FONT, theme::ACCENT, widgets::chip()),
                ));
            });
        bar.spawn((AudioCluster, cluster()))
            .with_children(audio_view::build_bus_cluster);
    });
}

/// A group of transport controls shown for one kind of asset.
fn cluster() -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: Val::Px(4.0),
        ..default()
    }
}

// ------------------------------------------------------------------ input ---

/// Play, loop and speed.
pub(super) fn transport_buttons(
    play: Query<&Interaction, (With<PlayButton>, Changed<Interaction>)>,
    loops: Query<&Interaction, (With<LoopButton>, Changed<Interaction>)>,
    speeds: Query<&Interaction, (With<SpeedButton>, Changed<Interaction>)>,
    mut playback: ResMut<Playback>,
) {
    if play.iter().any(|i| *i == Interaction::Pressed) {
        playback.playing = !playback.playing;
    }
    if loops.iter().any(|i| *i == Interaction::Pressed) {
        playback.looping = !playback.looping;
    }
    if speeds.iter().any(|i| *i == Interaction::Pressed) {
        playback.speed = match playback.speed {
            s if s < 0.3 => 0.5,
            s if s < 0.6 => 1.0,
            s if s < 1.1 => 2.0,
            _ => 0.25,
        };
    }
}

/// Dragging the bar moves the playhead.
pub(super) fn scrub(
    bars: Query<(&Interaction, &RelativeCursorPosition), With<ScrubBar>>,
    mut playback: ResMut<Playback>,
) {
    for (interaction, cursor) in &bars {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(normalized) = cursor.normalized else {
            continue;
        };
        // normalized is centre-relative, -0.5..0.5 across the node.
        let fraction = (normalized.x + 0.5).clamp(0.0, 1.0);
        playback.time = fraction * playback.duration;
        playback.playing = false;
    }
}

// ---------------------------------------------------------------- refresh ---

/// Show the parts of the window that apply to what is selected.
///
/// A clip wants the stage and the loop/speed controls; a sound wants the
/// waveform and the bus gains and has nothing to orbit. Everything is built
/// once and only hidden, so switching kinds keeps each side's state — and
/// hiding the centre slot is what gives the drag back to the camera.
/// One query for every part that appears and disappears: three `&mut Node`
/// queries over overlapping sets would have to prove themselves disjoint, and
/// Bevy panics on the conflict rather than taking anyone's word for it.
type ContextualParts<'w, 's> = Query<
    'w,
    's,
    (&'static mut Node, Has<CentreSlot>, Has<AudioCluster>),
    Or<(With<CentreSlot>, With<ClipCluster>, With<AudioCluster>)>,
>;

pub(super) fn refresh_context(selection: Res<Selection>, mut parts: ContextualParts) {
    let audio = selection.key().is_some_and(AssetKey::is_audio);
    for (mut node, centre, gains) in &mut parts {
        // The loop button cannot loop a sound, so everything that is not the
        // waveform or the gains belongs to a clip.
        let wanted = if centre || gains { audio } else { !audio };
        let display = if wanted { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
}

/// Everything the transport bar and the status line say.
///
/// One `&mut Text` query for the lot: a second one, however narrowly filtered,
/// overlaps it and Bevy panics on the aliasing conflict.
pub(super) fn refresh_transport(
    playback: Res<Playback>,
    status: Res<StatusLine>,
    time_label: Query<Entity, With<TimeLabel>>,
    status_label: Query<Entity, With<StatusLabel>>,
    play_label: Query<&Children, With<PlayButton>>,
    loop_label: Query<&Children, With<LoopButton>>,
    speed_label: Query<&Children, With<SpeedButton>>,
    mut fills: Query<&mut Node, With<ScrubFill>>,
    mut texts: Query<&mut Text>,
) {
    for mut node in &mut fills {
        let fraction = if playback.duration > 0.0 {
            (playback.time / playback.duration).clamp(0.0, 1.0)
        } else {
            0.0
        };
        node.width = Val::Percent(fraction * 100.0);
    }

    widgets::set_text(
        &mut texts,
        time_label.single().ok(),
        &format!("{:.2} / {:.2}s", playback.time, playback.duration),
    );
    if !status.0.is_empty() {
        widgets::set_text(&mut texts, status_label.single().ok(), &status.0);
    }
    widgets::set_child_text(
        &mut texts,
        play_label.single().ok(),
        if playback.playing { "PAUSE" } else { "PLAY" },
    );
    widgets::set_child_text(
        &mut texts,
        loop_label.single().ok(),
        if playback.looping { "LOOP" } else { "ONCE" },
    );
    widgets::set_child_text(
        &mut texts,
        speed_label.single().ok(),
        &format!("{:.2}x", playback.speed),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The context switch is the one piece of the transport a screenshot
    /// cannot check on its own: it only ever shows the cluster for the asset
    /// that happens to be selected. Both clusters and the centre slot are
    /// built up front and switching is a `Display` flip, so what is worth
    /// proving is that a clip shows the clip cluster alone and a sound shows
    /// the waveform and the gains alone.
    #[test]
    fn selecting_a_sound_swaps_the_clusters_and_shows_the_waveform() {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .add_systems(Startup, build)
            .add_systems(Update, refresh_context);
        app.update();

        let display_of = |app: &mut App, which: &str| -> Vec<Display> {
            let mut out = Vec::new();
            let mut query =
                app.world_mut()
                    .query::<(&Node, Has<CentreSlot>, Has<ClipCluster>, Has<AudioCluster>)>();
            for (node, centre, clip, audio) in query.iter(app.world()) {
                let hit = match which {
                    "centre" => centre,
                    "clip" => clip,
                    "audio" => audio,
                    _ => false,
                };
                if hit {
                    out.push(node.display);
                }
            }
            out
        };

        assert_eq!(display_of(&mut app, "centre"), vec![Display::None]);
        assert_eq!(display_of(&mut app, "clip"), vec![Display::Flex]);
        assert_eq!(display_of(&mut app, "audio"), vec![Display::None]);

        app.world_mut()
            .resource_mut::<Selection>()
            .select(AssetKey::audio("bark"));
        app.update();

        assert_eq!(display_of(&mut app, "centre"), vec![Display::Flex]);
        assert_eq!(display_of(&mut app, "clip"), vec![Display::None]);
        assert_eq!(display_of(&mut app, "audio"), vec![Display::Flex]);
    }
}
