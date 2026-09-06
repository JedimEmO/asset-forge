use crate::{
    Set,
    art::Ready,
    sim::{Game, MAGAZINE, Phase},
};
use bevy::prelude::*;

pub struct UiPlugin;
const INK: Color = Color::srgb(0.035, 0.05, 0.09);
const TEXT: Color = Color::srgb(0.9, 0.94, 0.98);
const DIM: Color = Color::srgb(0.55, 0.66, 0.76);
const CYAN: Color = Color::srgb(0.23, 0.84, 1.);
const VIOLET: Color = Color::srgb(0.7, 0.45, 1.);
#[derive(Resource)]
struct Fonts {
    regular: Handle<Font>,
    bold: Handle<Font>,
}
#[derive(Component)]
struct Hud;
#[derive(Component)]
struct Overlay;
#[derive(Component)]
struct DamageEdge;
#[derive(Component)]
struct Reticle(f32, f32);
#[derive(Component)]
enum Label {
    Distance,
    Sector,
    Shield,
    Ammo,
    WeaponState,
    Feedback,
    Charge,
    Energy,
    Combo,
    Hit,
    Health,
    Best,
}
#[derive(Component)]
enum Bar {
    Shield,
    Health,
    Charge,
    Energy,
    Progress,
    Magazine,
}
#[derive(Component, Clone, Copy)]
enum Action {
    Start,
    Resume,
    Restart,
}
#[derive(Resource, Default)]
struct UiState {
    phase: Option<Phase>,
}
fn text(
    value: impl Into<String>,
    size: f32,
    bold: bool,
    fonts: &Fonts,
    color: Color,
) -> impl Bundle {
    (
        Text::new(value),
        TextFont {
            font: if bold {
                fonts.bold.clone().into()
            } else {
                fonts.regular.clone().into()
            },
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}
fn column(gap: f32) -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(gap),
        ..default()
    }
}
fn fill(parent: &mut ChildSpawnerCommands, bar: Bar, color: Color, width: f32) {
    parent
        .spawn((
            Node {
                width: px(width),
                height: px(7),
                ..default()
            },
            BackgroundColor(Color::srgba(0.15, 0.22, 0.3, 0.8)),
        ))
        .with_children(|p| {
            p.spawn((
                bar,
                Node {
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(color),
            ));
        });
}
impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiState>()
            .add_systems(Startup, setup)
            .add_systems(Update, (menus, buttons, refresh).chain().in_set(Set::Ui));
    }
}
fn setup(mut commands: Commands, server: Res<AssetServer>) {
    let fonts = Fonts {
        regular: server.load("fonts/Lato-Regular.ttf"),
        bold: server.load("fonts/Lato-Bold.ttf"),
    };
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn(Node {
                position_type: PositionType::Absolute,
                left: px(42),
                top: px(30),
                ..column(4.)
            })
            .with_children(|p| {
                p.spawn(text("Distance", 15., false, &fonts, DIM));
                p.spawn((Label::Distance, text("0000 m", 36., true, &fonts, TEXT)));
            });
            p.spawn(Node {
                position_type: PositionType::Absolute,
                right: px(42),
                top: px(34),
                align_items: AlignItems::End,
                ..column(7.)
            })
            .with_children(|p| {
                p.spawn((Label::Sector, text("Causeway 01", 22., true, &fonts, TEXT)));
                p.spawn(text("Stay ahead of the collapse", 14., false, &fonts, DIM));
                fill(p, Bar::Progress, CYAN, 210.);
            });
            p.spawn(Node {
                position_type: PositionType::Absolute,
                left: px(42),
                bottom: px(34),
                ..column(9.)
            })
            .with_children(|p| {
                p.spawn((Label::Shield, text("Shield 100", 18., true, &fonts, TEXT)));
                fill(p, Bar::Shield, CYAN, 280.);
                p.spawn((Label::Health, text("Health 100", 13., false, &fonts, DIM)));
                fill(p, Bar::Health, TEXT, 170.);
            });
            p.spawn(Node {
                position_type: PositionType::Absolute,
                right: px(42),
                bottom: px(32),
                align_items: AlignItems::End,
                ..column(4.)
            })
            .with_children(|p| {
                p.spawn(text("ARC RIFLE / MAGAZINE", 17., true, &fonts, CYAN));
                p.spawn((Label::Ammo, text("24 / 24", 34., true, &fonts, TEXT)));
                p.spawn((Label::WeaponState, text("", 14., true, &fonts, CYAN)));
                fill(p, Bar::Magazine, CYAN, 160.);
                p.spawn(text(
                    "R reload    |    Hold RMB to focus",
                    13.,
                    false,
                    &fonts,
                    DIM,
                ));
            });
            p.spawn(Node {
                position_type: PositionType::Absolute,
                left: percent(43),
                bottom: px(32),
                align_items: AlignItems::Center,
                ..column(7.)
            })
            .with_children(|p| {
                p.spawn((
                    Label::Charge,
                    text("Q   Shockwave ready", 16., true, &fonts, VIOLET),
                ));
                fill(p, Bar::Charge, VIOLET, 200.);
                p.spawn((
                    Label::Energy,
                    text("E / MMB   Plasma blast ready", 15., true, &fonts, VIOLET),
                ));
                fill(p, Bar::Energy, VIOLET, 200.);
                p.spawn(text("Space jump    Shift dodge", 12., false, &fonts, DIM));
            });
            p.spawn(Node {
                position_type: PositionType::Absolute,
                left: percent(30),
                top: px(34),
                width: percent(40),
                align_items: AlignItems::Center,
                ..column(4.)
            })
            .with_children(|p| {
                p.spawn((Label::Combo, text("", 18., true, &fonts, CYAN)));
            });
            p.spawn((
                Label::Feedback,
                text("", 16., true, &fonts, CYAN),
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(43),
                    top: px(70),
                    ..default()
                },
            ));
            for right in [false, true] {
                p.spawn((
                    DamageEdge,
                    Node {
                        position_type: PositionType::Absolute,
                        left: if right { Val::Auto } else { px(0) },
                        right: if right { px(0) } else { Val::Auto },
                        width: px(12),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ));
            }
            // Four open reticle strokes leave the target visible at the actual camera ray.
            for (left, top, w, h) in [
                (-14., -1., 7., 2.),
                (7., -1., 7., 2.),
                (-1., -14., 2., 7.),
                (-1., 7., 2., 7.),
            ] {
                p.spawn((
                    Reticle(left, top),
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(50),
                        top: percent(50),
                        margin: UiRect {
                            left: px(left),
                            top: px(top),
                            ..default()
                        },
                        width: px(w),
                        height: px(h),
                        ..default()
                    },
                    BackgroundColor(TEXT),
                ));
            }
            p.spawn((
                Label::Hit,
                text("", 32., true, &fonts, CYAN),
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(50),
                    top: percent(50),
                    margin: UiRect {
                        left: px(-10),
                        top: px(-20),
                        ..default()
                    },
                    ..default()
                },
            ));
            p.spawn((
                text("Esc pause   M sound", 12., false, &fonts, DIM),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(42),
                    bottom: px(12),
                    ..default()
                },
            ));
        });
    commands.insert_resource(fonts);
}
fn menu_button(p: &mut ChildSpawnerCommands, fonts: &Fonts, label: &str, action: Action) {
    p.spawn((
        Button,
        action,
        Node {
            width: px(250),
            height: px(52),
            margin: UiRect::top(px(16)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(CYAN),
    ))
    .with_children(|p| {
        p.spawn(text(label, 18., true, fonts, INK));
    });
}
fn menus(
    mut commands: Commands,
    game: Res<Game>,
    fonts: Res<Fonts>,
    mut state: ResMut<UiState>,
    overlays: Query<Entity, With<Overlay>>,
    mut hud: Query<&mut Node, With<Hud>>,
) {
    for mut node in &mut hud {
        node.display = if game.phase == Phase::Playing {
            Display::Flex
        } else {
            Display::None
        };
    }
    if state.phase == Some(game.phase) {
        return;
    }
    state.phase = Some(game.phase);
    for entity in &overlays {
        commands.entity(entity).despawn();
    }
    if game.phase == Phase::Playing {
        return;
    }
    let is_title = game.phase == Phase::Title;
    commands.spawn((Overlay,Node{position_type:PositionType::Absolute,width:if is_title{px(610)}else{percent(100)},height:percent(100),padding:UiRect::all(px(62)),justify_content:JustifyContent::Center,..column(18.)},BackgroundColor(Color::srgba(0.02,0.035,0.065,if is_title{0.87}else{0.93})))).with_children(|p|{
  match game.phase{
   Phase::Title=>{
    p.spawn(text("Relay\nRun",100.,true,&fonts,TEXT));
    p.spawn(text("The station is falling.\nKeep moving.",26.,false,&fonts,CYAN));
    p.spawn(text("Sprint through an orbital firefight. Break their line,\ndodge the crossfire, and see how far you get.",17.,false,&fonts,DIM));
    menu_button(p,&fonts,"Start run  /  Enter",Action::Start);
    p.spawn((text("A / D    Strafe freely\nMouse    Aim and fire\nSpace    Jump\nShift    Dodge through incoming fire\nQ        Unleash a shockwave\nE / MMB  Fire a detonating plasma blast",16.,false,&fonts,TEXT),Node{margin:UiRect::top(px(12)),..default()}));
    p.spawn((Label::Best,text(format!("Best score  {}",game.best),14.,false,&fonts,DIM)));
   },
   Phase::Paused=>{p.spawn(text("Take a breath.",54.,true,&fonts,TEXT));p.spawn(text("Your run is paused.",22.,false,&fonts,DIM));menu_button(p,&fonts,"Resume  /  Esc",Action::Resume);menu_button(p,&fonts,"Restart run",Action::Restart);},
   Phase::Dead=>{p.spawn(text("Signal lost.",64.,true,&fonts,TEXT));p.spawn(text(format!("{} m travelled     {} hostiles eliminated",game.distance as u32,game.kills),24.,false,&fonts,DIM));p.spawn(text(format!("{} points",game.score+game.distance as u32),42.,true,&fonts,CYAN));p.spawn(text("Break line of sight to regenerate shields.\nChain 4 kills for Overdrive. Cyan crates restore ammo, shield and charge.",18.,false,&fonts,DIM));menu_button(p,&fonts,"Run again  /  Enter",Action::Restart);},_=>{}
  }
 });
}
fn buttons(
    mut interactions: Query<(&Interaction, &Action, &mut BackgroundColor), Changed<Interaction>>,
    mut game: ResMut<Game>,
    ready: Res<Ready>,
) {
    for (interaction, action, mut color) in &mut interactions {
        color.0 = if *interaction == Interaction::Hovered {
            Color::srgb(0.65, 0.95, 1.)
        } else {
            CYAN
        };
        if *interaction == Interaction::Pressed && ready.0 {
            match action {
                Action::Start | Action::Restart => game.start(),
                Action::Resume => game.phase = Phase::Playing,
            }
        }
    }
}
fn refresh(
    game: Res<Game>,
    ready: Res<Ready>,
    mut labels: Query<(&Label, &mut Text)>,
    mut bars: Query<(&Bar, &mut Node), Without<Reticle>>,
    mut reticles: Query<(&Reticle, &mut Node), Without<Bar>>,
    mut edges: Query<&mut BackgroundColor, With<DamageEdge>>,
) {
    for (label, mut content) in &mut labels {
        content.0 = match label {
            Label::Distance => format!("{:04} m", game.distance as u32),
            Label::Sector => format!("INTENSITY {} / Wave {:02}", game.intensity(), game.wave),
            Label::Shield => format!(
                "Shield {}{}",
                game.shield as u32,
                if game.shield < 100. && game.since_hit > 3.5 {
                    "  +"
                } else {
                    ""
                }
            ),
            Label::Health => format!("Health {}", game.health as u32),
            Label::Ammo => format!("{:02} / {}", game.ammo, MAGAZINE),
            Label::WeaponState => {
                if game.overdrive > 0. {
                    format!("OVERDRIVE  {:.1}s / FREE FIRE", game.overdrive)
                } else {
                    weapon_state(game.ammo, game.reload)
                }
            }
            Label::Feedback => {
                if game.overdrive > 0. {
                    "UNLIMITED AMMO / HOLD FIRE".into()
                } else if game
                    .effects
                    .iter()
                    .any(|f| f.kind == crate::sim::FxKind::Kill)
                {
                    "ELIMINATED".into()
                } else if game
                    .effects
                    .iter()
                    .any(|f| f.kind == crate::sim::FxKind::Pickup)
                {
                    "SUPPLIES / AMMO + SHIELDS + CHARGE".into()
                } else if game.hurt > 0. {
                    "TAKING FIRE".into()
                } else if game.nova_flash > 0. {
                    "SHOCKWAVE".into()
                } else if game.wave_banner > 0. {
                    game.encounter_name().into()
                } else {
                    String::new()
                }
            }
            Label::Energy => {
                if game.energy >= 100. {
                    "E / MMB   PLASMA BLAST READY".into()
                } else {
                    format!("PLASMA {}% / COLLECT VIOLET ENERGY", game.energy as u32)
                }
            }
            Label::Charge => {
                if game.charge >= 100. {
                    "Q   Shockwave ready".into()
                } else {
                    format!("Q   Shockwave  {}%", game.charge as u32)
                }
            }
            Label::Combo => {
                if game.multikill_banner > 0. {
                    format!(
                        "MULTIKILL ×{} / +{}",
                        game.multikill_size,
                        game.multikill_size * 75
                    )
                } else if game.overdrive > 0. {
                    format!("FREE FIRE / {:.1}s", game.overdrive)
                } else if game.combo >= 2 {
                    format!("CHAIN {} / {} TO OVERDRIVE", game.combo, 4 - game.combo % 4)
                } else {
                    String::new()
                }
            }
            Label::Hit => {
                if game.hitmarker > 0. {
                    "×".into()
                } else {
                    String::new()
                }
            }
            Label::Best => {
                if ready.0 {
                    format!("Best score  {}", game.best)
                } else {
                    "Loading equipment…".into()
                }
            }
        };
    }
    for (reticle, mut node) in &mut reticles {
        let kick = 1. + game.fire_cd / 0.105 * 0.7;
        node.margin.left = px(reticle.0 * kick);
        node.margin.top = px(reticle.1 * kick);
    }
    for mut color in &mut edges {
        color.0 = Color::srgba(1., 0.15, 0.04, (game.hurt * 1.5).min(0.85));
    }
    for (bar, mut node) in &mut bars {
        node.width = percent(match bar {
            Bar::Shield => game.shield,
            Bar::Health => game.health,
            Bar::Charge => game.charge,
            Bar::Energy => game.energy,
            Bar::Progress => ((game.time / 60.).clamp(0., 1.)) * 100.,
            Bar::Magazine => {
                if game.reload > 0. {
                    (1. - game.reload / 1.3) * 100.
                } else {
                    game.ammo as f32 / MAGAZINE as f32 * 100.
                }
            }
        });
    }
}

fn weapon_state(ammo: u32, reload: f32) -> String {
    if reload > 0. {
        format!("RELOADING  {:.1}s", reload)
    } else if ammo <= 5 {
        format!("{} ROUNDS  /  R RELOAD", ammo)
    } else {
        format!("{} ROUNDS", ammo)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weapon_status_explains_the_firing_lockout() {
        assert_eq!(weapon_state(0, 0.9), "RELOADING  0.9s");
        assert_eq!(weapon_state(3, 0.), "3 ROUNDS  /  R RELOAD");
        assert_eq!(weapon_state(24, 0.), "24 ROUNDS");
    }
}
