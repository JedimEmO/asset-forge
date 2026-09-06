//! Deterministic arena rules. Rendering and input never own gameplay state.
use bevy::prelude::{Resource, Vec2};

pub(crate) const ARENA_HALF: f32 = 22.0;
pub(crate) const MAX_ENEMIES: usize = 140;
pub(crate) const MAX_BULLETS: usize = 650;
pub(crate) const MAX_EFFECTS: usize = 160;
pub(crate) const MAX_PICKUPS: usize = 240;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Title,
    Playing,
    Upgrade,
    Paused,
    Victory,
    Defeat,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnemyKind {
    Rusher,
    Shooter,
    Tank,
    Bomber,
    Boss,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PickupKind {
    Xp,
    Health,
    Overdrive,
    Magnet,
    Shield,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EffectKind {
    Muzzle,
    Hit,
    Explosion,
    Dash,
    Heal,
    LevelUp,
    Spawn,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpgradeKind {
    Damage,
    FireRate,
    MaxHealth,
    MoveSpeed,
    Multishot,
    Piercing,
    Magnet,
    Regeneration,
    DashCooldown,
    Critical,
    Shield,
    BlastRounds,
    LifeSteal,
    Overclock,
}
impl UpgradeKind {
    pub(crate) const ALL: [Self; 14] = [
        Self::Damage,
        Self::FireRate,
        Self::MaxHealth,
        Self::MoveSpeed,
        Self::Multishot,
        Self::Piercing,
        Self::Magnet,
        Self::Regeneration,
        Self::DashCooldown,
        Self::Critical,
        Self::Shield,
        Self::BlastRounds,
        Self::LifeSteal,
        Self::Overclock,
    ];
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Damage => "Rail punch",
            Self::FireRate => "Hair trigger",
            Self::MaxHealth => "Salvaged armor",
            Self::MoveSpeed => "Servo boots",
            Self::Multishot => "Split chamber",
            Self::Piercing => "Tungsten core",
            Self::Magnet => "Scrap magnet",
            Self::Regeneration => "Repair nanites",
            Self::DashCooldown => "Phase capacitor",
            Self::Critical => "Target analyzer",
            Self::Shield => "Barrier cell",
            Self::BlastRounds => "Volatile rounds",
            Self::LifeSteal => "Salvage siphon",
            Self::Overclock => "Overclock",
        }
    }
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::Damage => "+25% weapon damage. Punch through heavier machines.",
            Self::FireRate => "+18% firing speed. More lead, fewer survivors.",
            Self::MaxHealth => "+25 maximum hull and repair 35 hull now.",
            Self::MoveSpeed => "+10% movement speed. Keep ahead of the horde.",
            Self::Multishot => "Fire an extra round in a tight spread. Up to 4 rounds.",
            Self::Piercing => "Each round penetrates one additional enemy.",
            Self::Magnet => "+2 m pickup range and +15% XP from scrap.",
            Self::Regeneration => "Repair 0.6 hull per second while fighting.",
            Self::DashCooldown => "Dash recharges 18% faster; +0.04 s invulnerability.",
            Self::Critical => "+12% chance to deal double damage.",
            Self::Shield => "+20 barrier capacity. Barrier recharges after 5 s unharmed.",
            Self::BlastRounds => "Impacts splash 25% weapon damage within 1.5 m.",
            Self::LifeSteal => "Repair 0.7 hull for each machine destroyed.",
            Self::Overclock => "+15% damage and fire speed, but lose 10 maximum hull.",
        }
    }
    fn cap(self) -> u32 {
        match self {
            Self::Multishot | Self::Piercing | Self::BlastRounds => 3,
            Self::Critical => 5,
            _ => 6,
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Stats {
    pub(crate) damage: f32,
    pub(crate) fire_rate: f32,
    pub(crate) speed: f32,
    pub(crate) multishot: u32,
    pub(crate) pierce: u32,
    pub(crate) pickup_range: f32,
    pub(crate) xp_multiplier: f32,
    pub(crate) regen: f32,
    pub(crate) dash_recharge: f32,
    pub(crate) crit: f32,
    pub(crate) shield_max: f32,
    pub(crate) blast: u32,
    pub(crate) life_steal: f32,
}
impl Default for Stats {
    fn default() -> Self {
        Self {
            damage: 18.,
            fire_rate: 6.,
            speed: 7.,
            multishot: 1,
            pierce: 0,
            pickup_range: 3.,
            xp_multiplier: 1.,
            regen: 0.,
            dash_recharge: 2.8,
            crit: 0.05,
            shield_max: 0.,
            blast: 0,
            life_steal: 0.,
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Player {
    pub(crate) pos: Vec2,
    pub(crate) aim: Vec2,
    pub(crate) hp: f32,
    pub(crate) max_hp: f32,
    pub(crate) shield: f32,
    pub(crate) dash_cooldown: f32,
    pub(crate) dashing: f32,
    pub(crate) invulnerable: f32,
    pub(crate) stats: Stats,
    pub(crate) overdrive: f32,
    pub(crate) magnet: f32,
    fire_timer: f32,
    hurt_timer: f32,
    dash_dir: Vec2,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct Input {
    pub(crate) movement: Vec2,
    pub(crate) aim: Vec2,
    pub(crate) fire: bool,
    pub(crate) dash: bool,
}
#[derive(Clone, Debug)]
pub(crate) struct Enemy {
    pub(crate) id: u64,
    pub(crate) kind: EnemyKind,
    pub(crate) pos: Vec2,
    pub(crate) hp: f32,
    pub(crate) max_hp: f32,
    pub(crate) radius: f32,
    pub(crate) hit_flash: f32,
    pub(crate) attack_timer: f32,
    pub(crate) aim: Vec2,
    pub(crate) telegraph: f32,
    pub(crate) spawn_timer: f32,
    charge: f32,
}
#[derive(Clone, Debug)]
pub(crate) struct Bullet {
    pub(crate) id: u64,
    pub(crate) pos: Vec2,
    pub(crate) dir: Vec2,
    pub(crate) radius: f32,
    pub(crate) hostile: bool,
    pub(crate) life: f32,
    pub(crate) damage: f32,
    pub(crate) speed: f32,
    pierce: u32,
    hit_ids: Vec<u64>,
}
#[derive(Clone, Debug)]
pub(crate) struct Pickup {
    pub(crate) id: u64,
    pub(crate) kind: PickupKind,
    pub(crate) pos: Vec2,
    pub(crate) radius: f32,
    pub(crate) age: f32,
    pub(crate) value: f32,
}
#[derive(Clone, Debug)]
pub(crate) struct Effect {
    pub(crate) id: u64,
    pub(crate) kind: EffectKind,
    pub(crate) pos: Vec2,
    pub(crate) age: f32,
    pub(crate) duration: f32,
    pub(crate) radius: f32,
}
#[derive(Resource, Clone)]
pub(crate) struct Sim {
    pub(crate) phase: Phase,
    pub(crate) player: Player,
    pub(crate) enemies: Vec<Enemy>,
    pub(crate) bullets: Vec<Bullet>,
    pub(crate) pickups: Vec<Pickup>,
    pub(crate) effects: Vec<Effect>,
    pub(crate) wave: u32,
    pub(crate) max_waves: u32,
    pub(crate) wave_remaining: u32,
    pub(crate) wave_timer: f32,
    pub(crate) run_time: f32,
    pub(crate) kills: u32,
    pub(crate) score: u32,
    pub(crate) level: u32,
    pub(crate) xp: f32,
    pub(crate) xp_to_next: f32,
    pub(crate) choices: Vec<UpgradeKind>,
    pub(crate) message: String,
    pub(crate) message_timer: f32,
    seed: u64,
    rng: u64,
    next_id: u64,
    ranks: [u32; 14],
    pending_upgrades: u32,
    next_wave: bool,
    boss_spawned: bool,
    burst_remaining: u32,
    burst_size: u32,
    burst_edges: [u32; 2],
    resume_phase: Phase,
}
impl Sim {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            phase: Phase::Title,
            player: Player {
                pos: Vec2::ZERO,
                aim: Vec2::NEG_Y,
                hp: 100.,
                max_hp: 100.,
                shield: 0.,
                dash_cooldown: 0.,
                dashing: 0.,
                invulnerable: 0.,
                stats: Stats::default(),
                overdrive: 0.,
                magnet: 0.,
                fire_timer: 0.,
                hurt_timer: 0.,
                dash_dir: Vec2::NEG_Y,
            },
            enemies: Vec::new(),
            bullets: Vec::new(),
            pickups: Vec::new(),
            effects: Vec::new(),
            wave: 0,
            max_waves: 10,
            wave_remaining: 0,
            wave_timer: 0.,
            run_time: 0.,
            kills: 0,
            score: 0,
            level: 1,
            xp: 0.,
            xp_to_next: 35.,
            choices: Vec::new(),
            message: "Recover the yard. Survive ten waves.".into(),
            message_timer: 5.,
            seed,
            rng: seed.max(1),
            next_id: 1,
            ranks: [0; 14],
            pending_upgrades: 0,
            next_wave: false,
            boss_spawned: false,
            burst_remaining: 0,
            burst_size: 0,
            burst_edges: [0, 1],
            resume_phase: Phase::Playing,
        }
    }
    pub(crate) fn start_run(&mut self) {
        let seed = self.seed;
        *self = Self::new(seed);
        self.phase = Phase::Playing;
        self.begin_wave();
    }
    /// Authored inspection state for explicit capture scenarios, never normal play.
    /// Starting a run replaces every preview override through the ordinary reset.
    pub(crate) fn prepare_showcase(&mut self, boss: bool) {
        self.start_run();
        self.wave = if boss { self.max_waves } else { 7 };
        self.wave_remaining = 0;
        self.wave_timer = 3600.;
        self.player.pos = Vec2::new(0., 2.);
        self.player.aim = Vec2::NEG_Y;
        self.player.max_hp = 1000.;
        self.player.hp = self.player.max_hp;
        self.player.invulnerable = 60.;
        if boss {
            self.spawn(EnemyKind::Boss, Vec2::new(0., -8.));
            self.enemies[0].hp = self.enemies[0].max_hp * 0.49;
            for (kind, position) in [
                (EnemyKind::Rusher, Vec2::new(-7., -3.)),
                (EnemyKind::Shooter, Vec2::new(7., -3.)),
                (EnemyKind::Tank, Vec2::new(-7., 6.)),
                (EnemyKind::Bomber, Vec2::new(7., 6.)),
            ] {
                self.spawn(kind, position);
            }
        } else {
            let kinds = [
                EnemyKind::Rusher,
                EnemyKind::Shooter,
                EnemyKind::Tank,
                EnemyKind::Bomber,
            ];
            for index in 0..18 {
                let angle = index as f32 * std::f32::consts::TAU / 18.;
                let radius = 11. + (index % 3) as f32 * 2.;
                self.spawn(
                    kinds[index % kinds.len()],
                    Vec2::new(angle.cos(), angle.sin()) * radius,
                );
            }
        }
        for enemy in &mut self.enemies {
            enemy.spawn_timer = 0.;
            enemy.aim = (self.player.pos - enemy.pos).normalize_or_zero();
            enemy.attack_timer = 1.5;
        }
        self.effects.clear();
        self.say("INSPECTION SCENE / authored asset showcase".into());
    }
    pub(crate) fn return_to_title(&mut self) {
        *self = Self::new(self.seed);
    }
    pub(crate) fn toggle_pause(&mut self) {
        match self.phase {
            Phase::Playing | Phase::Upgrade => {
                self.resume_phase = self.phase;
                self.phase = Phase::Paused;
            }
            Phase::Paused => self.phase = self.resume_phase,
            _ => {}
        }
    }
    pub(crate) fn upgrade_rank(&self, kind: UpgradeKind) -> u32 {
        self.ranks[kind as usize]
    }
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u32 << 24) as f32
    }
    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    fn say(&mut self, message: String) {
        self.message = message;
        self.message_timer = 3.5;
    }
    fn effect(&mut self, kind: EffectKind, pos: Vec2, radius: f32, duration: f32) {
        if self.effects.len() < MAX_EFFECTS {
            let id = self.id();
            self.effects.push(Effect {
                id,
                kind,
                pos,
                age: 0.,
                duration,
                radius,
            });
        }
    }
    fn begin_wave(&mut self) {
        self.wave += 1;
        self.wave_remaining = 24 + self.wave * 14;
        self.wave_timer = 2.5;
        self.next_wave = false;
        self.boss_spawned = false;
        self.burst_remaining = 0;
        self.phase = Phase::Playing;
        self.player.invulnerable = 2.;
        self.say(if self.wave == 10 {
            "WAVE 10 / 10 / THE FOREMAN".into()
        } else {
            format!("WAVE {} / 10 / HOLD THE YARD", self.wave)
        });
    }
    fn offer(&mut self) {
        self.choices.clear();
        let mut pool: Vec<_> = UpgradeKind::ALL
            .into_iter()
            .filter(|k| self.upgrade_rank(*k) < k.cap())
            .collect();
        while self.choices.len() < 3 && !pool.is_empty() {
            let i = (self.random() * pool.len() as f32) as usize;
            self.choices.push(pool.swap_remove(i.min(pool.len() - 1)));
        }
        if self.choices.is_empty() {
            self.pending_upgrades = 0;
            if self.next_wave {
                self.begin_wave();
            } else {
                self.phase = Phase::Playing;
            }
        } else {
            self.phase = Phase::Upgrade;
        }
    }
    pub(crate) fn choose_upgrade(&mut self, index: usize) {
        if self.phase != Phase::Upgrade {
            return;
        }
        let Some(kind) = self.choices.get(index).copied() else {
            return;
        };
        self.ranks[kind as usize] += 1;
        let s = &mut self.player.stats;
        match kind {
            UpgradeKind::Damage => s.damage *= 1.25,
            UpgradeKind::FireRate => s.fire_rate *= 1.18,
            UpgradeKind::MaxHealth => {
                self.player.max_hp += 25.;
                self.player.hp = (self.player.hp + 35.).min(self.player.max_hp);
            }
            UpgradeKind::MoveSpeed => s.speed *= 1.1,
            UpgradeKind::Multishot => s.multishot += 1,
            UpgradeKind::Piercing => s.pierce += 1,
            UpgradeKind::Magnet => {
                s.pickup_range += 2.;
                s.xp_multiplier += 0.15;
            }
            UpgradeKind::Regeneration => s.regen += 0.6,
            UpgradeKind::DashCooldown => s.dash_recharge *= 0.82,
            UpgradeKind::Critical => s.crit += 0.12,
            UpgradeKind::Shield => {
                s.shield_max += 20.;
                self.player.shield = s.shield_max;
            }
            UpgradeKind::BlastRounds => s.blast += 1,
            UpgradeKind::LifeSteal => s.life_steal += 0.7,
            UpgradeKind::Overclock => {
                s.damage *= 1.15;
                s.fire_rate *= 1.15;
                self.player.max_hp = (self.player.max_hp - 10.).max(30.);
                self.player.hp = self.player.hp.min(self.player.max_hp);
            }
        }
        self.pending_upgrades = self.pending_upgrades.saturating_sub(1);
        self.choices.clear();
        self.say(format!("{} installed", kind.title()));
        if self.pending_upgrades > 0 {
            self.offer();
        } else if self.next_wave {
            self.begin_wave();
        } else {
            self.phase = Phase::Playing;
            self.player.invulnerable = 1.2;
        }
    }
    pub(crate) fn step(&mut self, dt: f32, input: Input) {
        if self.phase != Phase::Playing || !dt.is_finite() || dt <= 0. {
            return;
        }
        let mut remaining = dt.min(0.25);
        let mut sub_input = input;
        while remaining > 0.000_001 && self.phase == Phase::Playing {
            let step = remaining.min(1. / 60.);
            self.tick(step, sub_input);
            sub_input.dash = false;
            remaining -= step;
        }
    }
    fn tick(&mut self, dt: f32, input: Input) {
        self.run_time += dt;
        self.message_timer = (self.message_timer - dt).max(0.);
        for e in &mut self.effects {
            e.age += dt;
        }
        self.effects.retain(|e| e.age < e.duration);
        self.move_player(dt, input);
        self.spawn_wave(dt);
        self.move_enemies(dt);
        self.move_bullets(dt);
        self.collect_dead();
        self.move_pickups(dt);
        if self.player.hp <= 0. {
            self.phase = Phase::Defeat;
            self.say("SIGNAL LOST / THE YARD REMEMBERS".into());
            return;
        }
        if self.wave_remaining == 0 && self.enemies.is_empty() {
            if self.wave == self.max_waves {
                self.phase = Phase::Victory;
                self.score += 10000;
                self.say("YARD RECLAIMED / ALL MACHINES SILENCED".into());
                return;
            }
            if !self.next_wave {
                self.next_wave = true;
                self.pending_upgrades += 1;
                self.player.hp = (self.player.hp + 15.).min(self.player.max_hp);
                self.bullets.retain(|b| !b.hostile);
                let xp = self
                    .pickups
                    .iter()
                    .filter(|p| p.kind == PickupKind::Xp)
                    .map(|p| p.value)
                    .sum::<f32>();
                self.xp += xp * self.player.stats.xp_multiplier;
                self.pickups.retain(|p| p.kind != PickupKind::Xp);
                self.say(format!(
                    "WAVE {} CLEAR / repair +15, choose a salvage upgrade",
                    self.wave
                ));
            }
        }
        while self.xp >= self.xp_to_next {
            self.xp -= self.xp_to_next;
            self.level += 1;
            self.xp_to_next = 35. + (self.level - 1) as f32 * 19.;
            self.pending_upgrades += 1;
            self.effect(EffectKind::LevelUp, self.player.pos, 3., 0.6);
        }
        if self.pending_upgrades > 0 {
            self.offer();
        }
    }
    fn move_player(&mut self, dt: f32, input: Input) {
        let movement = if input.movement.is_finite() {
            input.movement.clamp_length_max(1.)
        } else {
            Vec2::ZERO
        };
        if input.aim.is_finite() && input.aim.length_squared() > 0.01 {
            self.player.aim = input.aim.normalize();
        }
        self.player.dash_cooldown = (self.player.dash_cooldown - dt).max(0.);
        self.player.invulnerable = (self.player.invulnerable - dt).max(0.);
        self.player.dashing = (self.player.dashing - dt).max(0.);
        self.player.overdrive = (self.player.overdrive - dt).max(0.);
        self.player.magnet = (self.player.magnet - dt).max(0.);
        self.player.hurt_timer = (self.player.hurt_timer - dt).max(0.);
        if input.dash && self.player.dash_cooldown == 0. {
            self.player.dash_dir = if movement.length_squared() > 0.01 {
                movement.normalize()
            } else {
                self.player.aim
            };
            self.player.dashing = 0.18;
            self.player.invulnerable =
                0.3 + self.upgrade_rank(UpgradeKind::DashCooldown) as f32 * 0.04;
            self.player.dash_cooldown = self.player.stats.dash_recharge;
            self.effect(EffectKind::Dash, self.player.pos, 1.5, 0.35);
        }
        let velocity = if self.player.dashing > 0. {
            self.player.dash_dir * 28.
        } else {
            movement * self.player.stats.speed
        };
        self.player.pos = (self.player.pos + velocity * dt).clamp(
            Vec2::splat(-ARENA_HALF + 0.65),
            Vec2::splat(ARENA_HALF - 0.65),
        );
        self.player.hp = (self.player.hp + self.player.stats.regen * dt).min(self.player.max_hp);
        if self.player.hurt_timer == 0. {
            self.player.shield = (self.player.shield + 8. * dt)
                .min(self.player.stats.shield_max.max(self.player.shield));
        }
        self.player.fire_timer -= dt;
        if input.fire && self.player.fire_timer <= 0. {
            let count = self.player.stats.multishot;
            let overdrive = if self.player.overdrive > 0. { 1.8 } else { 1. };
            self.player.fire_timer = 1. / (self.player.stats.fire_rate * overdrive);
            for i in 0..count {
                let angle = (i as f32 - (count - 1) as f32 / 2.) * 0.105;
                let dir = rotate(self.player.aim, angle);
                let damage = self.player.stats.damage
                    * if self.random() < self.player.stats.crit {
                        2.
                    } else {
                        1.
                    };
                self.bullet(
                    self.player.pos + dir * 0.85,
                    dir,
                    false,
                    damage,
                    42.,
                    self.player.stats.pierce,
                );
            }
            self.effect(
                EffectKind::Muzzle,
                self.player.pos + self.player.aim * 1.1,
                0.5,
                4. / 30.,
            );
        }
    }
    fn spawn_wave(&mut self, dt: f32) {
        self.wave_timer -= dt;
        if self.wave_remaining == 0 || self.wave_timer > 0. || self.enemies.len() >= MAX_ENEMIES {
            return;
        }
        // Pulses alternate between two distant edges. The other approaches stay open.
        if self.burst_remaining == 0 {
            self.burst_size = (3 + self.wave * 9 / 10).min(self.wave_remaining);
            self.burst_remaining = self.burst_size;
            let centres = [
                Vec2::new(0., -21.),
                Vec2::new(21., 0.),
                Vec2::new(0., 21.),
                Vec2::new(-21., 0.),
            ];
            let mut edges = [0_u32, 1, 2, 3];
            edges.sort_by(|a, b| {
                centres[*b as usize]
                    .distance_squared(self.player.pos)
                    .total_cmp(&centres[*a as usize].distance_squared(self.player.pos))
            });
            self.burst_edges = [edges[0], edges[1]];
        }
        self.burst_remaining -= 1;
        self.wave_timer = if self.burst_remaining > 0 {
            0.085
        } else {
            (0.78 - self.wave as f32 * 0.022).max(0.35) * self.burst_size as f32
                - 0.085 * (self.burst_size - 1) as f32
        };
        let kind = if self.wave == self.max_waves && !self.boss_spawned && self.wave_remaining == 1
        {
            self.boss_spawned = true;
            EnemyKind::Boss
        } else {
            let r = self.random();
            if self.wave >= 4 && r < 0.12 {
                EnemyKind::Tank
            } else if self.wave >= 3 && r < 0.27 {
                EnemyKind::Bomber
            } else if self.wave >= 2 && r < 0.49 {
                EnemyKind::Shooter
            } else {
                EnemyKind::Rusher
            }
        };
        let edge = self.burst_edges[(self.burst_remaining % 2) as usize];
        let along = (self.random() * 2. - 1.) * (ARENA_HALF - 1.);
        let mut pos = match edge {
            0 => Vec2::new(along, -21.),
            1 => Vec2::new(21., along),
            2 => Vec2::new(along, 21.),
            _ => Vec2::new(-21., along),
        };
        if pos.distance(self.player.pos) < 11. {
            pos = -pos;
        }
        self.spawn(kind, pos);
        if kind == EnemyKind::Boss {
            self.say("FOREMAN ONLINE / dismantle the core".into());
        }
        self.wave_remaining -= 1;
    }
    fn spawn(&mut self, kind: EnemyKind, pos: Vec2) {
        if self.enemies.len() >= MAX_ENEMIES {
            return;
        }
        let scale = 1. + self.wave as f32 * 0.13;
        let (hp, radius) = match kind {
            EnemyKind::Rusher => (28. * scale, 0.65),
            EnemyKind::Shooter => (38. * scale, 0.75),
            EnemyKind::Tank => (200. * scale, 1.15),
            EnemyKind::Bomber => (24. * scale, 0.62),
            EnemyKind::Boss => (80000., 2.),
        };
        let id = self.id();
        let attack_timer = 1.5 + self.random();
        self.enemies.push(Enemy {
            id,
            kind,
            pos,
            hp,
            max_hp: hp,
            radius,
            hit_flash: 0.,
            attack_timer,
            aim: Vec2::Y,
            telegraph: 0.,
            spawn_timer: 0.9,
            charge: 0.,
        });
        self.effect(EffectKind::Spawn, pos, radius + 0.5, 0.9);
    }
    fn move_enemies(&mut self, dt: f32) {
        let mut shots = Vec::new();
        let mut explosions = Vec::new();
        let mut contact_damage = 0.;
        for e in &mut self.enemies {
            e.hit_flash = (e.hit_flash - dt).max(0.);
            e.spawn_timer = (e.spawn_timer - dt).max(0.);
            if e.hp <= 0. || e.spawn_timer > 0. {
                continue;
            }
            let delta = self.player.pos - e.pos;
            let distance = delta.length();
            let dir = delta.normalize_or_zero();
            if e.telegraph <= 0. && e.charge <= 0. {
                e.aim = dir;
            }
            e.attack_timer -= dt;
            if e.telegraph > 0. {
                e.telegraph -= dt;
                if e.telegraph <= 0. {
                    match e.kind {
                        EnemyKind::Shooter => shots.push((e.pos + e.aim, e.aim, 11., 14.)),
                        EnemyKind::Bomber => {
                            explosions.push(e.pos);
                            e.hp = 0.;
                        }
                        EnemyKind::Tank => e.charge = 0.55,
                        EnemyKind::Boss => {
                            let spokes = if e.hp < e.max_hp * 0.5 { 24 } else { 16 };
                            for i in 0..spokes {
                                let a = i as f32 * std::f32::consts::TAU / spokes as f32
                                    + self.run_time * 0.37;
                                shots.push((e.pos, Vec2::new(a.cos(), a.sin()), 8.5, 16.));
                            }
                            for angle in [-0.24, 0., 0.24] {
                                shots.push((e.pos, rotate(e.aim, angle), 15., 22.));
                            }
                        }
                        EnemyKind::Rusher => {}
                    }
                    e.attack_timer = match e.kind {
                        EnemyKind::Boss => {
                            if e.hp < e.max_hp * 0.5 {
                                1.9
                            } else {
                                2.7
                            }
                        }
                        EnemyKind::Shooter => 2.3,
                        EnemyKind::Tank => 3.8,
                        _ => 1.,
                    };
                }
            } else if e.attack_timer <= 0. {
                let telegraph = match e.kind {
                    EnemyKind::Shooter if distance < 24. => 0.8,
                    EnemyKind::Bomber if distance < 3. => 0.85,
                    EnemyKind::Tank if distance < 12. => 0.9,
                    EnemyKind::Boss => {
                        if e.hp < e.max_hp * 0.5 {
                            0.8
                        } else {
                            1.1
                        }
                    }
                    _ => 0.,
                };
                if telegraph > 0. {
                    e.telegraph = telegraph;
                    e.aim = dir;
                }
            }
            let speed = match e.kind {
                EnemyKind::Rusher => 2.8 + self.wave as f32 * 0.09,
                EnemyKind::Shooter => 2.3,
                EnemyKind::Tank => 1.8,
                EnemyKind::Bomber => 4.,
                EnemyKind::Boss => {
                    if e.hp < e.max_hp * 0.5 {
                        1.9
                    } else {
                        1.35
                    }
                }
            };
            let velocity = if e.charge > 0. {
                e.charge -= dt;
                e.aim * 14.
            } else if e.telegraph > 0. {
                Vec2::ZERO
            } else if e.kind == EnemyKind::Shooter && distance < 11. {
                -dir * speed * 0.6
            } else if e.kind == EnemyKind::Shooter && distance < 14. {
                Vec2::new(-dir.y, dir.x) * speed * 0.7
            } else {
                dir * speed
            };
            e.pos = (e.pos + velocity * dt).clamp(Vec2::splat(-21.), Vec2::splat(21.));
            if distance < e.radius + 0.55 {
                contact_damage = f32::max(
                    contact_damage,
                    match e.kind {
                        EnemyKind::Boss => 30.,
                        EnemyKind::Tank => 22.,
                        _ => 12.,
                    },
                );
            }
        }
        // Local crowd separation keeps packs readable without changing collision authority.
        for i in 0..self.enemies.len() {
            for j in i + 1..self.enemies.len() {
                let (a, b) = self.enemies.split_at_mut(j);
                let a = &mut a[i];
                let b = &mut b[0];
                let delta = b.pos - a.pos;
                let min = (a.radius + b.radius) * 0.82;
                let d2 = delta.length_squared();
                if d2 < min * min && d2 > 0.0001 {
                    let d = d2.sqrt();
                    let push = delta / d * (min - d) * 0.12;
                    a.pos = (a.pos - push).clamp(Vec2::splat(-21.), Vec2::splat(21.));
                    b.pos = (b.pos + push).clamp(Vec2::splat(-21.), Vec2::splat(21.));
                }
            }
        }
        for (pos, dir, speed, damage) in shots {
            self.bullet(pos, dir, true, damage, speed, 0);
        }
        for pos in explosions {
            self.effect(EffectKind::Explosion, pos, 3.5, 0.55);
            if pos.distance(self.player.pos) < 3.5 {
                self.hurt(24.);
            }
            for e in &mut self.enemies {
                if e.spawn_timer == 0. && e.pos.distance(pos) < 3.5 {
                    e.hp -= 40.;
                    e.hit_flash = 0.15;
                }
            }
        }
        if contact_damage > 0. {
            self.hurt(contact_damage);
        }
    }
    fn hurt(&mut self, damage: f32) {
        if self.player.invulnerable > 0. {
            return;
        }
        let absorbed = self.player.shield.min(damage);
        self.player.shield -= absorbed;
        self.player.hp -= damage - absorbed;
        self.player.invulnerable = 0.65;
        self.player.hurt_timer = 5.;
        self.effect(EffectKind::Hit, self.player.pos, 1., 0.2);
    }
    fn bullet(
        &mut self,
        pos: Vec2,
        dir: Vec2,
        hostile: bool,
        damage: f32,
        speed: f32,
        pierce: u32,
    ) {
        if self.bullets.len() >= MAX_BULLETS {
            return;
        }
        let id = self.id();
        self.bullets.push(Bullet {
            id,
            pos,
            dir,
            radius: if hostile { 0.22 } else { 0.13 },
            hostile,
            life: if hostile { 6. } else { 1.5 },
            damage,
            speed,
            pierce,
            hit_ids: Vec::new(),
        });
    }
    fn move_bullets(&mut self, dt: f32) {
        let mut hits = Vec::new();
        let mut hurt = 0_f32;
        let mut blasts = Vec::new();
        for b in &mut self.bullets {
            b.life -= dt;
            let old = b.pos;
            b.pos += b.dir * b.speed * dt;
            if b.hostile {
                if segment_hits(old, b.pos, self.player.pos, b.radius + 0.55) {
                    hurt = hurt.max(b.damage);
                    b.life = 0.;
                }
            } else {
                let mut targets: Vec<_> = self
                    .enemies
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.hp > 0. && e.spawn_timer == 0. && !b.hit_ids.contains(&e.id))
                    .filter_map(|(i, e)| {
                        segment_hit_time(old, b.pos, e.pos, e.radius + b.radius).map(|t| (i, t))
                    })
                    .collect();
                targets.sort_by(|a, b| a.1.total_cmp(&b.1));
                for (i, _) in targets {
                    let e = &mut self.enemies[i];
                    e.hp -= b.damage;
                    e.hit_flash = 0.12;
                    hits.push(e.pos);
                    b.hit_ids.push(e.id);
                    if self.player.stats.blast > 0 {
                        blasts.push((
                            e.pos,
                            b.damage * 0.25 * self.player.stats.blast as f32,
                            e.id,
                        ));
                    }
                    if b.pierce == 0 {
                        b.life = 0.;
                        break;
                    }
                    b.pierce -= 1;
                }
            }
            if b.pos.abs().max_element() > ARENA_HALF + 2. {
                b.life = 0.;
            }
        }
        self.bullets.retain(|b| b.life > 0.);
        for pos in hits {
            self.effect(EffectKind::Hit, pos, 0.45, 0.20);
        }
        for (pos, damage, original) in blasts {
            self.effect(EffectKind::Explosion, pos, 1.5, 0.25);
            for e in &mut self.enemies {
                if e.id != original && e.spawn_timer == 0. && e.pos.distance_squared(pos) < 2.25 {
                    e.hp -= damage;
                    e.hit_flash = 0.12;
                }
            }
        }
        if hurt > 0. {
            self.hurt(hurt);
        }
    }
    fn collect_dead(&mut self) {
        let dead: Vec<_> = self
            .enemies
            .iter()
            .filter(|e| e.hp <= 0.)
            .map(|e| (e.pos, e.kind, e.radius))
            .collect();
        self.enemies.retain(|e| e.hp > 0.);
        for (pos, kind, radius) in dead {
            self.kills += 1;
            let value = match kind {
                EnemyKind::Boss => 100.,
                EnemyKind::Tank => 16.,
                EnemyKind::Shooter => 8.,
                _ => 5.,
            };
            self.score += (value * 10.) as u32;
            self.player.hp =
                (self.player.hp + self.player.stats.life_steal).min(self.player.max_hp);
            self.effect(EffectKind::Explosion, pos, radius * 1.8, 0.4);
            self.pickup(PickupKind::Xp, pos, value);
            let roll = self.random();
            if roll < 0.04 {
                self.pickup(PickupKind::Health, pos, 20.);
            } else if roll < 0.065 {
                self.pickup(PickupKind::Overdrive, pos, 10.);
            } else if roll < 0.08 {
                self.pickup(PickupKind::Magnet, pos, 8.);
            } else if roll < 0.095 {
                self.pickup(PickupKind::Shield, pos, 25.);
            }
        }
    }
    fn pickup(&mut self, kind: PickupKind, pos: Vec2, value: f32) {
        if self.pickups.len() >= MAX_PICKUPS {
            if kind == PickupKind::Xp {
                self.xp += value * self.player.stats.xp_multiplier;
            }
            return;
        }
        let id = self.id();
        self.pickups.push(Pickup {
            id,
            kind,
            pos,
            radius: if kind == PickupKind::Xp { 0.25 } else { 0.5 },
            age: 0.,
            value,
        });
    }
    fn move_pickups(&mut self, dt: f32) {
        let mut collected = Vec::new();
        for p in &mut self.pickups {
            p.age += dt;
            let delta = self.player.pos - p.pos;
            let range = if self.player.magnet > 0. || p.age > 18. {
                100.
            } else {
                self.player.stats.pickup_range
            };
            if delta.length_squared() < range * range {
                p.pos += delta.normalize_or_zero() * (14. * dt).min(delta.length());
            }
            if p.pos.distance_squared(self.player.pos) < (0.55 + p.radius).powi(2) {
                collected.push((p.kind, p.value));
                p.age = -1.;
            }
        }
        self.pickups.retain(|p| p.age >= 0.);
        for (kind, value) in collected {
            match kind {
                PickupKind::Xp => self.xp += value * self.player.stats.xp_multiplier,
                PickupKind::Health => {
                    self.player.hp = (self.player.hp + value).min(self.player.max_hp);
                    self.effect(EffectKind::Heal, self.player.pos, 1.5, 0.45);
                }
                PickupKind::Overdrive => {
                    self.player.overdrive = 12.;
                    self.say("OVERDRIVE / fire speed +80% for 12 seconds".into());
                }
                PickupKind::Magnet => {
                    self.player.magnet = 8.;
                    self.say("MAGNET SURGE / all scrap inbound".into());
                }
                PickupKind::Shield => {
                    self.player.shield =
                        (self.player.shield + value).min(self.player.stats.shield_max.max(50.));
                    self.say("BARRIER +25".into());
                }
            }
        }
    }
}
fn rotate(v: Vec2, a: f32) -> Vec2 {
    Vec2::new(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos())
}
fn segment_hits(a: Vec2, b: Vec2, p: Vec2, r: f32) -> bool {
    segment_hit_time(a, b, p, r).is_some()
}
/// First contact with the expanded collision circle, including an origin inside it.
fn segment_hit_time(start: Vec2, end: Vec2, centre: Vec2, radius: f32) -> Option<f32> {
    let offset = start - centre;
    let outside = offset.length_squared() - radius * radius;
    if outside <= 0. {
        return Some(0.);
    }
    let displacement = end - start;
    let length_squared = displacement.length_squared();
    if length_squared < 0.000_001 {
        return None;
    }
    let projection = offset.dot(displacement);
    let discriminant = projection * projection - length_squared * outside;
    if discriminant < 0. {
        return None;
    }
    let contact_time = (-projection - discriminant.sqrt()) / length_squared;
    (0.0..=1.0).contains(&contact_time).then_some(contact_time)
}

#[cfg(test)]
// Exact values verify frozen state, unchanged health, and integer-valued rewards.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    fn running() -> Sim {
        let mut s = Sim::new(42);
        s.start_run();
        s
    }
    #[test]
    fn swept_collision_catches_fast_rounds() {
        assert!(segment_hits(
            Vec2::ZERO,
            Vec2::new(100., 0.),
            Vec2::new(50., 0.),
            0.3
        ));
        assert!(!segment_hits(Vec2::ZERO, Vec2::X, Vec2::new(3., 0.), 0.3));
        let mut s = running();
        s.spawn(EnemyKind::Rusher, Vec2::new(1., 0.));
        s.enemies[0].spawn_timer = 0.;
        s.bullet(Vec2::ZERO, Vec2::X, false, 100., 200., 0);
        s.move_bullets(0.02);
        assert!(s.enemies[0].hp <= 0.);
    }
    #[test]
    fn piercing_hits_each_target_once() {
        let mut s = running();
        for x in [1., 2.] {
            s.spawn(EnemyKind::Tank, Vec2::new(x, 0.));
        }
        for e in &mut s.enemies {
            e.spawn_timer = 0.;
        }
        let hp = s.enemies[0].hp;
        s.bullet(Vec2::ZERO, Vec2::X, false, 10., 10., 2);
        for _ in 0..30 {
            s.move_bullets(0.01);
        }
        assert_eq!(s.enemies[0].hp, hp - 10.);
        assert_eq!(s.enemies[1].hp, hp - 10.);
    }
    #[test]
    fn pause_freezes_and_restores_upgrade() {
        let mut s = running();
        s.toggle_pause();
        s.step(
            1.,
            Input {
                fire: true,
                ..Default::default()
            },
        );
        assert_eq!(s.run_time, 0.);
        s.toggle_pause();
        s.pending_upgrades = 1;
        s.offer();
        let choices = s.choices.clone();
        s.toggle_pause();
        s.toggle_pause();
        assert_eq!(s.phase, Phase::Upgrade);
        assert_eq!(choices, s.choices);
    }
    #[test]
    fn reset_discards_entire_run() {
        let mut s = running();
        s.score = 88;
        s.player.hp = 4.;
        s.level = 12;
        s.start_run();
        assert_eq!(s.wave, 1);
        assert_eq!(s.level, 1);
        assert_eq!(s.player.hp, 100.);
        assert_eq!(s.score, 0);
        assert!(s.enemies.is_empty());
    }
    #[test]
    fn dash_has_immunity_and_cooldown() {
        let mut s = running();
        s.player.invulnerable = 0.;
        s.step(
            0.016,
            Input {
                movement: Vec2::X,
                dash: true,
                ..Default::default()
            },
        );
        s.hurt(40.);
        assert_eq!(s.player.hp, 100.);
        let cooldown = s.player.dash_cooldown;
        s.step(
            0.016,
            Input {
                dash: true,
                ..Default::default()
            },
        );
        assert!(s.player.dash_cooldown < cooldown);
    }
    #[test]
    fn upgrades_are_distinct_capped_and_diverse() {
        let mut s = running();
        let mut seen = [false; 14];
        for _ in 0..100 {
            s.pending_upgrades = 1;
            s.offer();
            if s.phase != Phase::Upgrade {
                break;
            }
            for (i, k) in s.choices.iter().enumerate() {
                assert!(!s.choices[..i].contains(k));
                seen[*k as usize] = true;
            }
            s.choose_upgrade(0);
        }
        assert!(seen.into_iter().all(|v| v));
        for k in UpgradeKind::ALL {
            assert!(s.upgrade_rank(k) <= k.cap());
        }
        assert!(s.player.stats.multishot <= 4);
    }
    #[test]
    fn overflow_and_distant_scrap_cannot_softlock() {
        let mut s = running();
        for _ in 0..MAX_PICKUPS {
            s.pickup(PickupKind::Xp, Vec2::new(20., 20.), 1.);
        }
        s.pickup(PickupKind::Xp, Vec2::ZERO, 7.);
        assert_eq!(s.xp, 7.);
        for _ in 0..1400 {
            s.move_pickups(1. / 60.);
        }
        assert!(s.pickups.is_empty());
        assert_eq!(s.xp, 247.);
    }
    #[test]
    fn hard_caps_bound_every_collection() {
        let mut s = running();
        for _ in 0..1000 {
            s.spawn(EnemyKind::Rusher, Vec2::ZERO);
            s.bullet(Vec2::ZERO, Vec2::X, false, 1., 1., 0);
            s.effect(EffectKind::Hit, Vec2::ZERO, 1., 1.);
            s.pickup(PickupKind::Health, Vec2::ZERO, 1.);
        }
        assert_eq!(s.enemies.len(), MAX_ENEMIES);
        assert_eq!(s.bullets.len(), MAX_BULLETS);
        assert_eq!(s.effects.len(), MAX_EFFECTS);
        assert_eq!(s.pickups.len(), MAX_PICKUPS);
    }
    #[test]
    fn deterministic_complete_run_and_loss() {
        fn complete() -> (u32, u32, u32) {
            let mut s = running();
            for _ in 0..10000 {
                match s.phase {
                    Phase::Playing => {
                        s.wave_timer = 0.;
                        for e in &mut s.enemies {
                            e.hp = 0.;
                        }
                        s.step(0.016, Input::default());
                    }
                    Phase::Upgrade => s.choose_upgrade(0),
                    Phase::Victory => return (s.score, s.kills, s.level),
                    _ => panic!("unexpected phase"),
                }
            }
            panic!("run did not complete")
        }
        assert_eq!(complete(), complete());
        assert_eq!(complete().1, (1u32..=10).map(|w| 24 + w * 14).sum::<u32>());
        let mut s = running();
        s.player.invulnerable = 0.;
        s.hurt(200.);
        s.step(0.016, Input::default());
        assert_eq!(s.phase, Phase::Defeat);
    }
    #[test]
    fn spawn_has_warning_and_safe_distance() {
        let mut s = running();
        s.player.pos = Vec2::new(20., 20.);
        for _ in 0..100 {
            s.wave_remaining = 1;
            s.wave_timer = 0.;
            s.spawn_wave(0.01);
            let e = s.enemies.last().expect("the test spawned an enemy");
            assert!(e.pos.distance(s.player.pos) > 10.);
            assert!(e.spawn_timer > 0.);
        }
    }
    #[test]
    fn nonfinite_input_cannot_poison_world() {
        let mut s = running();
        s.step(f32::NAN, Input::default());
        s.step(
            0.016,
            Input {
                movement: Vec2::splat(f32::NAN),
                aim: Vec2::splat(f32::INFINITY),
                ..Default::default()
            },
        );
        assert!(s.player.pos.is_finite());
        assert!(s.player.aim.is_finite());
    }
    #[test]
    fn bot_balance_run() {
        for (seed, optimized) in [
            (3, true),
            (42, true),
            (731, true),
            (17, false),
            (111, false),
        ] {
            let mut min_hp = 100_f32;
            let mut boss_start = None;
            let mut peak_enemies = 0;
            let mut s = Sim::new(seed);
            s.start_run();
            for _ in 0..60000 {
                min_hp = min_hp.min(s.player.hp);
                peak_enemies = peak_enemies.max(s.enemies.len());
                if boss_start.is_none() && s.enemies.iter().any(|e| e.kind == EnemyKind::Boss) {
                    boss_start = Some(s.run_time);
                }
                match s.phase {
                    Phase::Upgrade => {
                        let priority = [
                            UpgradeKind::Multishot,
                            UpgradeKind::Damage,
                            UpgradeKind::Piercing,
                            UpgradeKind::FireRate,
                            UpgradeKind::Regeneration,
                            UpgradeKind::Shield,
                            UpgradeKind::BlastRounds,
                            UpgradeKind::LifeSteal,
                            UpgradeKind::MoveSpeed,
                            UpgradeKind::Critical,
                            UpgradeKind::DashCooldown,
                            UpgradeKind::MaxHealth,
                            UpgradeKind::Magnet,
                            UpgradeKind::Overclock,
                        ];
                        let preferred = priority
                            .iter()
                            .find_map(|kind| s.choices.iter().position(|k| k == kind))
                            .unwrap_or(0);
                        let index = if optimized {
                            preferred
                        } else {
                            s.level as usize % s.choices.len()
                        };
                        s.choose_upgrade(index);
                    }
                    Phase::Playing => {
                        let nearest = s.enemies.iter().min_by(|a, b| {
                            a.pos
                                .distance_squared(s.player.pos)
                                .total_cmp(&b.pos.distance_squared(s.player.pos))
                        });
                        let aim =
                            nearest.map_or(Vec2::X, |e| (e.pos - s.player.pos).normalize_or_zero());
                        let danger = nearest.is_some_and(|e| e.pos.distance(s.player.pos) < 4.);
                        let radial = s.player.pos.normalize_or_zero();
                        let tangent = Vec2::new(-radial.y, radial.x);
                        let mut movement = tangent + radial * (13. - s.player.pos.length()) * 0.4;
                        if s.player.pos.length() < 1. {
                            movement = Vec2::X;
                        }
                        if danger {
                            movement -= aim * 1.5;
                        }
                        for b in &s.bullets {
                            if b.hostile && b.pos.distance_squared(s.player.pos) < 16. {
                                movement += (s.player.pos - b.pos).normalize_or_zero() * 0.7;
                            }
                        }
                        s.step(
                            1. / 60.,
                            Input {
                                movement,
                                aim,
                                fire: true,
                                dash: danger,
                            },
                        );
                    }
                    _ => break,
                }
            }
            assert_eq!(s.phase, Phase::Victory, "seed {seed} no longer finishes");
            assert_eq!(s.kills, 1010);
            assert!((600.0..900.0).contains(&s.run_time));
            assert!(peak_enemies <= MAX_ENEMIES);
            assert!(s.player.hp > 0.);
            println!(
                "seed={seed} optimized={optimized} phase={:?} wave={} time={:.1} kills={} level={} hp={:.1}",
                s.phase, s.wave, s.run_time, s.kills, s.level, s.player.hp
            );
            println!(
                "min_hp={min_hp:.1} peak_enemies={peak_enemies} boss_duration={:.1} damage={:.1} rate={:.1} pellets={}",
                boss_start.map_or(0., |t| s.run_time - t),
                s.player.stats.damage,
                s.player.stats.fire_rate,
                s.player.stats.multishot
            );
        }
    }
    #[test]
    fn foreman_is_final_spawn_and_victory_requires_clear() {
        let mut s = running();
        s.wave = s.max_waves;
        s.wave_remaining = 2;
        s.wave_timer = 0.;
        s.spawn_wave(0.016);
        assert_ne!(s.enemies[0].kind, EnemyKind::Boss);
        s.wave_timer = 0.;
        s.spawn_wave(0.016);
        assert_eq!(
            s.enemies.last().expect("the test spawned an enemy").kind,
            EnemyKind::Boss
        );
        s.tick(0.016, Input::default());
        assert_eq!(s.phase, Phase::Playing);
        for enemy in &mut s.enemies {
            enemy.hp = 0.;
        }
        s.tick(0.016, Input::default());
        assert_eq!(s.phase, Phase::Victory);
    }
    #[test]
    fn shooter_locks_aim_and_warns_before_firing() {
        let mut s = running();
        s.spawn(EnemyKind::Shooter, Vec2::new(10., 0.));
        s.enemies[0].spawn_timer = 0.;
        s.enemies[0].attack_timer = 0.;
        s.move_enemies(0.016);
        assert!(s.enemies[0].telegraph > 0.);
        assert!(s.bullets.is_empty());
        let aim = s.enemies[0].aim;
        s.player.pos = Vec2::new(0., 10.);
        for _ in 0..55 {
            s.move_enemies(0.016);
        }
        assert!(!s.bullets.is_empty());
        assert_eq!(s.bullets[0].dir, aim);
    }
    #[test]
    fn idle_player_loses_to_actual_enemies() {
        let mut s = running();
        for _ in 0..6000 {
            s.step(1. / 60., Input::default());
            if s.phase == Phase::Defeat {
                break;
            }
        }
        assert_eq!(s.phase, Phase::Defeat);
        assert_eq!(s.wave, 1);
        assert_eq!(s.kills, 0);
    }
    #[test]
    fn horde_pulse_uses_two_approaches_then_leaves_a_lull() {
        let mut s = running();
        s.wave = 8;
        s.wave_remaining = 50;
        s.player.pos = Vec2::new(4., -3.);
        for _ in 0..10 {
            s.wave_timer = 0.;
            s.spawn_wave(0.016);
        }
        let edges: std::collections::BTreeSet<_> = s
            .enemies
            .iter()
            .map(|e| {
                if e.pos.x == 21. {
                    1
                } else if e.pos.x == -21. {
                    3
                } else if e.pos.y == 21. {
                    2
                } else {
                    0
                }
            })
            .collect();
        assert_eq!(edges.len(), 2);
        assert_eq!(s.enemies.len(), 10);
        assert!(s.wave_timer > 3.);
        assert!(s.enemies.iter().all(|e| e.pos.distance(s.player.pos) > 11.));
    }
    #[test]
    fn swept_round_hits_front_surface_before_nearer_centre() {
        let mut s = running();
        s.spawn(EnemyKind::Rusher, Vec2::new(2.7, 0.));
        s.spawn(EnemyKind::Tank, Vec2::new(3., 0.));
        for e in &mut s.enemies {
            e.spawn_timer = 0.;
        }
        let rusher_hp = s.enemies[0].hp;
        let tank_hp = s.enemies[1].hp;
        s.bullet(Vec2::new(1.5, 0.), Vec2::X, false, 10., 42., 0);
        s.move_bullets(1. / 60.);
        assert_eq!(s.enemies[0].hp, rusher_hp);
        assert_eq!(s.enemies[1].hp, tank_hp - 10.);
    }
    #[test]
    fn blast_does_not_damage_a_machine_during_its_spawn_warning() {
        let mut s = running();
        s.player.stats.blast = 1;
        s.spawn(EnemyKind::Rusher, Vec2::new(1., 0.));
        s.spawn(EnemyKind::Rusher, Vec2::new(1., 0.5));
        s.enemies[0].spawn_timer = 0.;
        let hp = s.enemies[1].hp;
        s.bullet(Vec2::ZERO, Vec2::X, false, 20., 42., 0);
        s.move_bullets(1. / 60.);
        assert!(s.enemies[0].hp < hp);
        assert_eq!(s.enemies[1].hp, hp);
    }
    #[test]
    fn wave_clear_scrap_choices_finish_before_next_wave_starts() {
        let mut s = running();
        s.wave_remaining = 0;
        s.pickup(PickupKind::Xp, Vec2::new(20., 20.), 35.);
        s.tick(1. / 60., Input::default());
        assert_eq!(s.phase, Phase::Upgrade);
        assert_eq!(s.level, 2);
        assert_eq!(s.pending_upgrades, 2);
        s.choose_upgrade(0);
        assert_eq!(s.phase, Phase::Upgrade);
        assert_eq!(s.wave, 1);
        s.choose_upgrade(0);
        assert_eq!(s.phase, Phase::Playing);
        assert_eq!(s.wave, 2);
    }
    #[test]
    fn invalid_and_paused_upgrade_actions_are_inert() {
        let mut s = running();
        s.pending_upgrades = 1;
        s.offer();
        let choices = s.choices.clone();
        let ranks = s.ranks;
        let rng = s.rng;
        s.choose_upgrade(99);
        assert_eq!(s.choices, choices);
        assert_eq!(s.ranks, ranks);
        assert_eq!(s.rng, rng);
        assert_eq!(s.phase, Phase::Upgrade);
        s.toggle_pause();
        s.choose_upgrade(0);
        assert_eq!(s.ranks, ranks);
        assert_eq!(s.pending_upgrades, 1);
        s.toggle_pause();
        s.choose_upgrade(0);
        assert_eq!(s.phase, Phase::Playing);
    }
    #[test]
    fn showcase_is_explicit_bounded_and_cleared_by_normal_restart() {
        for boss in [false, true] {
            let mut preview = Sim::new(42);
            assert_eq!(preview.phase, Phase::Title);
            assert!(preview.enemies.is_empty());
            preview.prepare_showcase(boss);
            assert_eq!(preview.enemies.len(), if boss { 5 } else { 18 });
            assert_eq!(preview.wave_remaining, 0);
            assert!(preview.wave_timer > 1000.);
            assert!(preview.player.hp > 100.);
            assert!(
                preview
                    .enemies
                    .iter()
                    .all(|e| e.pos.abs().max_element() < ARENA_HALF)
            );
            if boss {
                assert!(preview.enemies[0].hp < preview.enemies[0].max_hp * 0.5);
            }
            preview.start_run();
            let normal = running();
            assert_eq!(preview.phase, normal.phase);
            assert_eq!(preview.wave, normal.wave);
            assert_eq!(preview.wave_remaining, normal.wave_remaining);
            assert_eq!(preview.player.hp, normal.player.hp);
            assert_eq!(preview.player.max_hp, normal.player.max_hp);
            assert_eq!(preview.player.invulnerable, normal.player.invulnerable);
            assert!(preview.enemies.is_empty());
            assert_eq!(preview.rng, normal.rng);
        }
    }
}
