//! Retained UI: rebuild menus on transitions, update only changed HUD values.
use crate::runtime::GameSet;
use crate::sim::{EnemyKind, Phase, Sim, UpgradeKind};
use bevy::{prelude::*, ui::FocusPolicy, window::CursorOptions};

const INK: Color = Color::srgb(0.045, 0.063, 0.070);
const PANEL: Color = Color::srgba(0.075, 0.10, 0.11, 0.95);
const IVORY: Color = Color::srgb(0.94, 0.91, 0.82);
const MUTED: Color = Color::srgb(0.61, 0.69, 0.69);
const TEAL: Color = Color::srgb(0.30, 0.91, 0.82);
const AMBER: Color = Color::srgb(1.0, 0.70, 0.29);
const EDGE: Color = Color::srgb(0.23, 0.32, 0.34);

pub(crate) struct GameUiPlugin;
#[derive(Clone, Copy, Debug)]
pub(crate) enum UiAction {
    Start,
    Pause,
    Resume,
    Restart,
    Menu,
    Upgrade(usize),
    ToggleSound,
}
#[derive(Resource, Default)]
pub(crate) struct UiActions(pub Vec<UiAction>);
#[derive(Resource, Default)]
pub(crate) struct UiMeta {
    pub best_score: u32,
    pub best_wave: u32,
    pub runs: u32,
    pub sound_enabled: bool,
}
#[derive(Component)]
struct UiRoot;
#[derive(Component)]
struct BossPanel;
#[derive(Component)]
struct Reticle;
#[derive(Component)]
struct Action(UiAction);
#[derive(Component)]
struct ButtonTint(Color);
#[derive(Component)]
enum Readout {
    Health,
    Shield,
    Wave,
    Score,
    Xp,
    Dash,
    Message,
    Buffs,
    Boss,
}
#[derive(Component)]
enum Meter {
    Health,
    Xp,
    Boss,
}

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiActions>()
            .init_resource::<UiMeta>()
            .add_systems(
                Update,
                (resize_ui, rebuild, update_hud, update_boss, update_reticle)
                    .chain()
                    .in_set(GameSet::Present),
            )
            .add_systems(Update, buttons.before(GameSet::Input));
    }
}
fn label(text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: size.into(),
            ..default()
        },
        TextColor(color),
    )
}
fn column(gap: f32) -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(gap),
        ..default()
    }
}
fn row(gap: f32) -> Node {
    Node {
        column_gap: Val::Px(gap),
        align_items: AlignItems::Center,
        ..default()
    }
}
fn button(parent: &mut ChildSpawnerCommands, text: &str, action: UiAction, accent: Color) {
    parent
        .spawn((
            Button,
            Action(action),
            ButtonTint(accent),
            BackgroundColor(PANEL),
            BorderColor::all(accent),
            Node {
                min_height: Val::Px(46.0),
                padding: UiRect::axes(Val::Px(22.0), Val::Px(12.0)),
                border: UiRect::all(Val::Px(1.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn(label(text, 18.0, accent));
        });
}
fn resize_ui(windows: Query<&Window>, mut scale: ResMut<UiScale>) {
    if let Ok(window) = windows.single() {
        let desired = (window.width() / 1280.0)
            .min(window.height() / 720.0)
            .clamp(0.5, 1.5);
        if (scale.0 - desired).abs() > 0.001 {
            scale.0 = desired;
        }
    }
}
fn rebuild(
    mut commands: Commands,
    sim: Res<Sim>,
    meta: Res<UiMeta>,
    roots: Query<Entity, With<UiRoot>>,
    mut last: Local<String>,
) {
    let phase = match sim.phase {
        Phase::Title => "title",
        Phase::Playing => "playing",
        Phase::Upgrade => "upgrade",
        Phase::Paused => "paused",
        Phase::Victory => "victory",
        Phase::Defeat => "defeat",
    };
    let choices = if matches!(sim.phase, Phase::Upgrade) {
        sim.choices
            .iter()
            .map(|kind| format!("{}:{}", kind.title(), sim.upgrade_rank(*kind)))
            .collect::<Vec<_>>()
            .join("|")
    } else {
        String::new()
    };
    let signature = format!(
        "{phase}|{choices}|{}|{}|{}|{}",
        meta.sound_enabled, meta.best_score, meta.best_wave, meta.runs
    );
    if *last == signature {
        return;
    }
    *last = signature;
    for entity in &roots {
        commands.entity(entity).despawn();
    }
    commands
        .spawn((
            UiRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
        ))
        .with_children(|root| {
            if matches!(sim.phase, Phase::Playing | Phase::Upgrade | Phase::Paused) {
                hud(root);
            }
            match sim.phase {
                Phase::Title => title(root, &meta, sim.max_waves),
                Phase::Upgrade => upgrade(root, &sim),
                Phase::Paused => pause(root, &sim, &meta),
                Phase::Victory | Phase::Defeat => recap(root, &sim, &meta),
                Phase::Playing => (),
            }
        });
}
fn meter(parent: &mut ChildSpawnerCommands, kind: Meter, color: Color, height: f32) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(height),
                ..default()
            },
            BackgroundColor(EDGE),
        ))
        .with_children(|p| {
            p.spawn((
                kind,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(color),
            ));
        });
}
fn hud(root: &mut ChildSpawnerCommands) {
    root.spawn((
        BossPanel,
        FocusPolicy::Pass,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(104.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            display: Display::None,
            ..default()
        },
    ))
    .with_children(|p| {
        p.spawn((
            Node {
                width: Val::Px(490.0),
                padding: UiRect::axes(Val::Px(18.0), Val::Px(12.0)),
                ..column(9.0)
            },
            BackgroundColor(PANEL),
            FocusPolicy::Pass,
        ))
        .with_children(|p| {
            p.spawn((
                Readout::Boss,
                label("", 17.0, AMBER),
                TextLayout::no_wrap(),
                FocusPolicy::Pass,
            ));
            meter(p, Meter::Boss, AMBER, 7.0);
        });
    });
    root.spawn((
        Reticle,
        FocusPolicy::Pass,
        GlobalZIndex(5),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(26.0),
            height: Val::Px(26.0),
            display: Display::None,
            ..default()
        },
    ))
    .with_children(|p| {
        for (left, top, width, height) in [
            (12.0, 0.0, 2.0, 7.0),
            (12.0, 19.0, 2.0, 7.0),
            (0.0, 12.0, 7.0, 2.0),
            (19.0, 12.0, 7.0, 2.0),
        ] {
            p.spawn((
                FocusPolicy::Pass,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(left - 1.0),
                    top: Val::Px(top - 1.0),
                    width: Val::Px(width + 2.0),
                    height: Val::Px(height + 2.0),
                    ..default()
                },
                BackgroundColor(INK),
            ))
            .with_children(|p| {
                p.spawn((
                    FocusPolicy::Pass,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(1.0),
                        top: Val::Px(1.0),
                        width: Val::Px(width),
                        height: Val::Px(height),
                        ..default()
                    },
                    BackgroundColor(TEAL),
                ));
            });
        }
    });
    root.spawn(Node {
        position_type: PositionType::Absolute,
        top: Val::Px(22.0),
        left: Val::Px(28.0),
        right: Val::Px(28.0),
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Start,
        ..default()
    })
    .with_children(|p| {
        p.spawn((
            Node {
                width: Val::Px(252.0),
                padding: UiRect::all(Val::Px(14.0)),
                ..column(7.0)
            },
            BackgroundColor(PANEL),
        ))
        .with_children(|p| {
            p.spawn((
                Readout::Health,
                label("", 18.0, IVORY),
                TextLayout::no_wrap(),
            ));
            meter(p, Meter::Health, TEAL, 7.0);
            p.spawn((Readout::Shield, label("", 14.0, TEAL)));
        });
        p.spawn((
            Node {
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(26.0), Val::Px(12.0)),
                ..column(4.0)
            },
            BackgroundColor(PANEL),
        ))
        .with_children(|p| {
            p.spawn((Readout::Wave, label("", 23.0, IVORY)));
        });
        p.spawn(Node {
            align_items: AlignItems::End,
            ..column(10.0)
        })
        .with_children(|p| {
            p.spawn((Readout::Score, label("", 18.0, IVORY)));
            button(p, "Pause  [Esc]", UiAction::Pause, MUTED);
        });
    });
    root.spawn(Node {
        position_type: PositionType::Absolute,
        bottom: Val::Px(22.0),
        left: Val::Px(28.0),
        right: Val::Px(28.0),
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::End,
        ..default()
    })
    .with_children(|p| {
        p.spawn((
            Node {
                width: Val::Px(252.0),
                padding: UiRect::all(Val::Px(14.0)),
                ..column(7.0)
            },
            BackgroundColor(PANEL),
        ))
        .with_children(|p| {
            p.spawn((Readout::Xp, label("", 16.0, AMBER)));
            meter(p, Meter::Xp, AMBER, 5.0);
        });
        p.spawn((
            label(
                "WASD move   /   Mouse aim + fire   /   Space dash",
                14.0,
                IVORY,
            ),
            BackgroundColor(PANEL),
            Node {
                padding: UiRect::all(Val::Px(10.0)),
                ..default()
            },
        ));
        p.spawn((
            Readout::Dash,
            label("", 18.0, TEAL),
            BackgroundColor(PANEL),
            Node {
                padding: UiRect::all(Val::Px(14.0)),
                ..default()
            },
        ));
    });
    root.spawn((
        Readout::Buffs,
        label("", 16.0, AMBER),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(28.0),
            bottom: Val::Px(89.0),
            ..default()
        },
    ));
    root.spawn((
        Readout::Message,
        label("", 24.0, AMBER),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(20.0),
            width: Val::Percent(100.0),
            ..default()
        },
    ));
}
fn overlay(root: &mut ChildSpawnerCommands, build: impl FnOnce(&mut ChildSpawnerCommands)) {
    root.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.025, 0.04, 0.045, 0.90)),
        GlobalZIndex(10),
    ))
    .with_children(build);
}
fn title(root: &mut ChildSpawnerCommands, meta: &UiMeta, max_waves: u32) {
    overlay(root, |p| {
        p.spawn(Node { width: Val::Px(1110.0), justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(44.0), ..default() }).with_children(|p| {
            p.spawn(Node { width: Val::Px(610.0), ..column(20.0) }).with_children(|p| {
                p.spawn(label("SCRAPLINE", 88.0, IVORY));
                p.spawn(label("// LAST SHIFT", 31.0, AMBER));
                p.spawn((label("The yard is waking up.\nMake sure you're the one who clocks out.", 24.0, IVORY), Node { margin: UiRect::vertical(Val::Px(16.0)), ..default() }));
                p.spawn(label(format!("Survive {max_waves} waves of rogue machines. Collect scrap,\nchoose your upgrades, and dismantle the final foreman."), 18.0, MUTED));
                p.spawn(Node { margin: UiRect::top(Val::Px(12.0)), ..row(12.0) }).with_children(|p| {
                    button(p, "Start shift  [Enter]", UiAction::Start, AMBER);
                    button(p, if meta.sound_enabled { "Sound on  [M]" } else { "Sound off  [M]" }, UiAction::ToggleSound, MUTED);
                });
                p.spawn(label(format!("Best haul  {:06}     Best wave  {}     Shifts  {}", meta.best_score, meta.best_wave, meta.runs), 15.0, MUTED));
            });
            p.spawn((Node { width: Val::Px(380.0), padding: UiRect::all(Val::Px(28.0)), border: UiRect::left(Val::Px(3.0)), ..column(23.0) }, BorderColor::all(TEAL), BackgroundColor(PANEL))).with_children(|p| {
                p.spawn(label("Yard survival guide", 25.0, IVORY));
                control(p, "WASD / Arrow keys", "Move. Keep a way out.");
                control(p, "Mouse + Left click", "Aim and hold to fire.");
                control(p, "Space", "Dash through danger.");
                control(p, "1 / 2 / 3", "Pick an upgrade for your build.");
                control(p, "Esc / P", "Pause your shift.");
                p.spawn(label("Grab glowing scrap to level up.\nFind repairs, shields, Overdrive,\nand magnets around the yard.", 16.0, MUTED));
            });
        });
    });
}
fn control(p: &mut ChildSpawnerCommands, key: &str, hint: &str) {
    p.spawn(column(4.0)).with_children(|p| {
        p.spawn(label(key, 18.0, TEAL));
        p.spawn(label(hint, 16.0, IVORY));
    });
}
fn upgrade(root: &mut ChildSpawnerCommands, sim: &Sim) {
    overlay(root, |p| {
        p.spawn(Node {
            width: Val::Px(1100.0),
            ..column(22.0)
        })
        .with_children(|p| {
            p.spawn(label(
                format!("Level {}  /  Make it yours", sim.level),
                38.0,
                IVORY,
            ));
            p.spawn(label(
                "Choose one permanent upgrade for this shift. The yard can wait.",
                19.0,
                MUTED,
            ));
            p.spawn(Node {
                column_gap: Val::Px(18.0),
                ..default()
            })
            .with_children(|p| {
                for (index, kind) in sim.choices.iter().enumerate() {
                    p.spawn((
                        Button,
                        Action(UiAction::Upgrade(index)),
                        ButtonTint(AMBER),
                        BackgroundColor(PANEL),
                        BorderColor::all(EDGE),
                        Node {
                            width: Val::Percent(33.33),
                            min_height: Val::Px(270.0),
                            padding: UiRect::all(Val::Px(26.0)),
                            border: UiRect::all(Val::Px(2.0)),
                            ..column(18.0)
                        },
                    ))
                    .with_children(|p| {
                        p.spawn(label(
                            format!("[{}]    Rank {}", index + 1, sim.upgrade_rank(*kind) + 1),
                            17.0,
                            AMBER,
                        ));
                        p.spawn(label(kind.title(), 28.0, IVORY));
                        p.spawn(label(kind.description(), 19.0, MUTED));
                        p.spawn(Node {
                            flex_grow: 1.0,
                            ..default()
                        });
                        p.spawn(label("Install upgrade", 16.0, AMBER));
                    });
                }
            });
            p.spawn(label("Click a card or press 1, 2, or 3.", 16.0, MUTED));
        });
    });
}
fn pause(root: &mut ChildSpawnerCommands, sim: &Sim, meta: &UiMeta) {
    overlay(root, |p| {
        p.spawn(Node {
            width: Val::Px(1030.0),
            column_gap: Val::Px(70.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|p| {
            p.spawn(Node {
                width: Val::Px(490.0),
                ..column(20.0)
            })
            .with_children(|p| {
                p.spawn(label("Shift on hold", 49.0, IVORY));
                p.spawn(label("Take a breath. Your scrap can wait.", 19.0, MUTED));
                button(p, "Resume  [Enter / Esc]", UiAction::Resume, TEAL);
                button(p, "Restart shift  [R]", UiAction::Restart, AMBER);
                button(
                    p,
                    if meta.sound_enabled {
                        "Sound on  [M]"
                    } else {
                        "Sound off  [M]"
                    },
                    UiAction::ToggleSound,
                    MUTED,
                );
                button(p, "Return to title", UiAction::Menu, MUTED);
            });
            p.spawn((
                Node {
                    width: Val::Px(440.0),
                    padding: UiRect::all(Val::Px(24.0)),
                    ..column(16.0)
                },
                BackgroundColor(PANEL),
            ))
            .with_children(|p| {
                p.spawn(label("Your build", 29.0, AMBER));
                p.spawn(label(
                    format!(
                        "Level {}  /  {} elapsed\n{:.0} damage  /  {:.1} rounds per second",
                        sim.level,
                        elapsed(sim.run_time),
                        sim.player.stats.damage,
                        sim.player.stats.fire_rate
                    ),
                    16.0,
                    IVORY,
                ));
                let installed = UpgradeKind::ALL
                    .iter()
                    .filter_map(|kind| {
                        let rank = sim.upgrade_rank(*kind);
                        (rank > 0).then(|| format!("{}  [{}]", kind.title(), rank))
                    })
                    .collect::<Vec<_>>();
                p.spawn(label(
                    if installed.is_empty() {
                        "No upgrades installed yet.\nCollect scrap to choose your first.".into()
                    } else {
                        installed.join("\n")
                    },
                    17.0,
                    MUTED,
                ));
            });
        });
    });
}
fn recap(root: &mut ChildSpawnerCommands, sim: &Sim, meta: &UiMeta) {
    let victory = matches!(sim.phase, Phase::Victory);
    overlay(root, |p| {
        p.spawn(Node {
            width: Val::Px(840.0),
            ..column(23.0)
        })
        .with_children(|p| {
            p.spawn(label(
                if victory {
                    "Clocked out. Yard cleared."
                } else {
                    "End of shift."
                },
                53.0,
                if victory { TEAL } else { IVORY },
            ));
            p.spawn(label(
                if victory {
                    "The foreman is scrap. The rest of the night is yours."
                } else {
                    "The machines got this round. Build a different way next shift."
                },
                21.0,
                MUTED,
            ));
            p.spawn((
                Node {
                    padding: UiRect::all(Val::Px(26.0)),
                    justify_content: JustifyContent::SpaceBetween,
                    ..row(28.0)
                },
                BackgroundColor(PANEL),
            ))
            .with_children(|p| {
                stat(p, "Salvage score", format!("{:06}", sim.score));
                stat(p, "Wave", format!("{} / {}", sim.wave, sim.max_waves));
                stat(p, "Machines down", sim.kills.to_string());
                stat(p, "Time", elapsed(sim.run_time));
            });
            p.spawn(label(
                format!(
                    "Reached level {}     /     Best haul {:06}",
                    sim.level,
                    meta.best_score.max(sim.score)
                ),
                18.0,
                AMBER,
            ));
            p.spawn(row(16.0)).with_children(|p| {
                button(p, "Another shift  [Enter / R]", UiAction::Restart, AMBER);
                button(p, "Return to title", UiAction::Menu, MUTED);
            });
        });
    });
}
fn stat(p: &mut ChildSpawnerCommands, title: &str, value: String) {
    p.spawn(column(9.0)).with_children(|p| {
        p.spawn(label(title, 15.0, MUTED));
        p.spawn(label(value, 30.0, IVORY));
    });
}
fn elapsed(seconds: f32) -> String {
    let seconds = seconds.max(0.0) as u32;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
fn update_hud(
    sim: Res<Sim>,
    mut texts: Query<(&Readout, &mut Text)>,
    mut meters: Query<(&Meter, &mut Node)>,
) {
    for (kind, mut text) in &mut texts {
        let value = match kind {
            Readout::Health => format!(
                "Integrity  {:.0} / {:.0}",
                sim.player.hp.max(0.0),
                sim.player.max_hp
            ),
            Readout::Shield => format!("Shield  {:.0}", sim.player.shield.max(0.0)),
            Readout::Wave => {
                if sim.wave_timer > 0.0 && sim.enemies.is_empty() {
                    format!(
                        "Wave {:02} / {:02}   |   Incoming {:.0}s",
                        sim.wave,
                        sim.max_waves,
                        sim.wave_timer.ceil()
                    )
                } else {
                    format!(
                        "Wave {:02} / {:02}   |   {} threats",
                        sim.wave,
                        sim.max_waves,
                        sim.enemies.len() as u32 + sim.wave_remaining
                    )
                }
            }
            Readout::Score => format!("{:06} scrap   /   {} down", sim.score, sim.kills),
            Readout::Xp => format!(
                "Level {}    {:.0} / {:.0} XP",
                sim.level, sim.xp, sim.xp_to_next
            ),
            Readout::Dash => {
                if sim.player.dash_cooldown <= 0.0 {
                    "Dash ready  [Space]".into()
                } else {
                    format!("Dash  {:.1}s", sim.player.dash_cooldown)
                }
            }
            Readout::Boss => sim
                .enemies
                .iter()
                .find(|enemy| enemy.kind == EnemyKind::Boss && enemy.hp > 0.0)
                .map_or_else(String::new, |boss| {
                    let phase = if boss.hp < boss.max_hp * 0.5 {
                        "PHASE II"
                    } else {
                        "PHASE I"
                    };
                    format!(
                        "THE FOREMAN   /   {phase}   /   {:.0}%",
                        (boss.hp / boss.max_hp.max(1.0) * 100.0).clamp(0.0, 100.0)
                    )
                }),
            Readout::Buffs => {
                let mut buffs = Vec::new();
                if sim.player.overdrive > 0.0 {
                    buffs.push(format!("Overdrive {:.0}s", sim.player.overdrive.ceil()));
                }
                if sim.player.magnet > 0.0 {
                    buffs.push(format!("Magnet {:.0}s", sim.player.magnet.ceil()));
                }
                buffs.join("   /   ")
            }
            Readout::Message => {
                if sim.phase == Phase::Playing && sim.message_timer > 0.0 {
                    sim.message.clone()
                } else {
                    String::new()
                }
            }
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    for (kind, mut node) in &mut meters {
        let percent = match kind {
            Meter::Health => sim.player.hp / sim.player.max_hp.max(1.0),
            Meter::Xp => sim.xp / sim.xp_to_next.max(1.0),
            Meter::Boss => sim
                .enemies
                .iter()
                .find(|enemy| enemy.kind == EnemyKind::Boss && enemy.hp > 0.0)
                .map_or(0.0, |boss| boss.hp / boss.max_hp.max(1.0)),
        }
        .clamp(0.0, 1.0)
            * 100.0;
        if node.width != Val::Percent(percent) {
            node.width = Val::Percent(percent);
        }
    }
}
fn update_boss(sim: Res<Sim>, mut panels: Query<&mut Node, With<BossPanel>>) {
    let visible = sim.phase == Phase::Playing
        && sim
            .enemies
            .iter()
            .any(|enemy| enemy.kind == EnemyKind::Boss && enemy.hp > 0.0);
    for mut node in &mut panels {
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}
fn update_reticle(
    sim: Res<Sim>,
    scale: Res<UiScale>,
    mut windows: Query<(&Window, &mut CursorOptions)>,
    mut reticles: Query<&mut Node, With<Reticle>>,
) {
    let Ok((window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let position = window
        .cursor_position()
        .filter(|_| sim.phase == Phase::Playing && window.focused);
    // Menus and an unfocused window always restore the native pointer.
    let native_visible = position.is_none();
    if cursor.visible != native_visible {
        cursor.visible = native_visible;
    }
    for mut node in &mut reticles {
        if let Some(position) = position {
            node.display = Display::Flex;
            node.left = Val::Px(position.x / scale.0 - 13.0);
            node.top = Val::Px(position.y / scale.0 - 13.0);
        } else if node.display != Display::None {
            node.display = Display::None;
        }
    }
}
fn buttons(
    mut query: Query<
        (
            &Interaction,
            &Action,
            &ButtonTint,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Changed<Interaction>,
    >,
    mut actions: ResMut<UiActions>,
) {
    for (interaction, action, tint, mut background, mut border) in &mut query {
        match interaction {
            Interaction::Pressed => {
                actions.0.push(action.0);
                background.0 = INK;
                *border = BorderColor::all(IVORY);
            }
            Interaction::Hovered => {
                background.0 = Color::srgb(0.16, 0.22, 0.23);
                *border = BorderColor::all(tint.0);
            }
            Interaction::None => {
                background.0 = PANEL;
                *border = BorderColor::all(EDGE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        let mut app = App::new();
        app.insert_resource(Sim::new(7))
            .init_resource::<UiScale>()
            .add_plugins(GameUiPlugin);
        app.update();
        app
    }
    #[test]
    fn title_click_emits_one_action_per_press() {
        let mut app = app();
        let button = app
            .world_mut()
            .query::<(Entity, &Action)>()
            .iter(app.world())
            .find_map(|(entity, action)| matches!(action.0, UiAction::Start).then_some(entity))
            .expect("title must contain a start button");
        app.world_mut()
            .entity_mut(button)
            .insert(Interaction::Pressed);
        app.update();
        assert!(matches!(
            app.world().resource::<UiActions>().0.as_slice(),
            [UiAction::Start]
        ));
        app.update();
        assert_eq!(app.world().resource::<UiActions>().0.len(), 1);
    }
    #[test]
    fn hud_updates_without_rebuilding_its_tree() {
        let mut app = app();
        app.world_mut().resource_mut::<Sim>().start_run();
        app.update();
        let root = app
            .world_mut()
            .query_filtered::<Entity, With<UiRoot>>()
            .single(app.world())
            .expect("playing HUD must have one UI root");
        app.world_mut().resource_mut::<Sim>().player.hp = 42.0;
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<UiRoot>>()
                .single(app.world())
                .expect("health update must retain one UI root"),
            root
        );
        let health = app
            .world_mut()
            .query::<(&Readout, &Text)>()
            .iter(app.world())
            .find_map(|(kind, text)| matches!(kind, Readout::Health).then_some(text.0.clone()))
            .expect("playing HUD must include health text");
        assert_eq!(health, "Integrity  42 / 100");
    }
    #[test]
    fn upgrade_choices_dispatch_the_selected_index_and_disappear_on_resume() {
        let mut app = app();
        {
            let mut sim = app.world_mut().resource_mut::<Sim>();
            sim.phase = Phase::Upgrade;
            sim.choices = vec![
                UpgradeKind::Damage,
                UpgradeKind::Multishot,
                UpgradeKind::MaxHealth,
            ];
        }
        app.update();
        let choices = app
            .world_mut()
            .query::<(Entity, &Action)>()
            .iter(app.world())
            .filter_map(|(entity, action)| match action.0 {
                UiAction::Upgrade(index) => Some((entity, index)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(choices.len(), 3);
        let selected = choices
            .iter()
            .find(|(_, index)| *index == 1)
            .expect("upgrade screen must include its second choice")
            .0;
        app.world_mut()
            .entity_mut(selected)
            .insert(Interaction::Pressed);
        app.update();
        assert!(matches!(
            app.world().resource::<UiActions>().0.last(),
            Some(UiAction::Upgrade(1))
        ));
        app.world_mut().resource_mut::<Sim>().phase = Phase::Playing;
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&Action>()
                .iter(app.world())
                .filter(|action| matches!(action.0, UiAction::Upgrade(_)))
                .count(),
            0
        );
    }
    #[test]
    fn boss_panel_tracks_health_phase_and_defeat() {
        let mut app = app();
        {
            let mut sim = app.world_mut().resource_mut::<Sim>();
            sim.start_run();
            sim.wave_timer = 0.0;
            sim.step(1.0 / 60.0, crate::sim::Input::default());
            let boss = &mut sim.enemies[0];
            boss.kind = EnemyKind::Boss;
            boss.max_hp = 1000.0;
            boss.hp = 400.0;
        }
        app.update();
        let panel = app
            .world_mut()
            .query_filtered::<&Node, With<BossPanel>>()
            .single(app.world())
            .expect("playing HUD must include the boss panel");
        assert_eq!(panel.display, Display::Flex);
        let boss_label = app
            .world_mut()
            .query::<(&Readout, &Text)>()
            .iter(app.world())
            .find_map(|(kind, text)| matches!(kind, Readout::Boss).then_some(text.0.clone()))
            .expect("boss panel must include its health label");
        assert!(boss_label.contains("PHASE II"));
        assert!(boss_label.contains("40%"));
        let width = app
            .world_mut()
            .query::<(&Meter, &Node)>()
            .iter(app.world())
            .find_map(|(kind, node)| matches!(kind, Meter::Boss).then_some(node.width))
            .expect("boss panel must include its health meter");
        assert_eq!(width, Val::Percent(40.0));
        app.world_mut().resource_mut::<Sim>().enemies.clear();
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<&Node, With<BossPanel>>()
                .single(app.world())
                .expect("HUD must retain the hidden boss panel after defeat")
                .display,
            Display::None
        );
    }
    #[test]
    fn reticle_scales_coordinates_and_restores_native_cursor_on_menu() {
        let mut app = app();
        let mut window = Window {
            resolution: (1920, 1080).into(),
            focused: true,
            ..default()
        };
        window.set_cursor_position(Some(Vec2::new(300.0, 150.0)));
        let window = app.world_mut().spawn(window).id();
        app.world_mut().resource_mut::<Sim>().start_run();
        app.update();
        let (node, focus) = app
            .world_mut()
            .query_filtered::<(&Node, &FocusPolicy), With<Reticle>>()
            .single(app.world())
            .expect("playing HUD must contain one reticle");
        assert_eq!(node.display, Display::Flex);
        assert_eq!(node.left, Val::Px(187.0));
        assert_eq!(node.top, Val::Px(87.0));
        assert_eq!(*focus, FocusPolicy::Pass);
        assert!(
            !app.world()
                .entity(window)
                .get::<CursorOptions>()
                .expect("Window must include cursor options while playing")
                .visible
        );
        app.world_mut().resource_mut::<Sim>().phase = Phase::Title;
        app.update();
        assert!(
            app.world()
                .entity(window)
                .get::<CursorOptions>()
                .expect("Window must retain cursor options on title")
                .visible
        );
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<Reticle>>()
                .iter(app.world())
                .count(),
            0
        );
    }
}
