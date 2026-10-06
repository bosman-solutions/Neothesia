//! Juiced glow FX. Default look: gold keyline, flat white-hot flares at struck
//! keys, and fine glitter dust that drifts up and lingers. Sparks, embers,
//! beams and smoke are still here, gated off by their knobs.
//!
//! Public API is identical to upstream (`new / clear / push / prepare / render`)
//! so scenes need no changes. Note-on edges are detected here by diffing
//! which keys were pushed this frame vs. last frame.

use std::f32::consts::PI;
use std::time::{Duration, Instant};

use wgpu_jumpstart::{Color, Gpu, TransformUniform, Uniform};

use super::{GlowInstance, GlowPipeline, kind};

// ---- tuning knobs ---------------------------------------------------------
const MAX_PARTICLES: usize = 20_000;

// Keyline: always-on glowing edge along the top of the keyboard.
const KEYLINE: bool = true;
const KEYLINE_COLOR: [f32; 3] = [1.0, 0.62, 0.18]; // linear warm gold
const KEYLINE_INTENSITY: f32 = 0.9;
const KEYLINE_REACH: f32 = 90.0; // px, how far the glow climbs

// Flare: flat white-hot bloom where a key is struck.
const FLARE_W: f32 = 5.0; // x key width
const FLARE_H: f32 = 60.0; // px
const FLASH_DECAY: f32 = 5.0; // note-on flash falloff, 1/s

// Dust: fine glitter, the star of the show.
const DUST_BURST: usize = 45; // per note-on
const DUST_PER_SEC: f32 = 50.0; // per held key
const DUST_SWIRL: f32 = 140.0; // lateral wander, px/s^2
const DUST_WARMTH: f32 = 0.55; // 0 = note color, 1 = keyline gold

// Parked effects (set > 0 / true to bring back).
const BURST_SPARKS: usize = 0;
const EMBERS_PER_SEC: f32 = 0.0;
const SMOKE_PUFFS: usize = 0;
const SMOKE_PER_SEC: f32 = 0.0;
const BEAMS: bool = false;

const GRAVITY: f32 = 1500.0;
const EMBER_LIFT: f32 = -90.0;
const BEAM_HEIGHT: f32 = 320.0;
const SMOKE_START: f32 = 30.0;
const SMOKE_END: f32 = 170.0;
const SMOKE_INTENSITY: f32 = 0.22;
// ---------------------------------------------------------------------------

#[derive(Default, Clone, Copy)]
struct KeyFx {
    held: bool,
    held_prev: bool,
    hold_time: f32,
    dust_acc: f32,
    ember_acc: f32,
    smoke_acc: f32,
    color: [f32; 4],
    x: f32,
    y: f32,
    w: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum PKind {
    Spark,
    Smoke,
    Dust,
}

#[derive(Clone, Copy)]
struct Particle {
    kind: PKind,
    pos: [f32; 2],
    vel: [f32; 2],
    accel: f32,
    drag: f32,
    life: f32,
    max_life: f32,
    size: f32,
    color: [f32; 4],
    seed: f32,
}

/// xorshift32: tiny, deterministic, no extra deps.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

fn mix3(a: [f32; 4], b: [f32; 3], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3],
    ]
}

fn whiten(c: [f32; 4], t: f32) -> [f32; 4] {
    mix3(c, [1.0, 1.0, 1.0], t)
}

fn with_intensity(c: [f32; 4], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}

pub struct GlowRenderer {
    pipeline: GlowPipeline,
    keys: Vec<KeyFx>,
    particles: Vec<Particle>,
    rng: Rng,
    last_frame: Option<Instant>,
    keyline_y: Option<f32>,
    clock: f32,
}

