//! The transport clock, and the keys that drive it.
//!
//! # Playback is always a seek
//!
//! Even when playing, the studio owns the clock and drives the player with
//! `set_seek_time` + `pause` every frame rather than letting Bevy advance it.
//! That makes playing and scrubbing the same code path — so the frame you pause
//! on is exactly the frame you were watching — and it keeps this resource's
//! notion of "current time" authoritative for the transport bar.

use bevy::{animation::AnimationClip, input_focus::InputFocus, prelude::*, text::EditableText};

use crate::studio::{
    library::{AssetKey, ClipLibrary, LibraryView, Selection},
    rig::Rig,
};

/// Where the playhead is, and what it is doing.
#[derive(Resource)]
pub struct Playback {
    /// Playhead position, seconds.
    pub time: f32,
    /// Length of the selected clip, seconds.
    pub duration: f32,
    /// Whether the clock is running.
    pub playing: bool,
    /// Whether the playhead wraps at the end instead of stopping.
    pub looping: bool,
    /// Clock rate, 1.0 being real time.
    pub speed: f32,
    /// Set when the player must be re-pointed: a new clip, or the same clip
    /// rebuilt underneath.
    pub dirty: bool,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            time: 0.0,
            duration: 0.0,
            // Opening on a still frame reads as a broken viewer, so the studio
            // starts playing whatever it opened.
            playing: true,
            looping: true,
            speed: 1.0,
            dirty: false,
        }
    }
}

impl Playback {
    /// Re-point the player at the current clip, for when its asset changed
    /// underneath.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
}

/// Rewind when the selection moves.
///
/// Keyed on which clip, not on a flag: [`advance`] consumes `dirty` in the same
/// frame it is set, so anything downstream that watched the flag would see it
/// already cleared and never notice the change at all.
pub(super) fn follow_selection(
    selection: Res<Selection>,
    mut playback: ResMut<Playback>,
    mut last: Local<Option<AssetKey>>,
) {
    let showing = selection.key().cloned();
    if *last == showing {
        return;
    }
    *last = showing;
    playback.time = 0.0;
    playback.dirty = true;
}

/// Step the clock, then park the player on the frame it names.
pub(super) fn advance(
    time: Res<Time>,
    mut playback: ResMut<Playback>,
    rig: Res<Rig>,
    library: Res<ClipLibrary>,
    selection: Res<Selection>,
    clips: Res<Assets<AnimationClip>>,
    mut players: Query<&mut AnimationPlayer>,
) {
    if !rig.is_ready() {
        return;
    }
    let Some(root) = rig.anim_root() else {
        return;
    };
    let Some(item) = library.selected(&selection) else {
        return;
    };
    let Some(node) = item.node else {
        return;
    };

    if let Some(clip) = clips.get(&item.handle) {
        playback.duration = clip.duration();
    }

    if playback.playing {
        let delta = time.delta_secs() * playback.speed;
        let next = playback.time + delta;
        playback.time = if next > playback.duration {
            if playback.looping {
                if playback.duration > 0.0 {
                    next % playback.duration
                } else {
                    0.0
                }
            } else {
                playback.playing = false;
                playback.duration
            }
        } else {
            next
        };
    }

    let Ok(mut player) = players.get_mut(root) else {
        return;
    };
    if playback.dirty {
        // Switching clips without stopping the old one would leave both active
        // and blend them at equal weight.
        player.stop_all();
        playback.dirty = false;
    }
    let seek_time = playback.time;
    let active = player.play(node);
    active.set_seek_time(seek_time);
    active.pause();
}

/// Space, arrows and L, unless something is being typed into.
pub(super) fn keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut playback: ResMut<Playback>,
    view: Res<LibraryView>,
    mut selection: ResMut<Selection>,
    focus: Option<Res<InputFocus>>,
    editing: Query<(), With<EditableText>>,
) {
    // Space toggles playback and the arrows step frames — all of which are also
    // things you type. Without this the library filter is unusable.
    if let Some(focus) = focus
        && focus.get().is_some_and(|e| editing.contains(e))
    {
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        playback.playing = !playback.playing;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        playback.looping = !playback.looping;
    }
    // Step by a 30 fps frame: fine enough to inspect a contact moment, coarse
    // enough that holding the key still moves.
    let step = 1.0 / 30.0;
    if keys.just_pressed(KeyCode::ArrowRight) {
        playback.playing = false;
        playback.time = (playback.time + step).min(playback.duration);
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        playback.playing = false;
        playback.time = (playback.time - step).max(0.0);
    }
    let delta = if keys.just_pressed(KeyCode::ArrowDown) {
        1
    } else if keys.just_pressed(KeyCode::ArrowUp) {
        -1
    } else {
        return;
    };
    // Through the rows the browser is showing, not through the whole library:
    // stepping onto an asset the filter has hidden selects something nobody
    // can see, and the panel then describes a row that is not on screen.
    if let Some(next) = view.step(selection.key(), delta) {
        selection.select(next);
    }
}

#[cfg(test)]
mod tests {
    use bevy::input_focus::FocusCause;

    use super::*;

    /// Space toggles playback and the arrows step frames. Both are also things
    /// you type, so a focused filter field has to swallow them — and a
    /// screenshot cannot show whether it does.
    #[test]
    fn a_focused_text_field_swallows_the_transport_keys() {
        let mut app = App::new();
        app.init_resource::<Playback>()
            .init_resource::<LibraryView>()
            .init_resource::<Selection>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .add_systems(Update, keyboard);
        app.world_mut().resource_mut::<Playback>().playing = true;

        let field = app.world_mut().spawn(EditableText::new("")).id();
        let elsewhere = app.world_mut().spawn_empty().id();

        // Focus on something that is not a text field: space still works.
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(elsewhere, FocusCause::Pressed);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        app.update();
        assert!(
            !app.world().resource::<Playback>().playing,
            "space should toggle playback when no field is focused"
        );

        // Focus on the filter: the same key must do nothing.
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Pressed);
        app.world_mut().resource_mut::<Playback>().playing = true;
        app.update();
        assert!(
            app.world().resource::<Playback>().playing,
            "space reached the transport while typing in the filter"
        );
    }
}
