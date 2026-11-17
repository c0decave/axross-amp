//! The faces drawn pixel by pixel: one WGSL module (`shaders/faces.wgsl`)
//! run through egui's wgpu renderer as a paint callback - the face, the
//! time and the analysis in a uniform buffer, a texture for the faces that
//! keep a picture (the mel spectrogram, the reaction-diffusion). Where
//! there is no wgpu (another renderer, a test harness without a GPU) the
//! same functions are worked out here on the CPU on a coarse grid of
//! vertices, their colours blended in between: the face is there, softer.

use super::{Pen, Scheme};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};
use egui::{Color32, Rect, pos2};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// The faces the shader draws (its `u.face.x`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shader {
    Mel = 0,
    Plasma = 1,
    Fire = 2,
    Metaballs = 3,
    Aurora = 4,
    Tunnel = 5,
    Julia = 6,
    Reaction = 7,
    Ripples = 8,
    Moire = 9,
    Smoke = 10,
}

impl Shader {
    pub fn of(key: &str) -> Option<Shader> {
        Some(match key {
            "mel" => Shader::Mel,
            "plasma" => Shader::Plasma,
            "fire" => Shader::Fire,
            "metaballs" => Shader::Metaballs,
            "aurora" => Shader::Aurora,
            "tunnel" => Shader::Tunnel,
            "julia" => Shader::Julia,
            "reaction" => Shader::Reaction,
            "ripples" => Shader::Ripples,
            "moire" => Shader::Moire,
            "smoke" => Shader::Smoke,
            _ => return None,
        })
    }
}

/// vec4s of the uniform block (faces.wgsl's `U`).
const VEC4S: usize = 26;
/// Uniform slots: each picture its own (a window, a test's harness).
const SLOTS: usize = 8;
static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);

/// What the shader is told: faces.wgsl's `U`.
#[derive(Clone, Copy, Debug)]
pub struct Uniforms {
    pub v: [[f32; 4]; VEC4S],
}

impl Default for Uniforms {
    fn default() -> Self {
        Uniforms {
            v: [[0.0; 4]; VEC4S],
        }
    }
}

impl Uniforms {
    pub fn time(&self) -> f32 {
        self.v[2][0]
    }
    pub fn pulse(&self) -> f32 {
        self.v[2][1]
    }
    pub fn energy(&self) -> f32 {
        self.v[2][2]
    }
    pub fn bass(&self) -> f32 {
        self.v[2][3]
    }
    pub fn phase(&self) -> f32 {
        self.v[3][0]
    }
    pub fn middle(&self) -> f32 {
        self.v[3][1]
    }
    pub fn treble(&self) -> f32 {
        self.v[3][2]
    }
    pub fn scroll(&self) -> f32 {
        self.v[4][2]
    }
    pub fn colour(&self, i: usize) -> [f32; 3] {
        let c = self.v[5 + i];
        [c[0], c[1], c[2]]
    }
    pub fn band(&self, x: f32) -> f32 {
        let f = x.clamp(0.0, 1.0) * 31.0;
        let i = f.floor() as usize;
        let j = (i + 1).min(31);
        let at = |k: usize| self.v[10 + k / 4][k % 4];
        at(i) + (at(j) - at(i)) * (f - i as f32)
    }
    pub fn drop(&self, k: usize) -> [f32; 4] {
        self.v[18 + k]
    }

    /// Everything but the place: the face, the clock, the analysis, the
    /// scheme.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        shader: Shader,
        a: &crate::analyzer::Analyzer,
        s: &Scheme,
        drops: &[[f32; 4]; 8],
        scroll: f32,
    ) -> Uniforms {
        let mut u = Uniforms::default();
        let n = a.bars.len().max(1);
        let third = |k: usize| {
            let (from, to) = (k * n / 3, ((k + 1) * n / 3).max(k * n / 3 + 1).min(n));
            a.bars[from..to].iter().sum::<f32>() / (to - from) as f32
        };
        u.v[2] = [(a.time % 3600.0) as f32, a.beat.pulse, a.energy, a.bass];
        u.v[3] = [a.phase, third(1), third(2), 0.0];
        u.v[4] = [shader as u32 as f32, 0.0, scroll, a.beat.count as f32];
        for (i, c) in [s.background, s.low, s.mid, s.high, s.peak]
            .iter()
            .enumerate()
        {
            u.v[5 + i] = [
                c.r() as f32 / 255.0,
                c.g() as f32 / 255.0,
                c.b() as f32 / 255.0,
                1.0,
            ];
        }
        for k in 0..32 {
            let x = k as f32 / 31.0 * (n - 1) as f32;
            let i = x.floor() as usize;
            let j = (i + 1).min(n - 1);
            u.v[10 + k / 4][k % 4] = a.bars[i] + (a.bars[j] - a.bars[i]) * (x - i as f32);
        }
        u.v[18..26].copy_from_slice(drops);
        u
    }
}

