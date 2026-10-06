//! Juiced glow: halos, light beams, note-on spark bursts, held-note embers,
//! and smoke that curls up off struck keys. All CPU-simulated, drawn in one
//! additive pass.
//!
//! Public API is identical to upstream (`new / clear / push / prepare / render`)
//! so scenes need no changes. Note-on edges are detected here by diffing
//! which keys were pushed this frame vs. last frame.

use std::f32::consts::PI;
use std::time::{Duration, Instant};

use wgpu_jumpstart::{Color, Gpu, TransformUniform, Uniform};

use super::{GlowInstance, GlowPipeline, kind};

// ---- tuning knobs ---------------------------------------------------------
const MAX_PARTICLES: usize = 16_000;
const BURST_SPARKS: usize = 34;
const EMBERS_PER_SEC: f32 = 55.0;
const GRAVITY: f32 = 1500.0; // px/s^2, sparks
const EMBER_LIFT: f32 = -90.0; // px/s^2, embers float up
const HALO_SIZE: f32 = 120.0;
const BEAM_HEIGHT: f32 = 320.0;
const FLASH_DECAY: f32 = 7.0; // note-on flash falloff, 1/s
const SMOKE_PUFFS: usize = 3; // per note-on
const SMOKE_PER_SEC: f32 = 5.0; // while held
const SMOKE_START: f32 = 30.0; // px, puff size at birth
const SMOKE_END: f32 = 170.0; // px, puff size at death
const SMOKE_INTENSITY: f32 = 0.22;
// ---------------------------------------------------------------------------

#[derive(Default, Clone, Copy)]
struct KeyFx {
    held: bool,
    held_prev: bool,
    hold_time: f32,
    pulse: f32,
    ember_acc: f32,
    smoke_acc: f32,
    color: [f32; 4],
    x: f32,
    y: f32,
    w: f32,
}

#[derive(Clone, Copy)]
struct Particle {
    pos: [f32; 2],
    vel: [f32; 2],
    accel: f32,
    drag: f32,
    life: f32,
    max_life: f32,
    size: f32,
    color: [f32; 4],
    seed: f32,
    smoke: bool,
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

fn whiten(c: [f32; 4], t: f32) -> [f32; 4] {
    [
        c[0] + (1.0 - c[0]) * t,
        c[1] + (1.0 - c[1]) * t,
        c[2] + (1.0 - c[2]) * t,
        c[3],
    ]
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
            k.ember_acc = 0.0;
            k.smoke_acc = 0.0;
        } else {
            k.hold_time += delta.as_secs_f32();
        }
        k.pulse += delta.as_secs_f32() * 5.0;

        let k = *k;
        let cx = k.x + k.w / 2.0;
        let flash = (-k.hold_time * FLASH_DECAY).exp();

        if note_on {
            self.spawn_burst(&k);
            for _ in 0..SMOKE_PUFFS {
                self.spawn_smoke(&k);
            }
        }

        let inst = self.pipeline.instances();

        // Halo: tight, breathes while held, small punch on note-on.
        let hs = HALO_SIZE * (1.0 + 0.15 * flash) + k.pulse.sin() * 5.0;
        inst.push(GlowInstance {
            position: [cx - hs / 2.0, k.y - hs / 2.0],
            size: [hs, hs],
            color: with_intensity(whiten(k.color, 0.1), 0.35 + 0.7 * flash),
            params: [kind::HALO, 0.0, 0.0, 0.0],
        });

