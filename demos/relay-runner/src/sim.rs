//! Deterministic runner combat. World coordinates scroll past a stationary player Z.
use bevy::prelude::*;

pub const ROAD_HALF: f32 = 5.0;
pub const MAGAZINE: u32 = 24;
pub const BLAST_RADIUS: f32 = 7.;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Title,
    Playing,
    Paused,
    Dead,
}
#[derive(Default, Clone, Copy)]
pub struct Input {
    pub strafe: f32,
    pub fire: bool,
    pub jump: bool,
    pub dash: bool,
    pub nova: bool,
    pub secondary: bool,
    pub reload: bool,
    pub focus: bool,
    pub aim: Vec3,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnemyKind {
    Trooper,
    Rusher,
}
#[derive(Clone)]
pub struct Enemy {
    pub id: u64,
    pub pos: Vec3,
    pub hp: f32,
    pub max_hp: f32,
    pub kind: EnemyKind,
    pub fire_in: f32,
    pub flash: f32,
}
#[derive(Clone)]
pub struct Barrier {
    pub id: u64,
    pub x: f32,
    pub z: f32,
    pub width: f32,
    pub depth: f32,
}
#[derive(Clone)]
pub struct Bolt {
    pub id: u64,
    pub pos: Vec3,
    pub velocity: Vec3,
}
#[derive(Clone)]
pub struct Plasma {
    pub id: u64,
    pub pos: Vec3,
    pub velocity: Vec3,
    pub ttl: f32,
}
#[derive(Clone)]
pub struct Energy {
    pub id: u64,
    pub pos: Vec3,
}
#[derive(Clone)]
pub struct Pickup {
    pub id: u64,
    pub pos: Vec3,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FxKind {
    Shot,
    Hit,
    Kill,
    Nova,
    Dash,
    Hurt,
    Pickup,
    Launch,
    Detonate,
    Energy,
}
#[derive(Clone)]
pub struct Fx {
    pub id: u64,
    pub kind: FxKind,
    pub from: Vec3,
    pub to: Vec3,
    pub age: f32,
    pub life: f32,
}
#[derive(Resource)]
pub struct Game {
    pub phase: Phase,
    pub barrier_widths: [f32; 2],
    pub barrier_depths: [f32; 2],
    pub time: f32,
    pub distance: f32,
    pub x: f32,
    pub y: f32,
    pub vy: f32,
    pub health: f32,
    pub shield: f32,
    pub charge: f32,
    pub energy: f32,
    pub energy_collected: u32,
    pub blasts_fired: u32,
    pub detonations: u32,
    pub multikills: u32,
    pub multikill_size: u32,
    pub multikill_banner: f32,
    pub ammo: u32,
    pub reload: f32,
    pub fire_cd: f32,
    pub dash_cd: f32,
    pub dash_time: f32,
    pub dash_dir: f32,
    pub since_hit: f32,
    pub kills: u32,
    pub shots: u32,
    pub hits: u32,
    pub score: u32,
    pub supplies_collected: u32,
    pub combo: u32,
    pub combo_time: f32,
    pub overdrive: f32,
    pub overdrive_activations: u32,
    pub wave: u32,
    pub wave_banner: f32,
    pub impact: f32,
    pub aim: Vec3,
    pub focus: bool,
    pub hurt: f32,
    pub hitmarker: f32,
    pub nova_flash: f32,
    pub enemies: Vec<Enemy>,
    pub barriers: Vec<Barrier>,
    pub bolts: Vec<Bolt>,
    pub pickups: Vec<Pickup>,
    pub plasma: Vec<Plasma>,
    pub energy_drops: Vec<Energy>,
    pub effects: Vec<Fx>,
    pub best: u32,
    pub sound: bool,
    next_id: u64,
    rng: u64,
    spawn_in: f32,
    obstacle_in: f32,
}
impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}
impl Game {
    pub fn new() -> Self {
        Self {
            phase: Phase::Title,
            barrier_widths: [3., 2.4],
            barrier_depths: [1.2, 1.2],
            time: 0.,
            distance: 0.,
            x: 0.,
            y: 0.,
            vy: 0.,
            health: 100.,
            shield: 100.,
            charge: 100.,
            energy: 100.,
            energy_collected: 0,
            blasts_fired: 0,
            detonations: 0,
            multikills: 0,
            multikill_size: 0,
            multikill_banner: 0.,
            ammo: MAGAZINE,
            reload: 0.,
            fire_cd: 0.,
            dash_cd: 0.,
            dash_time: 0.,
            dash_dir: 1.,
            since_hit: 9.,
            kills: 0,
            shots: 0,
            hits: 0,
            score: 0,
            supplies_collected: 0,
            combo: 0,
            combo_time: 0.,
            overdrive: 0.,
            overdrive_activations: 0,
            wave: 0,
            wave_banner: 0.,
            impact: 0.,
            aim: Vec3::NEG_Z,
            focus: false,
            hurt: 0.,
            hitmarker: 0.,
            nova_flash: 0.,
            enemies: vec![],
            barriers: vec![],
            bolts: vec![],
            pickups: vec![],
            plasma: vec![],
            energy_drops: vec![],
            effects: vec![],
            best: 0,
            sound: true,
            next_id: 1,
            rng: 0xA9D34B,
            spawn_in: 1.5,
            obstacle_in: 3.0,
        }
    }
    pub fn start(&mut self) {
        let (best, sound, widths, depths) = (
            self.best,
            self.sound,
            self.barrier_widths,
            self.barrier_depths,
        );
        *self = Self::new();
        self.barrier_widths = widths;
        self.barrier_depths = depths;
        self.best = best;
        self.sound = sound;
        self.phase = Phase::Playing;
        self.add_enemy(-1.6, -28., EnemyKind::Trooper);
        self.add_enemy(2.8, -43., EnemyKind::Rusher);
    }
    /// Bounded harness only: keep twenty firing drones present while combat runs.
    pub fn crowded_fixture(&mut self) {
        self.spawn_in = 3600.;
        if self.enemies.len() != 20 || self.enemies.iter().any(|e| e.kind != EnemyKind::Trooper) {
            self.enemies.clear();
            for i in 0..20 {
                self.add_enemy(
                    (i % 5) as f32 * 1.8 - 3.6,
                    -10. - (i / 5) as f32 * 8.,
                    EnemyKind::Trooper,
                );
            }
        }
        for (i, e) in self.enemies.iter_mut().enumerate() {
            e.pos = Vec3::new((i % 5) as f32 * 1.8 - 3.6, 0., -10. - (i / 5) as f32 * 8.);
            e.hp = e.max_hp;
        }
        self.health = 100.;
        self.shield = 100.;
    }
    /// Bounded visual review of both prop silhouettes at gameplay distance.
    pub fn asset_fixture(&mut self) {
        self.spawn_in = 3600.;
        self.obstacle_in = 3600.;
        self.enemies.retain(|e| e.kind == EnemyKind::Trooper);
        if let Some(enemy) = self.enemies.first_mut() {
            enemy.pos = Vec3::new(0., 0., -24.);
        }
        self.barriers = vec![Barrier {
            id: 20001,
            x: 2.5,
            z: -12.,
            width: self.barrier_widths[1],
            depth: self.barrier_depths[1],
        }];
        self.pickups = vec![Pickup {
            id: 20000,
            pos: Vec3::new(-2.5, 0.8, -8.),
        }];
        self.health = 100.;
        self.shield = 100.;
    }
    pub fn burst_fixture(&mut self) {
        self.start();
        self.enemies.clear();
        for i in 0..4 {
            self.add_enemy(i as f32 * 2. - 3., -8. - i as f32, EnemyKind::Trooper);
        }
        for e in &mut self.enemies {
            e.hp = 0.;
        }
        self.nova_flash = 0.7;
        self.effect(FxKind::Nova, Vec3::new(0., 0.2, -5.), Vec3::ZERO, 0.7);
    }
    pub fn blast_fixture(&mut self) {
        self.start();
        self.enemies.clear();
        self.spawn_in = 3600.;
        self.obstacle_in = 3600.;
        for i in 0..4 {
            self.add_enemy(i as f32 * 1.5 - 2.25, -18., EnemyKind::Trooper);
        }
        self.aim = (Vec3::new(0., 1., -18.) - self.camera()).normalize();
        self.launch_plasma();
    }
    fn launch_plasma(&mut self) {
        if self.energy < 100. {
            return;
        }
        self.energy = 0.;
        self.blasts_fired += 1;
        let origin = self.camera();
        let direction = self.aim.normalize_or(Vec3::NEG_Z);
        let mut range: f32 = 34.;
        for e in &self.enemies {
            if let Some(t) = ray_sphere(origin, direction, e.pos + Vec3::Y, 0.85) {
                range = range.min(t);
            }
        }
        for b in &self.barriers {
            if let Some(t) = ray_box(
                origin,
                direction,
                Vec3::new(b.x, 1.15, b.z),
                Vec3::new(b.width * 0.5, 1.15, b.depth * 0.5),
            ) {
                range = range.min(t);
            }
        }
        let from = Vec3::new(self.x - 0.22, self.y + 1.38, -0.7);
        let velocity = (origin + direction * range - from).normalize_or(Vec3::NEG_Z) * 42.;
        let id = self.id();
        self.plasma.push(Plasma {
            id,
            pos: from,
            velocity,
            ttl: 0.85,
        });
        self.impact = 0.17;
        self.effect(FxKind::Launch, from, from, 0.25);
    }
    fn detonate(&mut self, pos: Vec3) {
        let mut kills = 0;
        for e in &mut self.enemies {
            if e.hp > 0. && (e.pos + Vec3::Y).distance(pos) <= BLAST_RADIUS {
                e.hp -= 160.;
                e.flash = 0.4;
                if e.hp <= 0. {
                    kills += 1;
                }
            }
        }
        if kills >= 2 {
            self.multikills += 1;
            self.multikill_size = kills;
            self.multikill_banner = 2.7;
            self.score += kills * 75;
        }
        self.detonations += 1;
        self.nova_flash = 0.7;
        self.impact = 0.65;
        self.hitmarker = if kills > 0 { 0.3 } else { self.hitmarker };
        self.bolts.retain(|b| b.pos.distance(pos) > BLAST_RADIUS);
        self.effect(FxKind::Detonate, pos, pos, 0.8);
    }
    pub fn encounter_name(&self) -> &'static str {
        match self.wave.saturating_sub(1) % 3 {
            0 => "INTERCEPT / break the firing line",
            1 => "PURSUIT / rushers incoming",
            _ => "CROSSFIRE / hold your escape route",
        }
    }
    pub fn sector(&self) -> u32 {
        1 + (self.distance / 300.) as u32
    }
    pub fn speed(&self) -> f32 {
        (8.0 + (self.sector() - 1) as f32 * 0.65).min(13.) * if self.focus { 0.78 } else { 1. }
    }
    pub fn camera(&self) -> Vec3 {
        Vec3::new(
            self.x + 0.65,
            2.25 + self.y * 0.45,
            if self.focus { 2.55 } else { 3.8 },
        )
    }
    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u32 << 24) as f32
    }
    fn add_enemy(&mut self, x: f32, z: f32, kind: EnemyKind) {
        let id = self.id();
        let hp = if kind == EnemyKind::Rusher { 65. } else { 95. };
        self.enemies.push(Enemy {
            id,
            pos: Vec3::new(x, 0., z),
            hp,
            max_hp: hp,
            kind,
            fire_in: 1.0 + (id % 3) as f32 * 0.4,
            flash: 0.,
        });
    }
    fn effect(&mut self, kind: FxKind, from: Vec3, to: Vec3, life: f32) {
        let id = self.id();
        self.effects.push(Fx {
            id,
            kind,
            from,
            to,
            age: 0.,
            life,
        });
    }
    pub fn damage(&mut self, amount: f32) {
        if amount <= 0. || self.dash_time > 0. || self.phase != Phase::Playing {
            return;
        }
        let absorbed = amount.min(self.shield);
        self.shield -= absorbed;
        self.health = (self.health - (amount - absorbed)).max(0.);
        self.since_hit = 0.;
        self.hurt = 0.65;
        self.combo = 0;
        self.effect(FxKind::Hurt, Vec3::new(self.x, 1., 0.), Vec3::ZERO, 0.4);
        if self.health <= 0. {
            self.phase = Phase::Dead;
            self.best = self.best.max(self.score + self.distance as u32);
        }
    }
    fn shoot(&mut self) {
        if self.reload > 0. || self.fire_cd > 0. {
            return;
        }
        if self.ammo == 0 {
            self.reload = 1.3;
            return;
        }
        if self.overdrive == 0. {
            self.ammo -= 1;
        }
        self.shots += 1;
        self.fire_cd = if self.overdrive > 0. { 0.065 } else { 0.105 };
        let origin = self.camera();
        let direction = self.aim.normalize_or(Vec3::NEG_Z);
        let mut closest = 110.;
        let mut victim = None;
        let mut headshot = false;
        for b in &self.barriers {
            if let Some(t) = ray_box(
                origin,
                direction,
                Vec3::new(b.x, 1.15, b.z),
                Vec3::new(b.width * 0.5, 1.15, b.depth * 0.5),
            ) && t < closest
            {
                closest = t;
            }
        }
        for (i, e) in self.enemies.iter().enumerate() {
            let head = ray_sphere(origin, direction, e.pos + Vec3::Y * 1.6, 0.29);
            let torso = ray_sphere(origin, direction, e.pos + Vec3::Y * 0.95, 0.62);
            if let Some(t) = head.or(torso)
                && t < closest
            {
                closest = t;
                victim = Some(i);
                headshot = head.is_some();
            }
        }
        let end = origin + direction * closest;
        self.effect(
            FxKind::Shot,
            Vec3::new(self.x - 0.22, self.y + 1.38, -0.55),
            end,
            0.075,
        );
        if let Some(i) = victim {
            self.enemies[i].hp -= if headshot { 75. } else { 30. };
            self.enemies[i].flash = if headshot { 0.32 } else { 0.12 };
            self.impact = if headshot { 0.13 } else { 0.055 };
            self.hits += 1;
            self.hitmarker = 0.14;
            self.effect(FxKind::Hit, end, end, 0.17);
        }
    }
    pub fn tick(&mut self, dt: f32, input: Input) {
        if self.phase != Phase::Playing {
            return;
        }
        self.time += dt;
        self.focus = input.focus;
        self.aim = input.aim.normalize_or(Vec3::NEG_Z);
        let speed = self.speed();
        self.distance += speed * dt;
        for v in [
            &mut self.fire_cd,
            &mut self.dash_cd,
            &mut self.dash_time,
            &mut self.hurt,
            &mut self.hitmarker,
            &mut self.nova_flash,
            &mut self.combo_time,
            &mut self.overdrive,
            &mut self.wave_banner,
            &mut self.impact,
            &mut self.multikill_banner,
        ] {
            *v = (*v - dt).max(0.);
        }
        if self.combo_time == 0. {
            self.combo = 0;
        }
        self.since_hit += dt;
        if self.since_hit > 3.5 {
            self.shield = (self.shield + 24. * dt).min(100.);
        }
        self.charge = (self.charge + 9. * dt).min(100.);
        if self.reload > 0. {
            self.reload = (self.reload - dt).max(0.);
            if self.reload == 0. {
                self.ammo = MAGAZINE;
            }
        }
        if input.reload && self.ammo < MAGAZINE && self.reload == 0. {
            self.reload = 1.3;
        }
        if input.jump && self.y <= 0.001 {
            self.vy = 7.2;
        }
        if input.dash && self.dash_cd == 0. {
            self.dash_cd = 1.5;
            self.dash_time = 0.22;
            self.dash_dir = if input.strafe.abs() > 0.1 {
                input.strafe.signum()
            } else if self.x > 0. {
                -1.
            } else {
                1.
            };
            self.effect(FxKind::Dash, Vec3::new(self.x, 0.8, 0.), Vec3::ZERO, 0.4);
        }
        self.x = (self.x
            + if self.dash_time > 0. {
                self.dash_dir * 16. * dt
            } else {
                input.strafe * 6.8 * dt
            })
        .clamp(-ROAD_HALF + 0.4, ROAD_HALF - 0.4);
        self.vy -= 19. * dt;
        self.y = (self.y + self.vy * dt).max(0.);
        if self.y == 0. {
            self.vy = 0.;
        }
        if input.nova && self.charge >= 100. {
            self.charge = 0.;
            self.nova_flash = 0.7;
            self.impact = 0.6;
            self.effect(FxKind::Nova, Vec3::new(self.x, 0.2, 0.), Vec3::ZERO, 0.7);
            for e in &mut self.enemies {
                if e.pos.distance(Vec3::new(self.x, 0., -7.)) < 19. {
                    e.hp -= 130.;
                }
            }
            self.bolts.retain(|b| b.pos.z < -26.);
        }
        if input.secondary {
            self.launch_plasma();
        }
        if input.fire {
            self.shoot();
        }
        self.spawn_in -= dt;
        if self.spawn_in <= 0. {
            self.wave += 1;
            self.wave_banner = 2.6;
            let mirror = if self.wave.is_multiple_of(2) { -1. } else { 1. };
            match (self.wave - 1) % 3 {
                0 => {
                    // A firing line: shift laterally and prioritize its nearest gun.
                    for i in 0..3 {
                        self.add_enemy(
                            (i as f32 - 1.) * 3.,
                            -48. - i as f32 * 3.,
                            EnemyKind::Trooper,
                        );
                    }
                }
                1 => {
                    // Rushers force a dodge or a well-timed shockwave.
                    for i in 0..3 {
                        self.add_enemy(
                            mirror * (i as f32 - 1.) * 2.6,
                            -45. - i as f32 * 5.,
                            EnemyKind::Rusher,
                        );
                    }
                    self.add_enemy(-mirror * 3.5, -63., EnemyKind::Trooper);
                }
                _ => {
                    // Mixed crossfire, followed by a supply/recovery interval.
                    for side in [-1., 1.] {
                        self.add_enemy(side * 3.5, -50., EnemyKind::Trooper);
                    }
                    self.add_enemy(0., -58., EnemyKind::Rusher);
                    let id = self.id();
                    self.pickups.push(Pickup {
                        id,
                        pos: Vec3::new(mirror * 2.5, 0.8, -78.),
                    });
                }
            }
            self.spawn_in = if self.wave.is_multiple_of(3) {
                12.
            } else {
                (9. - self.sector() as f32 * 0.25).max(6.5)
            };
        }
        self.obstacle_in -= dt;
        if self.obstacle_in <= 0. {
            let id = self.id();
            let x = (self.random() * 2. - 1.) * 3.1;
            if self.random() < 0.5 {
                self.pickups.push(Pickup {
                    id,
                    pos: Vec3::new(x, 0.8, -72.),
                });
            } else {
                self.barriers.push(Barrier {
                    id,
                    x,
                    z: -72.,
                    width: self.barrier_widths[1],
                    depth: self.barrier_depths[1],
                });
            }
            self.obstacle_in = 4.5 + self.random() * 2.5;
        }
        let mut fire = vec![];
        let mut damage = 0.;
        for e in &mut self.enemies {
            if e.hp <= 0. {
                continue;
            }
            e.flash = (e.flash - dt).max(0.);
            e.pos.z += (speed
                + if e.kind == EnemyKind::Rusher && e.flash == 0. {
                    3.5
                } else {
                    0.
                })
                * dt;
            if e.kind == EnemyKind::Rusher && e.pos.z > -18. {
                e.pos.x += (self.x - e.pos.x).clamp(-2.5, 2.5) * dt;
            }
            if e.kind == EnemyKind::Trooper && e.pos.z > -46. && e.pos.z < -4. {
                e.fire_in -= dt;
                if e.fire_in <= 0. {
                    fire.push(e.pos + Vec3::Y * 1.25);
                    e.fire_in = 2.0;
                }
            }
            if e.pos.z >= -0.6 && e.pos.z < 1.0 && (e.pos.x - self.x).abs() < 0.8 && self.y < 1.2 {
                damage += 28.;
                e.pos.z = 4.;
            }
        }
        let mut detonations = vec![];
        for p in &mut self.plasma {
            let travel = p.velocity * dt;
            let direction = travel.normalize_or_zero();
            let mut impact = travel.length();
            let mut hit = false;
            for e in &self.enemies {
                if e.hp > 0.
                    && let Some(t) = ray_sphere(p.pos, direction, e.pos + Vec3::Y, 0.85)
                    && t <= impact
                {
                    impact = t;
                    hit = true;
                }
            }
            for b in &self.barriers {
                if let Some(t) = ray_box(
                    p.pos,
                    direction,
                    Vec3::new(b.x, 1.15, b.z),
                    Vec3::new(b.width * 0.5, 1.15, b.depth * 0.5),
                ) && t <= impact
                {
                    impact = t;
                    hit = true;
                }
            }
            if direction.y < -0.0001 {
                let t = (0.15 - p.pos.y) / direction.y;
                if t >= 0. && t <= impact {
                    impact = t;
                    hit = true;
                }
            }
            p.pos += direction * impact + Vec3::Z * speed * dt;
            p.ttl -= dt;
            if hit || p.ttl <= 0. {
                detonations.push(p.pos);
                p.ttl = 0.;
            }
        }
        self.plasma.retain(|p| p.ttl > 0.);
        for pos in detonations {
            self.detonate(pos);
        }
        for pos in fire {
            let id = self.id();
            let velocity = (Vec3::new(self.x, self.y + 0.85, 1.) - pos).normalize() * 15.;
            self.bolts.push(Bolt { id, pos, velocity });
        }
        for bolt in &mut self.bolts {
            let old = bolt.pos;
            // Bolt and cover share the scrolling frame; test relative travel before
            // adding the world's common displacement to either object.
            let travel = bolt.velocity * dt;
            let blocked = self.barriers.iter().any(|b| {
                ray_box(
                    old,
                    travel.normalize_or_zero(),
                    Vec3::new(b.x, 1.15, b.z),
                    Vec3::new(b.width * 0.5, 1.15, b.depth * 0.5),
                )
                .is_some_and(|t| t <= travel.length())
            });
            if blocked {
                bolt.pos.z = 20.;
                continue;
            }
            bolt.pos += (bolt.velocity + Vec3::Z * speed) * dt;
            if segment_distance(old, bolt.pos, Vec3::new(self.x, self.y + 0.9, 0.)) < 0.48 {
                damage += 16.;
                bolt.pos.z = 20.;
            }
        }
        for b in &mut self.barriers {
            let old = b.z;
            b.z += speed * dt;
            if old < b.depth * 0.5 + 0.3
                && b.z >= -b.depth * 0.5 - 0.3
                && (b.x - self.x).abs() < b.width * 0.5 + 0.3
                && self.y < 2.3
            {
                damage += 30.;
                b.z = 20.;
            }
        }
        self.damage(damage);
        let mut collected = vec![];
        for p in &mut self.pickups {
            p.pos.z += speed * dt;
            if p.pos.distance(Vec3::new(self.x, self.y + 0.8, 0.)) < 1.1 {
                collected.push(p.pos);
                p.pos.z = 20.;
            }
        }
        for pos in collected {
            self.shield = (self.shield + 30.).min(100.);
            self.charge = (self.charge + 25.).min(100.);
            self.ammo = MAGAZINE;
            self.reload = 0.;
            self.supplies_collected += 1;
            self.score += 30;
            self.effect(FxKind::Pickup, pos, pos, 1.2);
        }
        let mut energy_positions = vec![];
        for p in &mut self.energy_drops {
            p.pos.z += speed * dt;
            let player = Vec3::new(self.x, self.y + 0.85, 0.);
            let delta = player - p.pos;
            if delta.length() < 5. {
                p.pos += delta.normalize_or_zero() * 9. * dt;
            }
            if p.pos.distance(player) < 1.2 {
                energy_positions.push(p.pos);
                p.pos.z = 20.;
            }
        }
        for pos in energy_positions {
            self.energy = (self.energy + 25.).min(100.);
            self.energy_collected += 1;
            self.effect(FxKind::Energy, pos, pos, 0.5);
        }
        self.energy_drops.retain(|p| p.pos.z < 8.);
        let dead: Vec<_> = self
            .enemies
            .iter()
            .filter(|e| e.hp <= 0.)
            .map(|e| e.pos)
            .collect();
        for pos in dead {
            self.kills += 1;
            self.combo += 1;
            self.combo_time = 6.;
            self.impact = self.impact.max(0.28);
            let id = self.id();
            self.energy_drops.push(Energy {
                id,
                pos: Vec3::new(pos.x.clamp(-4.4, 4.4), 0.85, pos.z),
            });
            self.charge = (self.charge + 12.).min(100.);
            if self.combo.is_multiple_of(4) {
                if self.overdrive == 0. {
                    self.overdrive_activations += 1;
                }
                self.overdrive = 6.;
                self.ammo = MAGAZINE;
                self.reload = 0.;
            }
            self.score += 100 * (1 + self.combo / 4);
            self.effect(FxKind::Kill, pos + Vec3::Y, pos + Vec3::Y, 0.65);
        }
        self.enemies.retain(|e| e.hp > 0. && e.pos.z < 7.);
        self.barriers.retain(|b| b.z < 10.);
        self.bolts.retain(|b| b.pos.z < 12. && b.pos.x.abs() < 20.);
        self.pickups.retain(|p| p.pos.z < 10.);
        for fx in &mut self.effects {
            if matches!(fx.kind, FxKind::Detonate | FxKind::Launch) {
                fx.from.z += speed * dt;
            }
            fx.age += dt;
        }
        self.effects.retain(|fx| fx.age < fx.life);
    }
}
pub fn ray_sphere(origin: Vec3, direction: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let delta = origin - center;
    let b = delta.dot(direction);
    let c = delta.length_squared() - radius * radius;
    let det = b * b - c;
    if det < 0. {
        return None;
    }
    let t = -b - det.sqrt();
    (t >= 0.).then_some(t)
}
fn ray_box(origin: Vec3, dir: Vec3, center: Vec3, half: Vec3) -> Option<f32> {
    let mut low: f32 = 0.;
    let mut high: f32 = 200.;
    for i in 0..3 {
        if dir[i].abs() < 1e-6 {
            if (origin[i] - center[i]).abs() > half[i] {
                return None;
            }
        } else {
            let a = (center[i] - half[i] - origin[i]) / dir[i];
            let b = (center[i] + half[i] - origin[i]) / dir[i];
            low = low.max(a.min(b));
            high = high.min(a.max(b));
        }
    }
    (low <= high).then_some(low)
}
fn segment_distance(a: Vec3, b: Vec3, p: Vec3) -> f32 {
    let delta = b - a;
    let t = ((p - a).dot(delta) / delta.length_squared().max(1e-9)).clamp(0., 1.);
    p.distance(a + delta * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plasma_spends_once_travels_then_kills_cluster_and_drops_energy() {
        let mut g = Game::new();
        g.blast_fixture();
        assert_eq!(g.blasts_fired, 1);
        assert_eq!(g.energy, 0.);
        assert_eq!(g.kills, 0);
        for _ in 0..60 {
            g.tick(
                1. / 60.,
                Input {
                    secondary: true,
                    ..default()
                },
            );
        }
        assert_eq!(g.blasts_fired, 1);
        assert_eq!(g.detonations, 1);
        assert_eq!(g.kills, 4);
        assert_eq!(g.multikills, 1);
        assert_eq!(g.multikill_size, 4);
        assert_eq!(g.energy_drops.len(), 4);
        assert_eq!(g.energy, 0.); // kills and time do not refill plasma directly
        assert_eq!(g.overdrive_activations, 1);
    }
    #[test]
    fn energy_collection_refills_plasma_once_without_refilling_magazine() {
        let mut g = Game::new();
        g.start();
        g.enemies.clear();
        g.energy = 80.;
        g.ammo = 3;
        g.energy_drops.push(Energy {
            id: 999,
            pos: Vec3::new(0., 0.85, -0.2),
        });
        g.tick(0.01, Input::default());
        assert_eq!(g.energy, 100.);
        assert_eq!(g.energy_collected, 1);
        assert_eq!(g.ammo, 3);
        g.tick(0.01, Input::default());
        assert_eq!(g.energy_collected, 1);
        g.start();
        assert_eq!(g.multikills, 0);
        assert!(g.energy_drops.is_empty());
        assert!(g.plasma.is_empty());
    }
    #[test]
    fn detonation_radius_and_multikill_count_only_new_victims() {
        let mut g = Game::new();
        g.start();
        g.enemies.clear();
        g.add_enemy(0., -20., EnemyKind::Trooper);
        g.add_enemy(0., -28., EnemyKind::Trooper);
        g.detonate(Vec3::new(0., 1., -20.));
        assert!(g.enemies[0].hp <= 0.);
        assert_eq!(g.enemies[1].hp, 95.);
        assert_eq!(g.multikills, 0);
        g.detonate(Vec3::new(0., 1., -20.));
        assert_eq!(g.multikills, 0);
    }
    #[test]
    fn plasma_hits_cover_before_distant_enemies_and_cleans_up() {
        let mut g = Game::new();
        g.start();
        g.enemies.clear();
        g.spawn_in = 3600.;
        g.add_enemy(0., -25., EnemyKind::Trooper);
        g.barriers.push(Barrier {
            id: 999,
            x: 0.,
            z: -5.,
            width: 3.,
            depth: 1.2,
        });
        g.aim = (Vec3::new(0., 1., -25.) - g.camera()).normalize();
        g.launch_plasma();
        for _ in 0..20 {
            g.tick(1. / 60., Input::default());
        }
        assert_eq!(g.detonations, 1);
        assert_eq!(g.kills, 0);
        assert!(g.plasma.is_empty());
    }
    #[test]
    fn four_chained_kills_trigger_temporary_overdrive() {
        let mut g = Game::new();
        g.start();
        g.enemies.clear();
        for i in 0..4 {
            g.add_enemy(i as f32, -15., EnemyKind::Trooper);
        }
        for e in &mut g.enemies {
            e.hp = 0.;
        }
        g.tick(0.01, Input::default());
        assert_eq!(g.combo, 4);
        assert_eq!(g.overdrive, 6.);
        assert_eq!(g.overdrive_activations, 1);
        g.shoot();
        assert_eq!(g.ammo, MAGAZINE);
        g.overdrive = 0.001;
        g.tick(0.1, Input::default());
        g.shoot();
        assert_eq!(g.ammo, MAGAZINE - 1);
        g.start();
        assert_eq!(g.overdrive, 0.);
        assert_eq!(g.wave, 0);
    }
    #[test]
    fn extending_overdrive_does_not_repeat_the_activation_call() {
        let mut g = Game::new();
        g.start();
        g.enemies.clear();
        g.combo = 3;
        g.combo_time = 6.;
        g.overdrive = 2.;
        g.overdrive_activations = 1;
        g.add_enemy(0., -10., EnemyKind::Trooper);
        g.enemies[0].hp = 0.;
        g.tick(0.01, Input::default());
        assert_eq!(g.overdrive, 6.);
        assert_eq!(g.overdrive_activations, 1);
        g.start();
        assert_eq!(g.overdrive_activations, 0);
    }
    #[test]
    fn encounter_cycle_changes_composition_and_grants_recovery() {
        let mut g = Game::new();
        g.start();
        for wave in 1..=3 {
            g.enemies.clear();
            g.spawn_in = 0.;
            g.tick(0.01, Input::default());
            assert_eq!(g.wave, wave);
            let rushers = g
                .enemies
                .iter()
                .filter(|e| e.kind == EnemyKind::Rusher)
                .count();
            assert_eq!(rushers, [0, 3, 1][(wave - 1) as usize]);
        }
        assert_eq!(g.spawn_in, 12.);
        assert_eq!(g.pickups.len(), 1);
    }
    #[test]
    fn barrier_contact_uses_its_visible_depth() {
        for (depth, expected_shield) in [(1.2, 100.), (3., 70.)] {
            let mut g = Game::new();
            g.start();
            g.enemies.clear();
            g.barriers.push(Barrier {
                id: 999,
                x: 0.,
                z: -1.3,
                width: 2.,
                depth,
            });
            g.tick(1. / 60., Input::default());
            assert_eq!(g.shield, expected_shield);
        }
    }
    #[test]
    fn crowded_fixture_keeps_the_render_workload_alive() {
        let mut g = Game::new();
        g.start();
        for _ in 0..1200 {
            g.crowded_fixture();
            g.tick(
                1. / 60.,
                Input {
                    fire: true,
                    aim: Vec3::NEG_Z,
                    ..default()
                },
            );
            assert_eq!(g.phase, Phase::Playing);
            assert_eq!(g.enemies.len(), 20);
            assert!(g.enemies.iter().all(|e| e.kind == EnemyKind::Trooper));
        }
        assert!(g.shots > 0);
    }
    #[test]
    fn pause_freezes_everything() {
        let mut g = Game::new();
        g.start();
        g.phase = Phase::Paused;
        g.tick(
            1.,
            Input {
                fire: true,
                ..default()
            },
        );
        assert_eq!(g.distance, 0.);
        assert_eq!(g.ammo, MAGAZINE);
    }
    #[test]
    fn shields_regenerate_but_health_does_not() {
        let mut g = Game::new();
        g.start();
        g.damage(130.);
        assert_eq!(g.health, 70.);
        assert_eq!(g.shield, 0.);
        g.since_hit = 4.;
        g.tick(0.1, Input::default());
        assert!(g.shield > 0.);
        assert_eq!(g.health, 70.);
    }
    #[test]
    fn zero_damage_does_not_reset_regeneration() {
        let mut g = Game::new();
        g.start();
        g.shield = 50.;
        g.tick(0.1, Input::default());
        assert!(g.since_hit > 4.);
    }
    #[test]
    fn nova_clears_nearby_threats_and_spends_charge() {
        let mut g = Game::new();
        g.start();
        g.enemies[0].pos.z = -10.;
        g.tick(
            0.01,
            Input {
                nova: true,
                ..default()
            },
        );
        assert_eq!(g.kills, 1);
        assert_eq!(g.charge, 12.); // the kill returns charge after the cast spends 100
    }
    #[test]
    fn shots_hit_the_nearest_enemy() {
        let mut g = Game::new();
        g.start();
        g.enemies.clear();
        g.add_enemy(0., -10., EnemyKind::Trooper);
        g.add_enemy(0., -20., EnemyKind::Trooper);
        g.aim = (Vec3::new(0., 1., -10.) - g.camera()).normalize();
        g.shoot();
        assert!(g.enemies[0].hp < 95.);
        assert_eq!(g.enemies[1].hp, 95.);
    }
    #[test]
    fn cover_blocks_shots() {
        let mut g = Game::new();
        g.start();
        g.enemies.clear();
        g.add_enemy(0., -10., EnemyKind::Trooper);
        g.barriers.push(Barrier {
            id: 999,
            x: 0.,
            z: -5.,
            width: 3.,
            depth: 1.2,
        });
        g.aim = (Vec3::new(0., 1., -10.) - g.camera()).normalize();
        g.shoot();
        assert_eq!(g.enemies[0].hp, 95.);
    }
    #[test]
    fn supply_pickup_refills_resources_once_without_damage() {
        let mut g = Game::new();
        g.start();
        g.enemies.clear();
        g.shield = 40.;
        g.charge = 10.;
        g.ammo = 0;
        g.reload = 1.;
        g.pickups.push(Pickup {
            id: 999,
            pos: Vec3::new(0., 0.8, -0.2),
        });
        g.tick(1. / 60., Input::default());
        assert!((g.shield - (70. + 24. / 60.)).abs() < 0.001);
        assert_eq!(g.health, 100.);
        assert!(g.charge >= 35.);
        assert_eq!(g.ammo, MAGAZINE);
        assert_eq!(g.reload, 0.);
        assert!(g.pickups.is_empty());
        assert_eq!(g.supplies_collected, 1);
        let score = g.score;
        g.tick(1. / 60., Input::default());
        assert_eq!(g.supplies_collected, 1);
        assert_eq!(g.score, score);
    }
    #[test]
    fn long_runs_bound_world_entities() {
        let mut g = Game::new();
        g.start();
        for _ in 0..36000 {
            g.health = 100.;
            g.shield = 100.;
            g.tick(1. / 60., Input::default());
        }
        assert!(g.distance > 4000.);
        assert!(g.enemies.len() < 40);
        assert!(g.barriers.len() < 8);
        assert!(g.bolts.len() < 150);
    }
    #[test]
    fn reload_restores_the_magazine_once() {
        let mut g = Game::new();
        g.start();
        g.ammo = 3;
        g.tick(
            0.01,
            Input {
                reload: true,
                ..default()
            },
        );
        assert_eq!(g.ammo, 3);
        for _ in 0..140 {
            g.tick(0.01, Input::default());
        }
        assert_eq!(g.ammo, MAGAZINE);
        assert_eq!(g.reload, 0.);
    }
    #[test]
    fn dodge_has_bounded_immunity_and_cooldown() {
        let mut g = Game::new();
        g.start();
        g.tick(
            0.01,
            Input {
                dash: true,
                ..default()
            },
        );
        g.damage(80.);
        assert_eq!(g.shield, 100.);
        for _ in 0..25 {
            g.tick(0.01, Input::default());
        }
        g.damage(20.);
        assert_eq!(g.shield, 80.);
        assert!(g.dash_cd > 0.);
    }
    #[test]
    fn restart_restores_run_state_but_keeps_preferences() {
        let mut g = Game::new();
        g.best = 500;
        g.sound = false;
        g.start();
        g.damage(250.);
        assert_eq!(g.phase, Phase::Dead);
        g.start();
        assert_eq!(g.phase, Phase::Playing);
        assert_eq!(g.best, 500);
        assert!(!g.sound);
        assert_eq!(g.health, 100.);
        assert_eq!(g.distance, 0.);
    }
    #[test]
    fn incoming_bolts_stop_at_cover() {
        let mut g = Game::new();
        g.start();
        g.enemies.clear();
        g.barriers.push(Barrier {
            id: 90,
            x: 0.,
            z: -5.,
            width: 3.,
            depth: 1.2,
        });
        g.bolts.push(Bolt {
            id: 91,
            pos: Vec3::new(0., 1., -6.),
            velocity: Vec3::Z * 15.,
        });
        g.tick(0.1, Input::default());
        assert!(g.bolts.is_empty());
        assert_eq!(g.shield, 100.);
    }
}
