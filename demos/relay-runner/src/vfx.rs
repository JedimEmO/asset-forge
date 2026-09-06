//! Layered GPU emitters. Shared procedural opacity masks; no opaque effect meshes.
use crate::{
    Set,
    sim::{FxKind, Game, Phase},
};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use bevy_hanabi::prelude::*;
use std::collections::HashSet;

pub struct VfxPlugin;
#[derive(Resource, Default)]
pub struct VfxStats {
    pub emitters: usize,
    pub peak_emitters: usize,
    pub bursts: u64,
    pub dropped: u64,
}
#[derive(Clone)]
struct Layer {
    effect: Handle<EffectAsset>,
    texture: Handle<Image>,
    life: f32,
}
#[derive(Resource)]
struct Palette {
    muzzle: Layer,
    hit: Layer,
    kill: Layer,
    smoke: Layer,
    flash: Layer,
    plasma: Layer,
    ion_smoke: Layer,
    shock: Layer,
    nova: Layer,
    pickup: Layer,
    hurt: Layer,
    dash: Layer,
    bolt: Layer,
    orb: Layer,
    tracer: Layer,
    energy: Layer,
    charge: Layer,
}
#[derive(Component)]
struct Emitter {
    expires: f32,
    track: Option<(u8, u64)>,
}
#[derive(Resource, Default)]
struct Seen {
    effect: u64,
    time: f32,
}
const MAX_EMITTERS: usize = 240;
impl Plugin for VfxPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(HanabiPlugin)
            .init_resource::<Seen>()
            .init_resource::<VfxStats>()
            .add_systems(Startup, setup)
            .add_systems(Update, update.after(Set::Present));
    }
}
// Analytic shader masks sampled once into small textures: smooth glow, wispy smoke,
// and a shock-front annulus. Their edges go to zero; particles never show quads.
fn opacity(kind: u32, images: &mut Assets<Image>) -> Handle<Image> {
    let n = 128;
    let mut bytes = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let p = Vec2::new(
                (x as f32 + 0.5) / n as f32 * 2. - 1.,
                (y as f32 + 0.5) / n as f32 * 2. - 1.,
            );
            let r = p.length();
            let a = match kind {
                1 => {
                    let noise = (p.x * 13. + (p.y * 9.).sin()).sin()
                        * (p.y * 17. + (p.x * 11.).cos()).cos();
                    let wisps = (p.x * 29. + p.y * 21.).sin() * (p.y * 37. - p.x * 19.).sin();
                    (1. - r).max(0.).powf(1.8) * (0.55 + noise * 0.3 + wisps * 0.15)
                }
                2 => (-((r - 0.72) / 0.035).powi(2)).exp() * (1. - r).max(0.) * 3.,
                _ => (-r * r * 7.).exp() * (1. - r).max(0.).powi(2),
            }
            .clamp(0., 1.);
            let b = (a * 255.) as u8;
            bytes.extend_from_slice(&[b, b, b, 255]);
        }
    }
    images.add(Image::new(
        Extent3d {
            width: n as u32,
            height: n as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    ))
}
#[allow(clippy::too_many_arguments)]
fn layer(
    effects: &mut Assets<EffectAsset>,
    texture: &Handle<Image>,
    name: &str,
    count: u32,
    life: f32,
    speed: f32,
    size: Vec3,
    end_size: Vec3,
    color: Vec4,
    smoke: bool,
    streak: bool,
    rate: bool,
) -> Layer {
    let w = ExprWriter::new();
    let scroll = w.add_property("scroll", 8_f32.into());
    let beam = w.add_property("beam", Vec3::ZERO.into());
    let beam_pos = SetAttributeModifier::new(
        Attribute::POSITION,
        (w.attr(Attribute::POSITION) + w.prop(beam) * w.rand(ScalarType::Float)).expr(),
    );
    let init_pos = SetPositionSphereModifier {
        center: w.lit(Vec3::ZERO).expr(),
        radius: w.lit(if smoke { 0.3 } else { 0.035 }).expr(),
        dimension: ShapeDimension::Volume,
    };
    let init_vel = SetVelocitySphereModifier {
        center: w.lit(Vec3::ZERO).expr(),
        speed: w.lit(speed * 0.35).uniform(w.lit(speed)).expr(),
    };
    let age = SetAttributeModifier::new(Attribute::AGE, w.lit(0_f32).expr());
    let lifetime = SetAttributeModifier::new(
        Attribute::LIFETIME,
        w.lit(life * 0.65).uniform(w.lit(life)).expr(),
    );
    let drag = LinearDragModifier::new(w.lit(if smoke { 1.5 } else { 2.8 }).expr());
    let accel = AccelModifier::new(
        w.lit(
            Vec3::Y
                * if smoke {
                    0.8
                } else if streak {
                    -6.
                } else {
                    0.
                },
        )
        .expr(),
    );
    let scroll_update = SetAttributeModifier::new(
        Attribute::POSITION,
        (w.attr(Attribute::POSITION) + w.lit(Vec3::Z) * w.prop(scroll) * w.delta_time()).expr(),
    );
    let slot = w.lit(0_u32).expr();
    let mut module = w.finish();
    module.add_texture_slot("opacity");
    let mut colors = bevy_hanabi::Gradient::new();
    colors.add_key(0., if smoke { color.with_w(0.) } else { color });
    colors.add_key(0.12, color);
    colors.add_key(0.45, color * Vec4::new(0.65, 0.7, 0.8, 0.65));
    colors.add_key(1., color.with_w(0.));
    let mut sizes = bevy_hanabi::Gradient::new();
    sizes.add_key(0., size);
    sizes.add_key(0.22, size.lerp(end_size, 0.35));
    sizes.add_key(1., end_size);
    let asset = EffectAsset::new(
        if rate { 256 } else { count.max(4) },
        if rate {
            SpawnerSettings::rate((count as f32).into())
        } else {
            SpawnerSettings::once((count as f32).into())
        },
        module,
    )
    .with_name(name)
    .with_simulation_space(SimulationSpace::Global)
    .with_alpha_mode(if smoke {
        bevy_hanabi::AlphaMode::Blend
    } else {
        bevy_hanabi::AlphaMode::Add
    })
    .init(init_pos)
    .init(beam_pos)
    .init(init_vel)
    .init(age)
    .init(lifetime)
    .update(drag)
    .update(accel)
    .update(scroll_update)
    .render(ParticleTextureModifier {
        texture_slot: slot,
        sample_mapping: ImageSampleMapping::ModulateOpacityFromR,
    })
    .render(ColorOverLifetimeModifier::new(colors))
    .render(SizeOverLifetimeModifier {
        gradient: sizes,
        screen_space_size: false,
    })
    .render(OrientModifier::new(if streak {
        OrientMode::AlongVelocity
    } else {
        OrientMode::FaceCameraPosition
    }));
    Layer {
        effect: effects.add(asset),
        texture: texture.clone(),
        life,
    }
}
fn setup(
    mut c: Commands,
    mut effects: ResMut<Assets<EffectAsset>>,
    mut images: ResMut<Assets<Image>>,
) {
    let glow = opacity(0, &mut images);
    let smoke = opacity(1, &mut images);
    let ring = opacity(2, &mut images);
    let cyan = Vec4::new(3., 9., 14., 1.);
    let violet = Vec4::new(5., 2., 12., 1.);
    let gold = Vec4::new(12., 4., 0.5, 1.);
    let mut make =
        |tex: &Handle<Image>, name, count, life, speed, size, end, color, smoke, streak, rate| {
            layer(
                &mut effects,
                tex,
                name,
                count,
                life,
                speed,
                size,
                end,
                color,
                smoke,
                streak,
                rate,
            )
        };
    let p = Palette {
        muzzle: make(
            &glow,
            "rifle ion flash",
            12,
            0.09,
            2.,
            Vec3::splat(0.32),
            Vec3::splat(0.05),
            cyan,
            false,
            false,
            false,
        ),
        hit: make(
            &glow,
            "impact sparks",
            38,
            0.5,
            10.,
            Vec3::new(0.3, 0.035, 0.035),
            Vec3::splat(0.01),
            gold,
            false,
            true,
            false,
        ),
        kill: make(
            &glow,
            "shattering embers",
            140,
            1.05,
            16.,
            Vec3::new(0.42, 0.045, 0.045),
            Vec3::splat(0.012),
            gold,
            false,
            true,
            false,
        ),
        smoke: make(
            &smoke,
            "impact smoke",
            24,
            1.7,
            2.8,
            Vec3::splat(0.5),
            Vec3::splat(2.4),
            Vec4::new(0.18, 0.16, 0.19, 0.38),
            true,
            false,
            false,
        ),
        flash: make(
            &glow,
            "detonation core",
            3,
            0.18,
            0.3,
            Vec3::splat(3.5),
            Vec3::splat(0.4),
            Vec4::new(15., 12., 7., 0.7),
            false,
            false,
            false,
        ),
        plasma: make(
            &glow,
            "plasma filaments",
            320,
            0.95,
            24.,
            Vec3::new(0.55, 0.05, 0.05),
            Vec3::splat(0.012),
            cyan,
            false,
            true,
            false,
        ),
        ion_smoke: make(
            &smoke,
            "ion vapor",
            35,
            1.3,
            4.5,
            Vec3::splat(0.65),
            Vec3::splat(3.),
            Vec4::new(0.45, 0.22, 0.85, 0.5),
            true,
            false,
            false,
        ),
        shock: make(
            &ring,
            "plasma shock front",
            1,
            0.6,
            0.,
            Vec3::splat(0.3),
            Vec3::splat(19.4),
            violet.with_w(0.7),
            false,
            false,
            false,
        ),
        nova: make(
            &ring,
            "defensive nova front",
            1,
            0.65,
            0.,
            Vec3::splat(1.),
            Vec3::splat(36.),
            cyan.with_w(0.55),
            false,
            false,
            false,
        ),
        pickup: make(
            &glow,
            "supply motes",
            46,
            0.65,
            3.5,
            Vec3::splat(0.15),
            Vec3::splat(0.015),
            cyan,
            false,
            false,
            false,
        ),
        hurt: make(
            &glow,
            "shield discharge",
            25,
            0.27,
            3.,
            Vec3::new(0.22, 0.04, 0.04),
            Vec3::splat(0.01),
            Vec4::new(8., 0.3, 0.1, 1.),
            false,
            true,
            false,
        ),
        dash: make(
            &glow,
            "phase wake",
            65,
            0.45,
            3.,
            Vec3::splat(0.22),
            Vec3::splat(0.03),
            violet,
            false,
            false,
            false,
        ),
        bolt: make(
            &glow,
            "hostile bolt trail",
            110,
            0.18,
            0.3,
            Vec3::splat(0.22),
            Vec3::splat(0.025),
            gold,
            false,
            false,
            true,
        ),
        orb: make(
            &glow,
            "plasma comet trail",
            280,
            0.35,
            0.9,
            Vec3::splat(0.55),
            Vec3::splat(0.04),
            violet,
            false,
            false,
            true,
        ),
        energy: make(
            &glow,
            "energy shard motes",
            100,
            0.7,
            0.65,
            Vec3::splat(0.34),
            Vec3::splat(0.025),
            violet,
            false,
            false,
            true,
        ),
        charge: make(
            &glow,
            "drone charge sparks",
            80,
            0.3,
            0.8,
            Vec3::splat(0.25),
            Vec3::splat(0.035),
            gold,
            false,
            false,
            true,
        ),
        tracer: make(
            &glow,
            "rifle tracer",
            48,
            0.055,
            0.,
            Vec3::splat(0.1),
            Vec3::splat(0.015),
            cyan,
            false,
            false,
            false,
        ),
    };
    // Compile each GPU variant during the loading/menu period, away from the camera.
    for l in [
        &p.muzzle,
        &p.hit,
        &p.kill,
        &p.smoke,
        &p.flash,
        &p.plasma,
        &p.ion_smoke,
        &p.shock,
        &p.nova,
        &p.pickup,
        &p.hurt,
        &p.dash,
        &p.bolt,
        &p.orb,
        &p.tracer,
        &p.energy,
        &p.charge,
    ] {
        c.spawn((
            ParticleEffect::new(l.effect.clone()),
            EffectMaterial {
                images: vec![l.texture.clone()],
            },
            Transform::from_xyz(0., 0., -5000.),
            Emitter {
                expires: 0.,
                track: None,
            },
        ));
    }
    c.insert_resource(p);
}
fn emit(
    c: &mut Commands,
    l: &Layer,
    pos: Vec3,
    now: f32,
    track: Option<(u8, u64)>,
    stats: &mut VfxStats,
) {
    if stats.emitters >= MAX_EMITTERS {
        stats.dropped += 1;
        return;
    }
    c.spawn((
        ParticleEffect::new(l.effect.clone()),
        EffectMaterial {
            images: vec![l.texture.clone()],
        },
        Transform::from_translation(pos),
        Emitter {
            expires: now + l.life + 0.1,
            track,
        },
    ));
    stats.emitters += 1;
    stats.bursts += 1;
    stats.peak_emitters = stats.peak_emitters.max(stats.emitters);
}
type EmitterQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Emitter,
        &'static mut Transform,
        Option<&'static mut EffectProperties>,
        Option<&'static mut EffectSpawner>,
    ),