        // Beam: shoots up into the waterfall, lands the note visually.
        let bw = k.w * 2.2;
        let bh = BEAM_HEIGHT * (0.75 + 0.5 * flash);
        inst.push(GlowInstance {
            position: [cx - bw / 2.0, k.y - bh],
            size: [bw, bh],
            color: with_intensity(k.color, 0.35 + 0.9 * flash),
            params: [kind::BEAM, 0.0, (id as f32 * 0.618).fract(), 0.0],
        });
    }

    pub fn prepare(&mut self) {
        let now = Instant::now();
        let dt = self
            .last_frame
            .map(|t| (now - t).as_secs_f32())
            .unwrap_or(0.0)
            .min(0.05);
        self.last_frame = Some(now);

        // Embers + smoke stream off held keys; roll held state for edge detection.
        for i in 0..self.keys.len() {
            let mut k = self.keys[i];
            if k.held {
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

        self.particles.retain_mut(|p| {
            p.life -= dt;
            if p.life <= 0.0 {
                return false;
            }
            let damp = (-p.drag * dt).exp();
            p.vel[0] *= damp;
            p.vel[1] = p.vel[1] * damp + p.accel * dt;
            p.pos[0] += p.vel[0] * dt;
            p.pos[1] += p.vel[1] * dt;
            true
        });

        let inst = self.pipeline.instances();

        for p in &self.particles {
            let t = p.life / p.max_life; // 1 -> 0
            let age = 1.0 - t;
            if p.smoke {
                // Grows and thins; fades in fast, out slow.
                let s = SMOKE_START + (SMOKE_END - SMOKE_START) * age.sqrt();
                let fade = (age * 8.0).min(1.0) * t.powf(1.2);
                inst.push(GlowInstance {
                    position: [p.pos[0] - s / 2.0, p.pos[1] - s / 2.0],
                    size: [s, s],
                    color: with_intensity(p.color, fade * SMOKE_INTENSITY),
                    params: [kind::SMOKE, age, p.seed, 0.0],
                });
            } else {
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

        self.pipeline.prepare();
    }

    fn spawn_burst(&mut self, k: &KeyFx) {
        let cx = k.x + k.w / 2.0;
        for _ in 0..BURST_SPARKS {
            if self.particles.len() >= MAX_PARTICLES {
                return;
            }
            // Upward cone; y is down, so upward angles are negative.
            let a = self.rng.range(-PI + 0.3, -0.3);
            let speed = self.rng.range(280.0, 950.0);
            let life = self.rng.range(0.4, 1.0);
            let hot = self.rng.range(0.0, 0.7);
            self.particles.push(Particle {
                pos: [cx + self.rng.range(-k.w, k.w) * 0.4, k.y],
                vel: [a.cos() * speed, a.sin() * speed],
                accel: GRAVITY,
                drag: 1.6,
                life,
                max_life: life,
                size: self.rng.range(7.0, 15.0),
                color: whiten(k.color, hot),
                seed: self.rng.next(),
                smoke: false,
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
            pos: [cx + self.rng.range(-0.5, 0.5) * k.w, k.y - self.rng.range(0.0, 6.0)],
            vel: [self.rng.range(-50.0, 50.0), self.rng.range(-380.0, -110.0)],
            accel: EMBER_LIFT,
            drag: 0.9,
            life,
            max_life: life,
            size: self.rng.range(4.0, 10.0),
            color: whiten(k.color, self.rng.range(0.0, 0.35)),
            seed: self.rng.next(),
            smoke: false,
        });
    }

    fn spawn_smoke(&mut self, k: &KeyFx) {
        if self.particles.len() >= MAX_PARTICLES {
            return;
        }
        let cx = k.x + k.w / 2.0;
        // Mostly grey, faint tint of the note's color.
        let c = k.color;
        let lum = 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2];
        let tint = |v: f32| (v * 0.35 + lum * 0.65) * 0.8 + 0.12;
        let life = self.rng.range(1.6, 2.8);
        self.particles.push(Particle {
            pos: [cx + self.rng.range(-0.5, 0.5) * k.w, k.y - self.rng.range(4.0, 16.0)],
            vel: [self.rng.range(-25.0, 25.0), self.rng.range(-110.0, -45.0)],
            accel: -12.0,
            drag: 0.6,
            life,
            max_life: life,
            size: 0.0,
            color: [tint(c[0]), tint(c[1]), tint(c[2]), 1.0],
            seed: self.rng.next(),
            smoke: true,
        });
    }
}
