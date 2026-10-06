use wgpu::vertex_attr_array;
use wgpu_jumpstart::wgpu;

use bytemuck::{Pod, Zeroable};

/// FX kinds understood by the glow shader (`params.x`).
pub mod kind {
    pub const HALO: f32 = 0.0;
    pub const SPARK: f32 = 1.0;
    #[allow(dead_code)] // shockwave, parked; shader still supports it
    pub const RING: f32 = 2.0;
    pub const BEAM: f32 = 3.0;
    pub const SMOKE: f32 = 4.0;
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable, PartialEq)]
pub struct GlowInstance {
    pub position: [f32; 2],
    pub size: [f32; 2],
    /// Linear RGB + intensity in alpha. Blending is additive.
    pub color: [f32; 4],
    /// x: kind, y: age 0..1, z: seed, w: reserved
    pub params: [f32; 4],
}

impl Default for GlowInstance {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0],
            size: [0.0, 0.0],
            color: [0.0, 0.0, 0.0, 1.0],
            params: [0.0; 4],
        }
    }
}

impl GlowInstance {
    pub fn attributes() -> [wgpu::VertexAttribute; 4] {
        vertex_attr_array!(1 => Float32x2, 2 => Float32x2, 3 => Float32x4, 4 => Float32x4)
    }

    pub fn layout(attributes: &[wgpu::VertexAttribute]) -> wgpu::VertexBufferLayout<'_> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GlowInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes,
        }
    }
}