/// A picture a face keeps (one byte a pixel, 0..1).
#[derive(Debug, Default)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub version: u64,
}

impl Picture {
    pub fn at(&self, x: f32, y: f32) -> f32 {
        if self.width == 0 || self.height == 0 {
            return 0.0;
        }
        // bilinear, clamped at the edges (as the sampler)
        let fx = (x * self.width as f32 - 0.5).clamp(0.0, (self.width - 1) as f32);
        let fy = (y * self.height as f32 - 0.5).clamp(0.0, (self.height - 1) as f32);
        let (i, j) = (fx as usize, fy as usize);
        let (i2, j2) = (
            (i + 1).min(self.width as usize - 1),
            (j + 1).min(self.height as usize - 1),
        );
        let w = self.width as usize;
        let p = |a: usize, b: usize| self.data[b * w + a] as f32 / 255.0;
        let (tx, ty) = (fx - i as f32, fy - j as f32);
        let top = p(i, j) * (1.0 - tx) + p(i2, j) * tx;
        let bottom = p(i, j2) * (1.0 - tx) + p(i2, j2) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

pub struct State {
    /// Whether the shaders run (wgpu is there and they were set up).
    pub on: bool,
    pub picture: Arc<Mutex<Picture>>,
    slot: usize,
}

impl Default for State {
    fn default() -> Self {
        State {
            on: false,
            picture: Arc::default(),
            slot: NEXT_SLOT.fetch_add(1, Ordering::Relaxed) % SLOTS,
        }
    }
}

struct Slot {
    uniforms: wgpu::Buffer,
    texture: wgpu::Texture,
    size: (u32, u32),
    version: u64,
    group: wgpu::BindGroup,
}

/// The pipeline and its slots, among egui's callback resources.
struct Faces {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    linear: bool,
    slots: Vec<Slot>,
}

impl Faces {
    fn texture(device: &wgpu::Device, size: (u32, u32)) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axross-amp face picture"),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn group(
        &self,
        device: &wgpu::Device,
        uniforms: &wgpu::Buffer,
        texture: &wgpu::Texture,
    ) -> wgpu::BindGroup {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axross-amp faces"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
}

/// Set the shaders up for egui's wgpu renderer (once, as the window is made).
pub fn install(render_state: &egui_wgpu::RenderState) {
    let device = &render_state.device;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("axross-amp faces"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/faces.wgsl").into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("axross-amp faces"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("axross-amp faces"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("axross-amp faces"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: render_state.target_format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("axross-amp faces"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let mut faces = Faces {
        pipeline,
        layout,
        sampler,
        linear: render_state.target_format.is_srgb(),
        slots: Vec::with_capacity(SLOTS),
    };
    for _ in 0..SLOTS {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axross-amp face uniforms"),
            size: (VEC4S * 16) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let texture = Faces::texture(device, (1, 1));
        let group = faces.group(device, &uniforms, &texture);
        faces.slots.push(Slot {
            uniforms,
            texture,
            size: (1, 1),
            version: u64::MAX,
            group,
        });
    }
    render_state
        .renderer
        .write()
        .callback_resources
        .insert(faces);
}

struct Paint {
    uniforms: Uniforms,
    picture: Option<Arc<Mutex<Picture>>>,
    slot: usize,
}

impl CallbackTrait for Paint {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(faces) = resources.get_mut::<Faces>() else {
            return Vec::new();
        };
        let mut u = self.uniforms;
        let ppp = screen.pixels_per_point;
        for row in &mut u.v[0..2] {
            for v in row.iter_mut() {
                *v *= ppp;
            }
        }
        u.v[3][3] *= ppp;
        u.v[4][1] = if faces.linear { 1.0 } else { 0.0 };
        let mut bytes = [0u8; VEC4S * 16];
        for (k, v) in u.v.iter().flatten().enumerate() {
            bytes[k * 4..k * 4 + 4].copy_from_slice(&v.to_ne_bytes());
        }
        let slot = self.slot % SLOTS;
        queue.write_buffer(&faces.slots[slot].uniforms, 0, &bytes);
        if let Some(picture) = &self.picture
            && let Ok(picture) = picture.lock()
            && picture.width > 0
            && picture.data.len() == (picture.width * picture.height) as usize
        {
            let size = (picture.width, picture.height);
            if faces.slots[slot].size != size {
                let texture = Faces::texture(device, size);
                let group = faces.group(device, &faces.slots[slot].uniforms, &texture);
                let s = &mut faces.slots[slot];
                s.texture = texture;
                s.group = group;
                s.size = size;
                s.version = u64::MAX;
            }
            let s = &mut faces.slots[slot];
            if s.version != picture.version {
                s.version = picture.version;
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &s.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &picture.data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(picture.width),
                        rows_per_image: Some(picture.height),
                    },
                    wgpu::Extent3d {
                        width: size.0,
                        height: size.1,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(faces) = resources.get::<Faces>() else {
            return;
        };
        pass.set_pipeline(&faces.pipeline);
        pass.set_bind_group(0, &faces.slots[self.slot % SLOTS].group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Draw `shader` into `rect` (`clip`: the rounded background it lies on):
/// on the GPU when it is there, else on the CPU into `pen`.
pub fn paint(
    painter: &egui::Painter,
    pen: &mut Pen<'_>,
    state: &State,
    rect: Rect,
    clip: Rect,
    mut u: Uniforms,
    picture: bool,
) {
    u.v[0] = [rect.left(), rect.top(), rect.width(), rect.height()];
    u.v[1] = [clip.left(), clip.top(), clip.width(), clip.height()];
    u.v[3][3] = 8.0;
    if state.on {
        painter.add(egui_wgpu::Callback::new_paint_callback(
            clip,
            Paint {
                uniforms: u,
                picture: picture.then(|| state.picture.clone()),
                slot: state.slot,
            },
        ));
    } else {
        let guard = state.picture.lock().ok();
        let picture = if picture { guard.as_deref() } else { None };
        cpu(pen, rect, &u, picture);
    }
}

/// The face on the CPU: the shader's function at the vertices of a grid
/// (8 points apart at the finest, as many as the face's cost allows: the
/// noise faces fewer), blended in between.
pub fn cpu(pen: &mut Pen<'_>, rect: Rect, u: &Uniforms, picture: Option<&Picture>) {
    let face = u.v[4][0] as u32;
    let budget = match face {
        // fire, aurora, smoke: layers of noise a point
        2 | 4 | 10 => 1400.0,
        3 | 6 | 8 | 9 => 4000.0,
        _ => 9000.0,
    };
    let aspect = rect.width() / rect.height().max(1.0);
    let rows_fit = (budget / aspect).sqrt();
    let cols = ((rect.width() / 8.0).min(rows_fit * aspect) as usize).max(8);
    let rows = ((rect.height() / 8.0).min(rows_fit) as usize).max(6);
    let base = pen.mesh.vertices.len() as u32;
    for r in 0..=rows {
        for c in 0..=cols {
            let uv = [c as f32 / cols as f32, r as f32 / rows as f32];
            let rgb = shade(face, u, uv, aspect, picture);
            let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            pen.mesh.colored_vertex(
                pos2(
                    rect.left() + uv[0] * rect.width(),
                    rect.top() + uv[1] * rect.height(),
                ),
                Color32::from_rgb(to(rgb[0]), to(rgb[1]), to(rgb[2])),
            );
        }
    }
    let w = (cols + 1) as u32;
    for r in 0..rows as u32 {
        for c in 0..cols as u32 {
            let i = base + r * w + c;
            pen.mesh.add_triangle(i, i + 1, i + w + 1);
            pen.mesh.add_triangle(i, i + w + 1, i + w);
        }
    }
}

// -- the shader's functions, on the CPU (faces.wgsl, line for line) ------------------

type V3 = [f32; 3];

fn mix3(a: V3, b: V3, t: f32) -> V3 {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn fract(x: f32) -> f32 {
    x - x.floor()
}

fn palette(u: &Uniforms, t: f32) -> V3 {
    let x = t.clamp(0.0, 1.0);
    let c = |i| u.colour(i);
    if x < 0.25 {
        mix3(c(0), c(1), x / 0.25)
    } else if x < 0.55 {
        mix3(c(1), c(2), (x - 0.25) / 0.3)
    } else if x < 0.8 {
        mix3(c(2), c(3), (x - 0.55) / 0.25)
    } else {
        mix3(c(3), c(4), (x - 0.8) / 0.2)
    }
}

fn cycle(u: &Uniforms, t: f32) -> V3 {
    let x = fract(t) * 3.0;
    let c = |i| u.colour(i);
    if x < 1.0 {
        mix3(c(1), c(2), x)
    } else if x < 2.0 {
        mix3(c(2), c(3), x - 1.0)
    } else {
        mix3(c(3), c(1), x - 2.0)
    }
}

fn hash(p: [f32; 2]) -> f32 {
    let q = [fract(p[0] * 123.34), fract(p[1] * 456.21)];
    let d = q[0] * (q[0] + 45.32) + q[1] * (q[1] + 45.32);
    let r = [q[0] + d, q[1] + d];
    fract(r[0] * r[1])
}

fn noise(p: [f32; 2]) -> f32 {
    let i = [p[0].floor(), p[1].floor()];
    let f = [p[0] - i[0], p[1] - i[1]];
    let w = [
        f[0] * f[0] * (3.0 - 2.0 * f[0]),
        f[1] * f[1] * (3.0 - 2.0 * f[1]),
    ];
    let a = hash(i);
    let b = hash([i[0] + 1.0, i[1]]);
    let c = hash([i[0], i[1] + 1.0]);
    let d = hash([i[0] + 1.0, i[1] + 1.0]);
    let top = a + (b - a) * w[0];
    let bottom = c + (d - c) * w[0];
    top + (bottom - top) * w[1]
}

fn fbm(p: [f32; 2]) -> f32 {
    let (mut v, mut a, mut q) = (0.0, 0.5, p);
    for _ in 0..5 {
        v += a * noise(q);
        q = [q[0] * 2.03 + 1.7, q[1] * 2.03 + 9.2];
        a *= 0.5;
    }
    v
}

fn shade(face: u32, u: &Uniforms, uv: [f32; 2], aspect: f32, picture: Option<&Picture>) -> V3 {
    let p = [(uv[0] - 0.5) * aspect * 2.0, (uv[1] - 0.5) * 2.0];
    let t = u.time();
    let c = |i| u.colour(i);
    match face {
        0 => {
            let v = picture.map_or(0.0, |pic| {
                pic.at(uv[0] + u.scroll() / pic.width.max(1) as f32, uv[1])
            });
            palette(u, v)
        }
        1 => {
            let t = t * (0.35 + 0.8 * u.energy());
            let mut v = (p[0] * 3.0 + t).sin();
            v += ((p[1] * 3.0 + t) * 0.7).sin();
            v += ((p[0] * 2.0 + p[1] * 3.0 + t * 1.3) * (1.0 + u.bass())).sin();
            let cc = [p[0] + 0.6 * (t * 0.33).sin(), p[1] + 0.6 * (t * 0.5).cos()];
            v += (((cc[0] * cc[0] + cc[1] * cc[1]) * 18.0 + 1.0).sqrt() - t * 2.0).sin();
            let shade = 0.55 + 0.45 * (v * 1.6 + u.phase()).sin();
            add(
                scale(cycle(u, v * 0.18 + t * 0.05), 0.35 + 0.65 * shade),
                scale(c(4), u.pulse() * 0.15 * shade),
            )
        }
        2 => {
            let q = [uv[0] * aspect * 3.0, (1.0 - uv[1]) * 3.0];
            let rise = t * (1.2 + 1.5 * u.energy());
            let inner = fbm([q[0] * 0.7, q[1] * 0.7 - rise * 0.5]);
            let n = fbm([q[0] + inner * 1.3, q[1] - rise]);
            let height = 0.35 + 0.55 * u.band(uv[0] * 0.6) + 0.3 * u.pulse();
            let y = 1.0 - uv[1];
            let flicker = 0.25 * u.treble() * noise([q[0] * 6.0 + t * 8.0, q[1] * 6.0 + t * 8.0]);
            let heat = (n * 1.4 - y / height.max(0.05) + 0.25 + flicker).clamp(0.0, 1.0);
            palette(u, heat.powf(0.8))
        }
        3 => {
            let t = t * 0.6;
            let (mut field, mut tint) = (0.0, [0.0; 3]);
            for k in 0..8 {
                let fk = k as f32;
                let centre = [
                    0.7 * (t * (0.5 + 0.13 * fk) + fk * 1.7).sin(),
                    0.4 * (t * (0.4 + 0.11 * fk) + fk * 2.3).cos(),
                ];
                let level = u.band((fk + 0.5) / 8.0);
                let r = 0.06 + 0.16 * level + 0.05 * u.pulse();
                let d = [p[0] - centre[0], p[1] - centre[1]];
                let f = r * r / (d[0] * d[0] + d[1] * d[1]).max(1e-4);
                field += f;
                tint = add(tint, scale(cycle(u, fk / 8.0 + t * 0.05), f));
            }
            let base = scale(tint, 1.0 / field.max(1e-4));
            let edge = smoothstep(0.9, 1.1, field);
            let glow = (field * 0.45).clamp(0.0, 1.0);
            mix3(
                add(c(0), scale(base, glow * 0.35)),
                add(base, scale(c(4), 0.25 * smoothstep(2.0, 5.0, field))),
                edge,
            )
        }
        4 => {
            let t = t * 0.25;
            let mut col = scale(c(0), 0.7 + 0.3 * uv[1]);
            for k in 0..3 {
                let fk = k as f32;
                let x = uv[0] * aspect * (1.2 + 0.4 * fk);
                let curve = 0.28
                    + 0.12 * fk
                    + 0.08 * (x * 1.3 + t * (1.0 + fk * 0.3) + fk).sin()
                    + 0.05 * fbm([x * 0.8, t + fk]);
                let level = u.band(0.15 + 0.3 * fk + 0.2 * uv[0]);
                let d = uv[1] - curve;
                let rays = 0.55 + 0.45 * fbm([x * 6.0, t * 2.0 + fk * 3.0]);
                let below = if d >= 0.0 {
                    (-d * (5.0 - 3.0 * level)).exp()
                } else {
                    0.0
                };
                let above = (-(-d).max(0.0) * 40.0).exp();
                let light = (below * rays + above * 0.6) * (0.25 + 0.9 * level + 0.3 * u.pulse());
                let hue = mix3(c(1), c(2 + k % 2), uv[1]);
                col = add(col, scale(hue, light * 0.6));
            }
            col
        }
        5 => {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt().max(1e-3);
            let a = p[1].atan2(p[0]);
            let depth = 0.35 / r + t * (0.6 + 1.4 * u.bass());
            let turn = a / std::f32::consts::TAU + 0.15 * (depth * 0.7 + u.phase()).sin();
            let rings = 0.5 + 0.5 * (depth * 6.0).sin();
            let stripes = 0.5 + 0.5 * (turn * std::f32::consts::TAU * 8.0 + depth).sin();
            let level = u.band(fract(turn + 1.0));
            let light = rings * stripes * (0.4 + 0.8 * level) + 0.25 * u.pulse() * rings;
            let fog = (r * 2.2).clamp(0.0, 1.0);
            add(
                mix3(
                    c(0),
                    scale(cycle(u, depth * 0.05 + level * 0.3), 0.3 + light),
                    fog,
                ),
                scale(c(4), (rings * stripes).powi(8) * fog * 0.5),
            )
        }
        6 => {
            let tt = t * 0.07 + u.phase() * 0.05;
            let k = 0.985 + 0.012 * u.bass();
            let cc = [
                (tt.cos() * 0.5 - (2.0 * tt).cos() * 0.25) * k,
                (tt.sin() * 0.5 - (2.0 * tt).sin() * 0.25) * k,
            ];
            let zoom = 1.35 - 0.15 * u.pulse();
            let mut z = [p[0] * zoom, p[1] * zoom];
            // (the CPU's grid is coarse: fewer steps do)
            let most = 48;
            let mut i = 0;
            while i < most {
                z = [z[0] * z[0] - z[1] * z[1] + cc[0], 2.0 * z[0] * z[1] + cc[1]];
                if z[0] * z[0] + z[1] * z[1] > 64.0 {
                    break;
                }
                i += 1;
            }
            if i >= most {
                return mix3(c(0), c(1), 0.15 + 0.2 * u.pulse());
            }
            let m = z[0] * z[0] + z[1] * z[1];
            let smooth = (i as f32 - m.log2().log2() + 4.0).max(0.0);
            let v = (1.0 + smooth).log2() / 97f32.log2();
            palette(u, (v * (0.9 + 0.25 * u.energy())).clamp(0.0, 1.0))
        }
        7 => {
            let Some(pic) = picture else { return c(0) };
            let v = pic.at(uv[0], uv[1]);
            let dx = pic.at(uv[0] + 1.0 / pic.width.max(1) as f32, uv[1]) - v;
            let dy = pic.at(uv[0], uv[1] + 1.0 / pic.height.max(1) as f32) - v;
            let light = (0.6 + 3.0 * (dx - dy)).clamp(0.0, 1.4);
            scale(palette(u, smoothstep(0.05, 0.6, v) * 0.95), light)
        }
        8 => {
            let q = [uv[0] * aspect, uv[1]];
            let (mut h, mut slope) = (0.0, [0.0f32; 2]);
            for k in 0..8 {
                let d = u.drop(k);
                if d[3] <= 0.0 {
                    continue;
                }
                let v = [q[0] - d[0] * aspect, q[1] - d[1]];
                let r = (v[0] * v[0] + v[1] * v[1]).sqrt();
                let front = r - d[2] * 0.35;
                let env = (-front * front * 90.0).exp() * (-d[2] * 1.1).exp() * d[3];
                h += (front * 60.0).sin() * env;
                let k2 = (front * 60.0).cos() * env / r.max(1e-3);
                slope = [slope[0] + v[0] * k2, slope[1] + v[1] * k2];
            }
            let ambient = 0.5 + 0.5 * (q[0] * 7.0 + t * 0.7).sin() * (q[1] * 9.0 - t * 0.5).sin();
            let base = mix3(c(0), c(1), 0.25 + 0.2 * ambient * u.energy());
            let light = (slope[0] * 0.6 - slope[1] * 0.8).clamp(-1.0, 1.0);
            let lit = light.max(0.0);
            let col = add(
                add(base, scale(cycle(u, t * 0.03 + h * 0.2), lit * 0.9)),
                scale(c(4), lit.powi(6) * 0.6),
            );
            add(col, scale(c(0), -(-light).max(0.0) * 0.4))
        }
        9 => {
            let tt = t * 0.4;
            let a = [0.35 * tt.sin(), 0.25 * (tt * 0.8).cos()];
            let b = [0.35 * (tt * 0.7 + 2.0).sin(), 0.25 * (tt * 1.1 + 1.0).cos()];
            let len = |x: f32, y: f32| (x * x + y * y).sqrt();
            let f1 = 40.0 + 30.0 * u.bass();
            let f2 = 42.0 + 30.0 * u.middle();
            let r1 = 0.5 + 0.5 * (len(p[0] - a[0], p[1] - a[1]) * f1 - t * 2.0).sin();
            let r2 = 0.5 + 0.5 * (len(p[0] - b[0], p[1] - b[1]) * f2 + t * 1.5).sin();
            let lines = 0.5
                + 0.5
                    * ((p[0] * (tt * 0.3).cos() + p[1] * (tt * 0.3).sin())
                        * (30.0 + 20.0 * u.treble()))
                    .sin();
            let v = r1 * r2 + 0.35 * lines * r1;
            add(
                mix3(c(0), cycle(u, v * 0.4 + tt * 0.1), smoothstep(0.15, 0.9, v)),
                scale(c(4), u.pulse() * 0.2 * v),
            )
        }
        _ => {
            let tt = t * 0.25;
            let q = [uv[0] * aspect * 2.2, uv[1] * 2.2];
            let warp = [
                fbm([q[0], q[1] + tt * 2.0]),
                fbm([q[0] + 5.2, q[1] + 1.3 + tt * 2.0]),
            ];
            let warp2 = [
                fbm([
                    q[0] + 3.0 * warp[0] + 1.7 + tt,
                    q[1] + 3.0 * warp[1] + 9.2 + tt,
                ]),
                fbm([
                    q[0] + 3.0 * warp[0] + 8.3 - tt,
                    q[1] + 3.0 * warp[1] + 2.8 - tt,
                ]),
            ];
            let d = fbm([q[0] + 2.5 * warp2[0], q[1] + 2.5 * warp2[1] + tt * 3.0]);
            let density =
                ((d - 0.25 + 0.35 * u.energy()) * (1.3 + u.pulse() * 0.5)).clamp(0.0, 1.0);
            let length = (warp2[0] * warp2[0] + warp2[1] * warp2[1])
                .sqrt()
                .clamp(0.0, 1.0);
            let shade = mix3(c(1), c(3), length);
            add(
                mix3(c(0), shade, density),
                scale(c(4), density.powi(4) * 0.3),
            )
        }
    }
}