impl GlowRenderer {
    pub fn new(
        gpu: &Gpu,
        transform: &Uniform<TransformUniform>,
        layout: &piano_layout::KeyboardLayout,
    ) -> Self {
        let pipeline = GlowPipeline::new(gpu, transform);
        let keys = vec![KeyFx::default(); layout.range.iter().count()];

        Self {
            pipeline,
            keys,
            particles: Vec::with_capacity(MAX_PARTICLES),
            rng: Rng(0x9E37_79B9),
            last_frame: None,
            keyline_y: None,
            clock: 0.0,
        }
    }

    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        self.pipeline.render(render_pass);
    }

    pub fn clear(&mut self) {
        self.pipeline.clear();
    }

    pub fn push(
        &mut self,
        id: usize,
        color: Color,
        key_x: f32,
        key_y: f32,
        key_w: f32,
        delta: Duration,
    ) {
        self.keyline_y = Some(key_y);

        let Some(k) = self.keys.get_mut(id) else {
            return;
        };

        let note_on = !k.held_prev;
        k.held = true;
        k.color = color.into_linear_rgba();
        k.x = key_x;
        k.y = key_y;
        k.w = key_w;

        if note_on {
            k.hold_time = 0.0;
            k.dust_acc = 0.0;
            k.ember_acc = 0.0;
            k.smoke_acc = 0.0;
        } else {
            k.hold_time += delta.as_secs_f32();
        }

        let k = *k;
        let cx = k.x + k.w / 2.0;
        let flash = (-k.hold_time * FLASH_DECAY).exp();

        if note_on {
            for _ in 0..DUST_BURST {
                self.spawn_dust(&k, true);
            }
            self.spawn_burst(&k);
            for _ in 0..SMOKE_PUFFS {
                self.spawn_smoke(&k);
            }
        }

        let warm = mix3(k.color, KEYLINE_COLOR, 0.6);
        let inst = self.pipeline.instances();

        // Flare: wide soft bloom sitting on the keyline...
        let fw = k.w * FLARE_W * (1.0 + 0.3 * flash);
        let fh = FLARE_H * (1.0 + 0.4 * flash);
        inst.push(GlowInstance {
            position: [cx - fw / 2.0, k.y - fh / 2.0],
            size: [fw, fh],
            color: with_intensity(whiten(warm, 0.3), 0.6 + 0.9 * flash),
            params: [kind::HALO, 0.0, 0.0, 0.0],
        });
        // ...plus a white-hot core.
        let cw = k.w * 1.8;
        let ch = 22.0;
        inst.push(GlowInstance {
            position: [cx - cw / 2.0, k.y - ch / 2.0],
            size: [cw, ch],
            color: with_intensity(whiten(warm, 0.8), 0.9 + 0.8 * flash),
            params: [kind::HALO, 0.0, 0.0, 0.0],
        });

        if BEAMS {
            let bw = k.w * 2.2;
            let bh = BEAM_HEIGHT * (0.75 + 0.5 * flash);
            inst.push(GlowInstance {
                position: [cx - bw / 2.0, k.y - bh],
                size: [bw, bh],
                color: with_intensity(k.color, 0.35 + 0.9 * flash),
                params: [kind::BEAM, 0.0, (id as f32 * 0.618).fract(), 0.0],
            });
        }
    }

    pub fn prepare(&mut self) {
        let now = Instant::now();
        let dt = self
            .last_frame
            .map(|t| (now - t).as_secs_f32())
            .unwrap_or(0.0)
            .min(0.05);
        self.last_frame = Some(now);
        self.clock += dt;

        // Continuous emitters on held keys; roll held state for edge detection.
        for i in 0..self.keys.len() {
            let mut k = self.keys[i];
            if k.held {
                k.dust_acc += DUST_PER_SEC * dt;
                while k.dust_acc >= 1.0 {
                    k.dust_acc -= 1.0;
                    self.spawn_dust(&k, false);
                }
                k.ember_acc += EMBERS_PER_SEC * dt;
                while k.ember_acc >= 1.0 {
                    k.ember_acc -= 1.0;
                    self.spawn_ember(&k);
                }
                k.smoke_acc += SMOKE_PER_SEC * dt;
                while k.smoke_acc >= 1.0 {
                    k.smoke_acc -= 1.0;
                    self.spawn_smoke(&k);
                }
            }
            k.held_prev = k.held;
            k.held = false;
            self.keys[i] = k;
        }

        let clock = self.clock;
        self.particles.retain_mut(|p| {
            p.life -= dt;
            if p.life <= 0.0 {
                return false;
            }
            if p.kind == PKind::Dust {
                // Lazy curl: each mote wanders on its own phase.
                let ph = p.seed * 40.0 + clock * 1.7 + p.pos[1] * 0.012;
                p.vel[0] += ph.sin() * DUST_SWIRL * dt;
            }
            let damp = (-p.drag * dt).exp();
            p.vel[0] *= damp;
            p.vel[1] = p.vel[1] * damp + p.accel * dt;
            p.pos[0] += p.vel[0] * dt;
            p.pos[1] += p.vel[1] * dt;
            true
        });

        let inst = self.pipeline.instances();

        if KEYLINE && let Some(y) = self.keyline_y {
            // Oversized quad; the shader only cares about vertical distance.
            let h = KEYLINE_REACH * 2.0;
            inst.push(GlowInstance {
                position: [-8000.0, y - h / 2.0],
                size: [24000.0, h],
                color: [KEYLINE_COLOR[0], KEYLINE_COLOR[1], KEYLINE_COLOR[2], KEYLINE_INTENSITY],
                params: [kind::KEYLINE, 0.0, 0.0, KEYLINE_REACH],
            });
        }

        for p in &self.particles {
            let t = p.life / p.max_life; // 1 -> 0
            let age = 1.0 - t;
            match p.kind {
                PKind::Dust => {
                    let twinkle = (p.seed * 90.0 + clock * (6.0 + p.seed * 10.0)).sin();
                    let twinkle = 0.3 + 0.7 * twinkle * twinkle;
                    let fade = (age * 12.0).min(1.0) * t.powf(0.8);
                    let s = p.size;
                    inst.push(GlowInstance {
                        position: [p.pos[0] - s / 2.0, p.pos[1] - s / 2.0],
                        size: [s, s],
                        color: with_intensity(p.color, fade * twinkle * 1.8),
                        params: [kind::SPARK, age, p.seed, 0.0],
                    });
                }
                PKind::Smoke => {
                    let s = SMOKE_START + (SMOKE_END - SMOKE_START) * age.sqrt();
                    let fade = (age * 8.0).min(1.0) * t.powf(1.2);
                    inst.push(GlowInstance {
                        position: [p.pos[0] - s / 2.0, p.pos[1] - s / 2.0],
                        size: [s, s],
                        color: with_intensity(p.color, fade * SMOKE_INTENSITY),
                        params: [kind::SMOKE, age, p.seed, 0.0],
                    });
                }
                PKind::Spark => {
                    let s = p.size * (0.4 + 0.6 * t);
                    let flicker = 0.8 + 0.2 * (p.seed * 50.0 + p.life * 30.0).sin();
                    inst.push(GlowInstance {
                        position: [p.pos[0] - s / 2.0, p.pos[1] - s / 2.0],
                        size: [s, s],
                        color: with_intensity(p.color, t.powf(1.4) * flicker * 1.6),
                        params: [kind::SPARK, age, p.seed, 0.0],
                    });
                }
            }
        }

        self.pipeline.prepare();
    }

    fn spawn_dust(&mut self, k: &KeyFx, burst: bool) {
        if self.particles.len() >= MAX_PARTICLES {
            return;
        }
        let cx = k.x + k.w / 2.0;
        let (vx, vy) = if burst {
            // Note-on: a soft spray, wider and quicker.
            let a = self.rng.range(-PI + 0.5, -0.5);
            let speed = self.rng.range(40.0, 260.0);
            (a.cos() * speed, a.sin() * speed)
        } else {
            // Held: a thin rising column of glitter.
            (self.rng.range(-20.0, 20.0), self.rng.range(-170.0, -50.0))
        };
        let life = self.rng.range(1.4, 3.6);
        let warm = mix3(k.color, KEYLINE_COLOR, DUST_WARMTH);
        self.particles.push(Particle {
            kind: PKind::Dust,
            pos: [cx + self.rng.range(-0.4, 0.4) * k.w, k.y - self.rng.range(0.0, 8.0)],
            vel: [vx, vy],
            accel: -15.0,
            drag: 1.1,
            life,
            max_life: life,
            size: self.rng.range(2.5, 6.0),
            color: whiten(warm, self.rng.range(0.2, 0.7)),
            seed: self.rng.next(),
        });
    }

    fn spawn_burst(&mut self, k: &KeyFx) {
        let cx = k.x + k.w / 2.0;
        for _ in 0..BURST_SPARKS {
            if self.particles.len() >= MAX_PARTICLES {
                return;
            }
            let a = self.rng.range(-PI + 0.3, -0.3);
            let speed = self.rng.range(280.0, 950.0);
            let life = self.rng.range(0.4, 1.0);
            let hot = self.rng.range(0.0, 0.7);
            self.particles.push(Particle {
                kind: PKind::Spark,
                pos: [cx + self.rng.range(-k.w, k.w) * 0.4, k.y],
                vel: [a.cos() * speed, a.sin() * speed],
                accel: GRAVITY,
                drag: 1.6,
                life,
                max_life: life,
                size: self.rng.range(7.0, 15.0),
                color: whiten(k.color, hot),
                seed: self.rng.next(),
            });
        }
    }

    fn spawn_ember(&mut self, k: &KeyFx) {
        if self.particles.len() >= MAX_PARTICLES {
            return;
        }
        let cx = k.x + k.w / 2.0;
        let life = self.rng.range(0.6, 1.5);
        self.particles.push(Particle {
            kind: PKind::Spark,
            pos: [cx + self.rng.range(-0.5, 0.5) * k.w, k.y - self.rng.range(0.0, 6.0)],
            vel: [self.rng.range(-50.0, 50.0), self.rng.range(-380.0, -110.0)],
            accel: EMBER_LIFT,
            drag: 0.9,
            life,
            max_life: life,
            size: self.rng.range(4.0, 10.0),
            color: whiten(k.color, self.rng.range(0.0, 0.35)),
            seed: self.rng.next(),
        });
    }

    fn spawn_smoke(&mut self, k: &KeyFx) {
        if self.particles.len() >= MAX_PARTICLES {
            return;
        }
        let cx = k.x + k.w / 2.0;
        let c = k.color;
        let lum = 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2];
        let tint = |v: f32| (v * 0.35 + lum * 0.65) * 0.8 + 0.12;
        let life = self.rng.range(1.6, 2.8);
        self.particles.push(Particle {
            kind: PKind::Smoke,
            pos: [cx + self.rng.range(-0.5, 0.5) * k.w, k.y - self.rng.range(4.0, 16.0)],
            vel: [self.rng.range(-25.0, 25.0), self.rng.range(-110.0, -45.0)],
            accel: -12.0,
            drag: 0.6,
            life,
            max_life: life,
            size: 0.0,
            color: [tint(c[0]), tint(c[1]), tint(c[2]), 1.0],
            seed: self.rng.next(),
        });
    }
}