>;
fn update(
    mut c: Commands,
    g: Res<Game>,
    p: Res<Palette>,
    mut seen: ResMut<Seen>,
    mut stats: ResMut<VfxStats>,
    mut clock: ResMut<Time<EffectSimulation>>,
    mut emitters: EmitterQuery,
) {
    clock.set_relative_speed(if g.phase == Phase::Playing { 1. } else { 0. });
    let restarted = g.time < seen.time;
    seen.time = g.time;
    stats.emitters = emitters.iter().count();
    let mut tracked = HashSet::new();
    for (entity, mut e, mut t, properties, spawner) in &mut emitters {
        if restarted {
            c.entity(entity).despawn();
            continue;
        }
        if let Some(mut properties) = properties {
            properties.set("scroll", g.speed().into());
        }
        if let Some((plasma, id)) = e.track {
            tracked.insert((plasma, id));
            let pos = match plasma {
                1 => g.plasma.iter().find(|b| b.id == id).map(|b| b.pos),
                2 => g.energy_drops.iter().find(|b| b.id == id).map(|b| b.pos),
                3 => g
                    .enemies
                    .iter()
                    .find(|e| e.id == id && e.fire_in < 0.65)
                    .map(|e| e.pos + Vec3::new(0., 1.4, 0.4)),
                _ => g.bolts.iter().find(|b| b.id == id).map(|b| b.pos),
            };
            if let Some(pos) = pos {
                t.translation = pos;
                e.expires = g.time + 1.0;
            } else {
                if let Some(mut spawner) = spawner {
                    spawner.active = false;
                }
                e.track = None;
            }
        }
        if g.time > e.expires {
            c.entity(entity).despawn();
        }
    }
    if restarted {
        seen.effect = 0;
        stats.emitters = 0;
    }
    if g.phase != Phase::Playing {
        return;
    }
    for f in &g.effects {
        if f.id <= seen.effect {
            continue;
        }
        seen.effect = f.id;
        let layers: Vec<&Layer> = match f.kind {
            FxKind::Shot => vec![&p.muzzle],
            FxKind::Hit => vec![&p.hit],
            FxKind::Kill => vec![&p.kill, &p.smoke, &p.flash],
            FxKind::Nova => vec![&p.nova, &p.plasma],
            FxKind::Detonate => vec![&p.plasma, &p.flash, &p.ion_smoke, &p.shock],
            FxKind::Launch => vec![&p.muzzle, &p.dash],
            FxKind::Dash => vec![&p.dash],
            FxKind::Hurt => vec![&p.hurt],
            FxKind::Pickup => vec![&p.pickup],
            FxKind::Energy => vec![&p.dash],
        };
        for layer in layers {
            emit(&mut c, layer, f.from, g.time, None, &mut stats);
        }
        if f.kind == FxKind::Shot && stats.emitters < MAX_EMITTERS {
            let l = &p.tracer;
            c.spawn((
                ParticleEffect::new(l.effect.clone()),
                EffectMaterial {
                    images: vec![l.texture.clone()],
                },
                EffectProperties::default()
                    .with_properties([("beam".into(), (f.to - f.from).into())]),
                Transform::from_translation(f.from),
                Emitter {
                    expires: g.time + l.life + 0.1,
                    track: None,
                },
            ));
            stats.emitters += 1;
        }
    }

    for b in &g.bolts {
        if !tracked.contains(&(0, b.id)) {
            emit(&mut c, &p.bolt, b.pos, g.time, Some((0, b.id)), &mut stats);
        }
    }
    for b in &g.plasma {
        if !tracked.contains(&(1, b.id)) {
            emit(&mut c, &p.orb, b.pos, g.time, Some((1, b.id)), &mut stats);
        }
    }
    for b in &g.energy_drops {
        if !tracked.contains(&(2, b.id)) {
            emit(
                &mut c,
                &p.energy,
                b.pos,
                g.time,
                Some((2, b.id)),
                &mut stats,
            );
        }
    }
    for e in &g.enemies {
        if e.kind != crate::sim::EnemyKind::Rusher
            && e.fire_in < 0.65
            && e.pos.z > -46.
            && e.pos.z < -4.
            && !tracked.contains(&(3, e.id))
        {
            emit(
                &mut c,
                &p.charge,
                e.pos + Vec3::new(0., 1.4, 0.4),
                g.time,
                Some((3, e.id)),
                &mut stats,
            );
        }
    }
    stats.peak_emitters = stats.peak_emitters.max(stats.emitters);
}
