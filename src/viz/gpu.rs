//! The faces drawn pixel by pixel: one WGSL module (`shaders/faces.wgsl`)
//! run through egui's wgpu renderer as a paint callback - the face, the
//! time and the analysis in a uniform buffer, a texture for the faces that
//! keep a picture (the mel spectrogram, the reaction-diffusion, the
//! goniometer's phosphor), and for the feedback face two textures of its
//! own it draws into in turn (each frame its last one, warped, under a new
//! layer).
//!
//! Where shaders do not run on a GPU of their own - no wgpu, or wgpu on a
//! software rasterizer (llvmpipe, lavapipe), which takes a CPU's time as
//! well - the same functions are worked out per pixel by a thread of the
//! face's own, at the resolution it manages within a frame, and shown as a
//! picture.

use super::{Pen, Scheme};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};
use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions, pos2};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// The faces the shader draws (its `u.face.x`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shader {
    Mel = 0,
    Liquid = 1,
    Fire = 2,
    Metaballs = 3,
    Aurora = 4,
    Tunnel = 5,
    Julia = 6,
    Reaction = 7,
    Feedback = 8,
    Moire = 9,
    Smoke = 10,
    Flyover = 11,
    Crystals = 12,
    Phosphor = 13,
    /// The waterfall at high resolution (its picture: rows of time).
    Spectrogram = 15,
    /// A height field the CPU moves (its picture), lit.
    Water = 16,
    /// A Clifford attractor's density (its picture), glowing.
    Attractor = 17,
    /// Sand on a Chladni plate.
    Cymatics = 18,
    /// A contour map of land the bands raise.
    Contours = 19,
}

impl State {
    /// The picture of the face shown (`second`: the one faded from).
    pub fn picture(&self, second: bool) -> &Mutex<Picture> {
        &self.pictures[usize::from(second)]
    }
}

impl Shader {
    pub fn of(key: &str) -> Option<Shader> {
        Some(match key {
            "mel" => Shader::Mel,
            "plasma" => Shader::Liquid,
            "fire" => Shader::Fire,
            "metaballs" => Shader::Metaballs,
            "aurora" => Shader::Aurora,
            "tunnel" => Shader::Tunnel,
            "julia" => Shader::Julia,
            "reaction" => Shader::Reaction,
            "milkdrop" => Shader::Feedback,
            "moire" => Shader::Moire,
            "smoke" => Shader::Smoke,
            "flyover" => Shader::Flyover,
            "crystals" => Shader::Crystals,
            "water" => Shader::Water,
            "attractor" => Shader::Attractor,
            "cymatics" => Shader::Cymatics,
            "contours" => Shader::Contours,
            _ => return None,
        })
    }
}

/// vec4s of the uniform block (faces.wgsl's `U`).
const VEC4S: usize = 26;
/// Uniform slots, two to a picture (a face, and the one it fades from).
const SLOTS: usize = 16;
static NEXT_PAIR: AtomicUsize = AtomicUsize::new(0);
/// The feedback face's own pictures: this share of its size.
const FEEDBACK_SCALE: f32 = 0.6;

/// A shader face painted: where, its uniforms, its picture.
pub type Drawn = (Rect, Uniforms, Option<Arc<Mutex<Picture>>>);

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
    pub fn face(&self) -> u32 {
        self.v[4][0] as u32
    }
    pub fn colour(&self, i: usize) -> V3 {
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
    pub fn extra(&self, k: usize) -> [f32; 4] {
        self.v[18 + k]
    }
    /// The clock as the energy drives it, and the bass (see
    /// `Analyzer::energy_flow`).
    pub fn energy_flow(&self) -> f32 {
        self.v[25][0]
    }
    pub fn bass_flow(&self) -> f32 {
        self.v[25][1]
    }

    /// The face, the clock, the analysis, the scheme; opaque.
    pub fn new(shader: Shader, a: &crate::analyzer::Analyzer, s: &Scheme, scroll: f32) -> Uniforms {
        let mut u = Uniforms::default();
        let n = a.bars.len().max(1);
        u.v[2] = [(a.time % 3600.0) as f32, a.beat.pulse, a.energy, a.bass];
        u.v[3] = [a.phase, a.middle, a.treble, 0.0];
        u.v[4] = [shader as u32 as f32, 0.0, scroll, 1.0];
        // (u.extra[7]: the clocks the energy and the bass drive)
        u.v[25] = [
            (a.energy_flow % 3600.0) as f32,
            (a.bass_flow % 3600.0) as f32,
            0.0,
            0.0,
        ];
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
        u
    }

    /// The feedback face's motion this frame: turn, zoom, swirl, decay.
    pub fn with_motion(mut self, motion: [f32; 4]) -> Uniforms {
        self.v[18] = motion;
        self
    }

    /// Per face numbers, `extra[k]` (k 1..7; 0 is the feedback's motion).
    pub fn with_extra(mut self, k: usize, values: [f32; 4]) -> Uniforms {
        self.v[18 + k.min(7)] = values;
        self
    }

    pub fn with_opacity(mut self, opacity: f32) -> Uniforms {
        self.v[4][3] = opacity.clamp(0.0, 1.0);
        self
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
        if self.width == 0 || self.height == 0 || self.data.is_empty() {
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
    /// Whether the shaders run on a GPU (else the CPU's thread draws them).
    pub on: bool,
    /// The pictures the faces keep (the face shown's, the faded one's).
    pub pictures: [Arc<Mutex<Picture>>; 2],
    /// Two uniform slots (the face shown, the one it fades from).
    pair: usize,
    cpu: Option<Cpu>,
    /// Tests: the CPU's picture made in the same frame (not one behind).
    pub sync: bool,
    /// Tests: the CPU's resolution fixed at this share of the face's.
    pub cpu_scale: Option<f32>,
    /// The shaders' resolution as a share of the face's (below 1 on a slow
    /// GPU: drawn small, scaled up), and the seconds between two frames
    /// (the frame rate's cap: the CPU's thread makes no more pictures).
    pub scale: f32,
    pub interval: f32,
    /// What a pixel of a shader face costs the GPU (ms, measured at the
    /// start): the faces are drawn as small as keeps them within
    /// GPU_BUDGET_MS a frame. 0: as they are.
    pub ms_per_pixel: f32,
    /// The most pixels a shader face is drawn in (a software renderer: the
    /// window's size does not make it dearer); 0: no bound.
    pub max_pixels: f32,
    /// The shader faces painted this frame (their rect, uniforms, picture):
    /// the flash guard estimates what they show from them.
    pub drawn: Vec<Drawn>,
}

impl Default for State {
    fn default() -> Self {
        State {
            on: false,
            pictures: [Arc::default(), Arc::default()],
            pair: NEXT_PAIR.fetch_add(1, Ordering::Relaxed) % (SLOTS / 2),
            cpu: None,
            sync: false,
            cpu_scale: None,
            scale: 1.0,
            interval: 1.0 / 30.0,
            ms_per_pixel: 0.0,
            max_pixels: 0.0,
            drawn: Vec::with_capacity(4),
        }
    }
}

struct Feedback {
    textures: [wgpu::Texture; 2],
    /// The groups that read texture k (with this slot's uniforms).
    groups: [wgpu::BindGroup; 2],
    size: (u32, u32),
    /// Which texture holds the last frame.
    last: usize,
}

struct Slot {
    uniforms: wgpu::Buffer,
    texture: wgpu::Texture,
    size: (u32, u32),
    version: u64,
    group: wgpu::BindGroup,
    feedback: Option<Feedback>,
    scaled: Option<Scaled>,
}

/// A face drawn smaller than it is shown: its own picture, the uniforms it
/// is drawn with, the group that draws it (with the slot's picture) and
/// the one that shows it (scaled up).
struct Scaled {
    texture: wgpu::Texture,
    size: (u32, u32),
    data_size: (u32, u32),
    uniforms: wgpu::Buffer,
    draw: wgpu::BindGroup,
    blit: wgpu::BindGroup,
}

/// The pipelines and their slots, among egui's callback resources.
struct Faces {
    pipeline: wgpu::RenderPipeline,
    feedback: wgpu::RenderPipeline,
    offscreen: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    linear: bool,
    slots: Vec<Slot>,
}

impl Faces {
    fn texture(
        device: &wgpu::Device,
        size: (u32, u32),
        format: wgpu::TextureFormat,
        target: bool,
    ) -> wgpu::Texture {
        let mut usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        if target {
            usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
        }
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
            format,
            usage,
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

/// Whether the adapter is a software rasterizer (its shading costs a CPU's
/// time: the CPU's own drawing is cheaper there).
pub fn is_software(render_state: &egui_wgpu::RenderState) -> bool {
    render_state.adapter.get_info().device_type == wgpu::DeviceType::Cpu
}

/// Set the shaders up for egui's wgpu renderer (once, as the window is
/// made).
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
    let pipeline = |entry: &str, format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    let main = pipeline(
        "fs_main",
        render_state.target_format,
        Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
    );
    let feedback = pipeline("fs_feedback", wgpu::TextureFormat::Rgba8Unorm, None);
    let offscreen = pipeline("fs_main", wgpu::TextureFormat::Rgba8Unorm, None);
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("axross-amp faces"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let mut faces = Faces {
        pipeline: main,
        feedback,
        offscreen,
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
        let texture = Faces::texture(device, (1, 1), wgpu::TextureFormat::R8Unorm, false);
        let group = faces.group(device, &uniforms, &texture);
        faces.slots.push(Slot {
            uniforms,
            texture,
            size: (1, 1),
            version: u64::MAX,
            group,
            feedback: None,
            scaled: None,
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
    /// The face's size in points (the feedback's pictures follow it).
    size: (f32, f32),
    /// Drawn at this share of its size and scaled up (1: as it is).
    scale: f32,
}

impl CallbackTrait for Paint {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
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
        if u.face() == Shader::Feedback as u32 {
            // the next frame from the last, into the other picture
            let want = (
                ((self.size.0 * ppp * FEEDBACK_SCALE) as u32).clamp(16, 2048),
                ((self.size.1 * ppp * FEEDBACK_SCALE) as u32).clamp(16, 2048),
            );
            if faces.slots[slot]
                .feedback
                .as_ref()
                .is_none_or(|f| f.size != want)
            {
                let make = || Faces::texture(device, want, wgpu::TextureFormat::Rgba8Unorm, true);
                let textures = [make(), make()];
                let uniforms = &faces.slots[slot].uniforms;
                let groups = [
                    faces.group(device, uniforms, &textures[0]),
                    faces.group(device, uniforms, &textures[1]),
                ];
                faces.slots[slot].feedback = Some(Feedback {
                    textures,
                    groups,
                    size: want,
                    last: 0,
                });
            }
            let pipeline = &faces.feedback;
            if let Some(f) = faces.slots[slot].feedback.as_mut() {
                let next = 1 - f.last;
                let view = f.textures[next].create_view(&wgpu::TextureViewDescriptor::default());
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("axross-amp feedback"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &f.groups[f.last], &[]);
                    pass.draw(0..3, 0..1);
                }
                f.last = next;
            }
            return Vec::new();
        }
        if let Some(picture) = &self.picture
            && let Ok(picture) = picture.lock()
            && picture.width > 0
            && picture.data.len() == (picture.width * picture.height) as usize
        {
            let size = (picture.width, picture.height);
            if faces.slots[slot].size != size {
                let texture = Faces::texture(device, size, wgpu::TextureFormat::R8Unorm, false);
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
        if self.scale < 0.999 {
            self.draw_small(device, queue, ppp, encoder, faces, slot, &u);
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
        let slot = &faces.slots[self.slot % SLOTS];
        pass.set_pipeline(&faces.pipeline);
        let feedback = self.uniforms.face() == Shader::Feedback as u32;
        let group = match (&slot.feedback, &slot.scaled) {
            (Some(f), _) if feedback => &f.groups[f.last],
            (_, Some(small)) if self.scale < 0.999 && !feedback => &small.blit,
            _ => &slot.group,
        };
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
}

impl Paint {
    /// The face drawn small into a picture of its own, the slot's uniforms
    /// made to show that picture scaled up (`blit`, the shader's face 14).
    #[allow(clippy::too_many_arguments)]
    fn draw_small(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        ppp: f32,
        encoder: &mut wgpu::CommandEncoder,
        faces: &mut Faces,
        slot: usize,
        u: &Uniforms,
    ) {
        let want = (
            ((self.size.0 * ppp * self.scale) as u32).clamp(16, 4096),
            ((self.size.1 * ppp * self.scale) as u32).clamp(9, 4096),
        );
        let data_size = faces.slots[slot].size;
        if faces.slots[slot]
            .scaled
            .as_ref()
            .is_none_or(|s| s.size != want || s.data_size != data_size)
        {
            let texture = Faces::texture(device, want, wgpu::TextureFormat::Rgba8Unorm, true);
            let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("axross-amp face uniforms (small)"),
                size: (VEC4S * 16) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let draw = faces.group(device, &uniforms, &faces.slots[slot].texture);
            let blit = faces.group(device, &faces.slots[slot].uniforms, &texture);
            faces.slots[slot].scaled = Some(Scaled {
                texture,
                size: want,
                data_size,
                uniforms,
                draw,
                blit,
            });
        }
        let Some(small) = faces.slots[slot].scaled.as_ref() else {
            return;
        };
        // the face at its small size: its own area, no corners, opaque, as
        // the scheme's colours are (the showing makes them linear if need be)
        let mut drawn = *u;
        drawn.v[0] = [0.0, 0.0, want.0 as f32, want.1 as f32];
        drawn.v[1] = drawn.v[0];
        drawn.v[3][3] = 0.0;
        drawn.v[4][1] = 0.0;
        drawn.v[4][3] = 1.0;
        queue.write_buffer(&small.uniforms, 0, &to_bytes(&drawn));
        // and shown: the picture over the face's area
        let mut shown = *u;
        shown.v[4][0] = BLIT as f32;
        queue.write_buffer(&faces.slots[slot].uniforms, 0, &to_bytes(&shown));
        let view = small
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("axross-amp face (small)"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&faces.offscreen);
        pass.set_bind_group(0, &small.draw, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// What a shader face may take of the GPU's time a frame (ms, by the
/// clock: a software rasterizer spends that on each of its threads).
const GPU_BUDGET_MS: f32 = 1.0;

/// The shader's face that shows a picture drawn smaller, scaled up.
const BLIT: u32 = 14;

fn to_bytes(u: &Uniforms) -> [u8; VEC4S * 16] {
    let mut bytes = [0u8; VEC4S * 16];
    for (k, v) in u.v.iter().flatten().enumerate() {
        bytes[k * 4..k * 4 + 4].copy_from_slice(&v.to_ne_bytes());
    }
    bytes
}

/// Draw `u`'s face into `rect` (`clip`: the rounded background it lies
/// on): on the GPU when the shaders run there, else the CPU's picture of
/// it. `second`: the face being faded from (its own slot).
#[allow(clippy::too_many_arguments)]
pub fn paint(
    painter: &egui::Painter,
    _pen: &mut Pen<'_>,
    state: &mut State,
    rect: Rect,
    clip: Rect,
    mut u: Uniforms,
    picture: bool,
    second: bool,
    live: bool,
) {
    u.v[0] = [rect.left(), rect.top(), rect.width(), rect.height()];
    u.v[1] = [clip.left(), clip.top(), clip.width(), clip.height()];
    u.v[3][3] = 8.0;
    if state.drawn.len() < 8 {
        state.drawn.push((
            rect,
            u,
            picture.then(|| state.pictures[usize::from(second)].clone()),
        ));
    }
    if state.on {
        painter.add(egui_wgpu::Callback::new_paint_callback(
            clip,
            Paint {
                uniforms: u,
                picture: picture.then(|| state.pictures[usize::from(second)].clone()),
                slot: state.pair * 2 + usize::from(second),
                size: (rect.width(), rect.height()),
                scale: {
                    let pixels = (rect.width() * rect.height()).max(1.0)
                        * painter.ctx().pixels_per_point().powi(2);
                    let by_time = if state.ms_per_pixel > 0.0 {
                        (GPU_BUDGET_MS / (state.ms_per_pixel * pixels))
                            .sqrt()
                            .clamp(0.25, 1.0)
                    } else {
                        state.scale
                    };
                    if state.max_pixels > 0.0 {
                        by_time.min((state.max_pixels / pixels).sqrt().max(0.2))
                    } else {
                        by_time
                    }
                },
            },
        ));
        return;
    }
    let picture = picture.then(|| state.pictures[usize::from(second)].clone());
    let (sync, scale, interval) = (state.sync, state.cpu_scale, state.interval);
    let cpu = state.cpu.get_or_insert_with(Cpu::start);
    cpu.frame(
        painter, rect, clip, u, picture, sync, scale, second, live, interval,
    );
}

// -- the CPU's pictures ----------------------------------------------------------

struct Job {
    u: Uniforms,
    width: usize,
    height: usize,
    picture: Option<Arc<Mutex<Picture>>>,
    /// Which of the two faces (a crossfade's: each its own feedback).
    second: bool,
}

struct Done {
    pixels: Vec<Color32>,
    width: usize,
    height: usize,
    millis: f32,
    second: bool,
}

/// A thread that draws the face per pixel, a frame at a time, at the
/// resolution it manages in about 12 ms; its pictures shown as textures.
struct Cpu {
    jobs: Sender<Job>,
    done: Receiver<Done>,
    textures: [Option<TextureHandle>; 2],
    pending: [bool; 2],
    /// When each last asked for a picture.
    asked: [Option<Instant>; 2],
    /// Pixels a frame (adjusted to the time it takes).
    budget: f32,
}

impl Cpu {
    fn start() -> Cpu {
        let (jobs, rx) = channel::<Job>();
        let (tx, done) = channel::<Done>();
        let _ = std::thread::Builder::new()
            .name("amp-face-cpu".into())
            .spawn(move || {
                let mut feedback: [Vec<V3>; 2] = [Vec::new(), Vec::new()];
                let mut sizes = [(0usize, 0usize); 2];
                while let Ok(job) = rx.recv() {
                    let started = Instant::now();
                    let k = usize::from(job.second);
                    if sizes[k] != (job.width, job.height) {
                        sizes[k] = (job.width, job.height);
                        feedback[k] = vec![[0.0; 3]; job.width * job.height];
                    }
                    let pixels = draw(&job, &mut feedback[k]);
                    let millis = started.elapsed().as_secs_f32() * 1000.0;
                    if tx
                        .send(Done {
                            pixels,
                            width: job.width,
                            height: job.height,
                            millis,
                            second: job.second,
                        })
                        .is_err()
                    {
                        break;
                    }
                    // (no repaint asked for: the window shows the picture
                    // at its next frame, at its own rate)
                }
            });
        Cpu {
            jobs,
            done,
            textures: [None, None],
            pending: [false; 2],
            asked: [None; 2],
            budget: 160.0 * 90.0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn frame(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        clip: Rect,
        u: Uniforms,
        picture: Option<Arc<Mutex<Picture>>>,
        sync: bool,
        scale: Option<f32>,
        second: bool,
        live: bool,
        interval: f32,
    ) {
        let k = usize::from(second);
        let ppp = painter.ctx().pixels_per_point();
        let full = (rect.width() * ppp).max(1.0) * (rect.height() * ppp).max(1.0);
        let share = match scale {
            Some(s) => s * s,
            None => (self.budget / full).min(1.0),
        };
        let width = ((rect.width() * ppp * share.sqrt()) as usize).max(16);
        let height = ((rect.height() * ppp * share.sqrt()) as usize).max(9);
        // a new picture only while the face moves (paused, stopped: the last
        // one stays) and when the next frame is due (the frame rate's cap)
        // (tests, in step with the frames: every frame is due)
        let due = sync || self.asked[k].is_none_or(|t| t.elapsed().as_secs_f32() >= interval * 0.9);
        let first = self.textures[k].is_none();
        if !self.pending[k] && ((live && due) || first) {
            self.asked[k] = Some(Instant::now());
            let job = Job {
                u,
                width,
                height,
                picture,
                second,
            };
            if self.jobs.send(job).is_ok() {
                self.pending[k] = true;
            }
        }
        // what came back (in the same frame for tests)
        loop {
            let result = if sync && self.pending[k] {
                self.done.recv().ok()
            } else {
                self.done.try_recv().ok()
            };
            let Some(done) = result else { break };
            let j = usize::from(done.second);
            self.pending[j] = false;
            if scale.is_none() {
                // towards 12 ms a picture
                let fit = self.budget * (12.0 / done.millis.max(0.5)).clamp(0.5, 1.6);
                self.budget = fit.clamp(64.0 * 36.0, 1920.0 * 1080.0);
            }
            let image = ColorImage::new([done.width, done.height], done.pixels);
            match &mut self.textures[j] {
                Some(t) => t.set(image, TextureOptions::LINEAR),
                none => {
                    *none = Some(painter.ctx().load_texture(
                        "axross-amp face",
                        image,
                        TextureOptions::LINEAR,
                    ))
                }
            }
            if j == k && (sync || !self.pending[k]) {
                break;
            }
        }
        if let Some(texture) = &self.textures[k] {
            let tint = Color32::from_white_alpha((u.v[4][3] * 255.0) as u8);
            painter
                .with_clip_rect(clip.intersect(painter.clip_rect()))
                .image(
                    texture.id(),
                    rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    tint,
                );
        }
    }
}

/// The job's face, pixel by pixel (on as many threads as there are, up to 4).
fn draw(job: &Job, feedback: &mut [V3]) -> Vec<Color32> {
    let (w, h) = (job.width, job.height);
    let aspect = w as f32 / h as f32;
    let guard = job.picture.as_ref().and_then(|p| p.lock().ok());
    let picture = guard.as_deref();
    let u = &job.u;
    let face = u.face();
    let mut pixels = vec![Color32::BLACK; w * h];
    let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    if face == Shader::Feedback as u32 {
        // (its own last frame: worked out in one go, read as it was)
        let last = feedback.to_vec();
        for y in 0..h {
            for x in 0..w {
                let uv = [(x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32];
                let c = feedback_pixel(u, uv, aspect, &last, w, h);
                feedback[y * w + x] = c;
                pixels[y * w + x] = Color32::from_rgb(to(c[0]), to(c[1]), to(c[2]));
            }
        }
        return pixels;
    }
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 4);
    let rows = h.div_ceil(threads);
    let pixel = 2.0 / h as f32;
    std::thread::scope(|scope| {
        for (part, chunk) in pixels.chunks_mut(rows * w).enumerate() {
            scope.spawn(move || {
                for (i, px) in chunk.iter_mut().enumerate() {
                    let (x, y) = (i % w, part * rows + i / w);
                    let uv = [(x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32];
                    let c = shade(face, u, uv, aspect, picture, pixel);
                    *px = if face == Shader::Phosphor as u32 {
                        // (the phosphor lies over the face's ground)
                        let a = (picture.map_or(0.0, |p| p.at(uv[0], uv[1])) * 6.0).clamp(0.0, 1.0);
                        Color32::from_rgba_premultiplied(
                            to(c[0] * a),
                            to(c[1] * a),
                            to(c[2] * a),
                            to(a),
                        )
                    } else {
                        // (dithered, as the shader: no bands)
                        let d = (hash([x as f32, y as f32]) - 0.5) / 255.0;
                        Color32::from_rgb(to(c[0] + d), to(c[1] + d), to(c[2] + d))
                    };
                }
            });
        }
    });
    pixels
}

// -- the shader's functions, on the CPU (faces.wgsl, line for line) ------------------

pub type V3 = [f32; 3];

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

fn mul(a: V3, b: V3) -> V3 {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn fract(x: f32) -> f32 {
    x - x.floor()
}

fn len2(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
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

/// Magma (matplotlib's perceptual map), the shader's polynomial.
pub fn magma(t: f32) -> V3 {
    let x = t.clamp(0.0, 1.0);
    const C: [[f32; 3]; 7] = [
        [-0.002_136_485, -0.000_749_655, -0.005_386_127],
        [0.251_660_5, 0.677_523_2, 2.494_026],
        [8.353_717, -3.577_719, 0.314_467_9],
        [-27.668_74, 14.264_73, -13.649_21],
        [52.176_14, -27.943_61, 12.944_16],
        [-50.768_52, 29.046_58, 4.234_15],
        [18.655_71, -11.489_77, -5.601_962],
    ];
    let mut out = [0.0; 3];
    for (k, o) in out.iter_mut().enumerate() {
        let mut v = C[6][k];
        for c in C[..6].iter().rev() {
            v = c[k] + x * v;
        }
        *o = v.clamp(0.0, 1.0);
    }
    out
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

fn rot(p: [f32; 2], a: f32) -> [f32; 2] {
    let (s, c) = a.sin_cos();
    [c * p[0] - s * p[1], s * p[0] + c * p[1]]
}

/// A face's colour at `uv` (0..1 over its area; `pixel`: a pixel in the
/// faces' units, 2 across the height).
pub fn shade(
    face: u32,
    u: &Uniforms,
    uv: [f32; 2],
    aspect: f32,
    picture: Option<&Picture>,
    pixel: f32,
) -> V3 {
    let p = [(uv[0] - 0.5) * aspect * 2.0, (uv[1] - 0.5) * 2.0];
    let t = u.time();
    let c = |i| u.colour(i);
    let pic = |x: f32, y: f32| picture.map_or(0.0, |pic| pic.at(x, y));
    let out = match face {
        0 => {
            let w = picture.map_or(1, |pic| pic.width.max(1)) as f32;
            magma(pic(uv[0] + u.scroll() / w, uv[1]))
        }
        1 => liquid(u, p),
        2 => fire(u, uv, aspect),
        3 => {
            let t = t * 0.6;
            let (mut field, mut tint) = (0.0, [0.0; 3]);
            for k in 0..8 {
                let fk = k as f32;
                let centre = [
                    1.3 * (t * (0.5 + 0.13 * fk) + fk * 1.7).sin(),
                    0.6 * (t * (0.4 + 0.11 * fk) + fk * 2.3).cos(),
                ];
                let level = u.band((fk + 0.5) / 8.0);
                let r = 0.14 + 0.16 * level + 0.05 * u.pulse();
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
            let star = hash([(uv[0] * aspect * 140.0).floor(), (uv[1] * 140.0).floor()]);
            if star >= 0.997 {
                col = add(col, scale(c(4), 0.6));
            }
            for k in 0..3 {
                let fk = k as f32;
                let x = uv[0] * aspect * (1.0 + 0.3 * fk);
                let foot = 0.62 - 0.12 * fk
                    + 0.07 * (x * 1.1 + t * (0.8 + 0.25 * fk) + fk).sin()
                    + 0.05 * fbm([x * 0.7, t + fk]);
                let level = u.band(0.15 + 0.3 * fk + 0.2 * uv[0]);
                let d = foot - uv[1];
                let ray = fbm([x * 3.5, t * 0.4 + fk * 3.0]);
                let rays = 0.35 + 0.9 * ray;
                let up = if d >= 0.0 {
                    (-d * (3.0 - 1.5 * level)).exp()
                } else {
                    0.0
                };
                let edge = (-d.abs() * 60.0).exp();
                let light = (up * rays + edge * 0.8) * (0.3 + 0.9 * level + 0.2 * u.pulse());
                let tint = mix3(c(2), c(3 - k % 2), (d * 3.0).clamp(0.0, 1.0));
                col = add(col, scale(tint, light * 0.55));
            }
            col
        }
        5 => tunnel(u, p),
        6 => julia(u, p),
        7 => {
            let Some(pc) = picture else { return c(0) };
            let v = pc.at(uv[0], uv[1]);
            let dx = pc.at(uv[0] + 1.0 / pc.width.max(1) as f32, uv[1]) - v;
            let dy = pc.at(uv[0], uv[1] + 1.0 / pc.height.max(1) as f32) - v;
            // (lit softly, never burnt white: the scheme's colours, the
            // light on the ridges a shade above them)
            let light = (0.75 + 2.0 * (dx - dy)).clamp(0.45, 1.1);
            scale(palette(u, smoothstep(0.05, 0.6, v) * 0.78), light)
        }
        9 => moire(u, p, pixel),
        10 => {
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
            let density = ((d - 0.2 + 0.2 * u.energy()) * (1.3 + u.pulse() * 0.15)).clamp(0.0, 1.0);
            let length = len2(warp2[0], warp2[1]).clamp(0.0, 1.0);
            let shade = mix3(c(1), c(3), length);
            no_red(add(
                mix3(c(0), shade, density),
                scale(c(4), density.powi(4) * 0.3),
            ))
        }
        11 => flyover(u, uv, aspect, 48),
        12 => crystals(u, p),
        15 => {
            let h = picture.map_or(1, |pic| pic.height.max(1)) as f32;
            waterfall_colour(u, pic(uv[0], uv[1] + u.scroll() / h))
        }
        16 => water(u, uv, aspect, picture),
        17 => attractor(u, uv, picture),
        18 => cymatics(u, p, pixel, aspect),
        19 => contours(u, p, pixel),
        _ => magma(pic(uv[0], uv[1]).max(0.0).powf(0.6) * 0.95),
    };
    [
        out[0].clamp(0.0, 1.0),
        out[1].clamp(0.0, 1.0),
        out[2].clamp(0.0, 1.0),
    ]
}

fn liquid(u: &Uniforms, p: [f32; 2]) -> V3 {
    // (the bass warps it less, the energy speeds it less: a warp that
    // followed the bass reshuffled the whole picture at each turn)
    let t = u.time() * 0.12 + u.energy_flow() * 0.1;
    let s = [p[0] * 1.1, p[1] * 1.1];
    let q = [fbm([s[0], s[1] + t]), fbm([s[0] + 5.2, s[1] + 1.3 - t])];
    let r = [
        fbm([
            s[0] + 3.5 * q[0] + 1.7 + 0.6 * t,
            s[1] + 3.5 * q[1] + 9.2 + 0.6 * t,
        ]),
        fbm([
            s[0] + 3.5 * q[0] + 8.3 - 0.4 * t,
            s[1] + 3.5 * q[1] + 2.8 - 0.4 * t,
        ]),
    ];
    let k = 3.0 + 0.5 * u.bass();
    let v = fbm([s[0] + k * r[0], s[1] + k * r[1]]);
    let base = palette(u, 0.4 + 0.45 * smoothstep(0.3, 0.72, v));
    let deep = mix3(u.colour(0), base, 0.35 + 0.65 * smoothstep(0.15, 0.7, v));
    let fold = (len2(r[0] - q[0], r[1] - q[1]) * 1.2)
        .clamp(0.0, 1.0)
        .powi(3);
    add(deep, scale(u.colour(4), fold * (0.22 + 0.1 * u.pulse())))
}

fn fire(u: &Uniforms, uv: [f32; 2], aspect: f32) -> V3 {
    let t = u.time();
    let y = 1.0 - uv[1];
    let x = uv[0] * aspect;
    let turb = fbm([x * 3.0, y * 2.0 - t - 0.6 * u.energy_flow()]);
    let lick = fbm([x * 5.0 + 3.0, y * 3.0 - t * 1.6]);
    let height = 0.52 + 0.3 * u.band(uv[0] * 0.7) + 0.08 * u.pulse();
    let shape = 1.0 - y / height.max(0.05);
    let heat = (shape * 0.85 + (turb - 0.5) * 1.2 + (lick - 0.5) * 0.6 * (1.0 - y)).clamp(0.0, 1.0);
    let h = heat.powf(1.4);
    let mut col = mix3(
        [0.02, 0.008, 0.006],
        [0.55, 0.17, 0.03],
        smoothstep(0.0, 0.35, h),
    );
    col = mix3(col, [0.95, 0.35, 0.02], smoothstep(0.3, 0.6, h));
    col = mix3(col, [1.0, 0.8, 0.25], smoothstep(0.55, 0.85, h));
    col = mix3(col, [1.0, 0.97, 0.85], smoothstep(0.85, 1.0, h));
    let tinted = mul(col, add([0.6; 3], scale(u.colour(3), 0.8)));
    // (never saturated red: its licks, in a red scheme, were red flashes)
    no_red(add(
        mix3(col, tinted, 0.1),
        scale(u.colour(0), (1.0 - smoothstep(0.0, 0.2, h)) * 0.6),
    ))
}

/// The waterfall's colours (axross's): the background, the quiet colour
/// at 0.15, the middle at 0.6, the loud at 1.
fn waterfall_colour(u: &Uniforms, v: f32) -> V3 {
    if v < 0.15 {
        mix3(u.colour(0), u.colour(1), v / 0.15)
    } else if v < 0.6 {
        mix3(u.colour(1), u.colour(2), (v - 0.15) / 0.45)
    } else {
        mix3(u.colour(2), u.colour(3), ((v - 0.6) / 0.4).min(1.0))
    }
}

fn tunnel(u: &Uniforms, p: [f32; 2]) -> V3 {
    use std::f32::consts::TAU;
    let t = u.time();
    let r = len2(p[0], p[1]).max(1e-3);
    let a = p[1].atan2(p[0]);
    let depth = 0.45 / r + t * 0.3 + 0.35 * u.bass_flow();
    let turn = a / TAU + 0.06 * (depth * 0.35 + 0.5 * u.phase()).sin();
    let fold = (2.0 * fract(turn) - 1.0).abs();
    let level = u.band(fold);
    let rr = fract(depth * 1.5);
    let ring_d = rr.min(1.0 - rr) / 1.5 * r * r / 0.45;
    let ss = fract(turn * 10.0);
    let rib_d = ss.min(1.0 - ss) / 10.0 * TAU * r;
    let ring = (-ring_d / 0.010).exp() * (1.0 - 0.8 * smoothstep(0.3, 0.85, r));
    let rib = (-rib_d / 0.008).exp();
    let checker = fract(((depth * 1.5).floor() + (turn * 10.0).floor()) * 0.5) * 2.0;
    // (panels and a lit side, as the shader's; on a light ground they fade
    // out and the checker keeps a fifth of its contrast, as there)
    let ground = u.colour(0);
    let light = smoothstep(
        0.35,
        0.9,
        0.2126 * ground[0] + 0.7152 * ground[1] + 0.0722 * ground[2],
    );
    let relief = 1.0 - light;
    let grain = 1.0 - relief * 0.2 * (1.0 - (turn * TAU * 20.0).sin() * (depth * TAU * 3.0).sin());
    let side = 1.0 - relief * 0.25 * (1.0 - (a - t * 0.2).cos());
    let tiles = 1.0 - 0.25 * (1.0 - 0.8 * light) * (1.0 - checker);
    let wall = scale(
        mix3(
            u.colour(0),
            cycle(u, (depth * 1.5).floor() * 0.04 + fold * 0.4),
            0.10 + 0.22 * level,
        ),
        tiles * grain * side,
    );
    let line_colour = cycle(u, (depth * 1.5).floor() * 0.04 + fold * 0.4 + 0.15);
    let glow = ring * (0.45 + 0.85 * level + 0.12 * u.pulse()) + rib * (0.25 + 0.6 * level);
    let c = add(wall, scale(line_colour, glow));
    let fog = smoothstep(0.03, 0.6, r);
    add(
        mix3(scale(u.colour(0), 0.6), c, fog),
        scale(
            u.colour(3),
            (-r * 9.0).exp() * (0.3 + 0.2 * u.pulse() + 0.3 * u.energy()),
        ),
    )
}

fn julia(u: &Uniforms, p: [f32; 2]) -> V3 {
    let t = u.time() * 0.07 + u.phase() * 0.03;
    let k = 0.985 + 0.006 * u.bass();
    let cc = [
        (t.cos() * 0.5 - (2.0 * t).cos() * 0.25) * k,
        (t.sin() * 0.5 - (2.0 * t).sin() * 0.25) * k,
    ];
    let zoom = 1.25 - 0.03 * u.pulse();
    let mut z = [p[0] * zoom, p[1] * zoom];
    let mut trap = 1e9f32;
    let trap_at = 0.25 * (u.time() * 0.3).sin();
    let most = 128;
    let mut i = 0;
    while i < most {
        z = [z[0] * z[0] - z[1] * z[1] + cc[0], 2.0 * z[0] * z[1] + cc[1]];
        trap = trap.min(len2(z[0] - trap_at, z[1]));
        if z[0] * z[0] + z[1] * z[1] > 256.0 {
            break;
        }
        i += 1;
    }
    let trapped = (-trap * 6.0).exp();
    if i >= most {
        return mix3(u.colour(0), u.colour(1), 0.2 + 0.5 * trapped);
    }
    let m = z[0] * z[0] + z[1] * z[1];
    let smooth = (i as f32 + 1.0 - (m.log2() * 0.5).log2()).max(0.0);
    let v = ((smooth / most as f32).sqrt() * (0.98 + 0.12 * u.energy())).clamp(0.0, 1.0);
    add(
        palette(u, v),
        scale(cycle(u, trap * 0.5 + u.time() * 0.02), trapped * 0.6),
    )
}

fn wave(phase: f32, per_pixel: f32) -> f32 {
    let aa = (1.0 - per_pixel / 3.0).clamp(0.0, 1.0);
    0.5 + 0.5 * phase.sin() * aa
}

fn moire(u: &Uniforms, p: [f32; 2], pixel: f32) -> V3 {
    // (dense rings again, interfering as moiré does - slower than they
    // were, and no brighter than 0.09 above the ground: stripes whose light
    // and dark differ by less than 0.1 are no pattern hazard)
    let t = u.time() * 0.4;
    let a = [0.35 * t.sin(), 0.25 * (t * 0.8).cos()];
    let b = [0.35 * (t * 0.7 + 2.0).sin(), 0.25 * (t * 1.1 + 1.0).cos()];
    let f1 = 26.0 + 10.0 * u.bass();
    let f2 = 28.0 + 10.0 * u.middle();
    let f3 = 18.0 + 8.0 * u.treble();
    let r1 = wave(
        len2(p[0] - a[0], p[1] - a[1]) * f1 - u.time() * 0.9,
        f1 * pixel,
    );
    let r2 = wave(
        len2(p[0] - b[0], p[1] - b[1]) * f2 + u.time() * 0.7,
        f2 * pixel,
    );
    let lines = wave(
        (p[0] * (t * 0.3).cos() + p[1] * (t * 0.3).sin()) * f3,
        f3 * pixel,
    );
    let v = r1 * r2 + 0.35 * lines * r1;
    let ground = u.colour(0);
    let c = mix3(
        ground,
        cycle(u, v * 0.4 + t * 0.1),
        smoothstep(0.15, 0.9, v),
    );
    let over = (luma(c) - luma(ground)).abs();
    if over <= 0.09 {
        return c;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..8 {
        let t = (lo + hi) * 0.5;
        if (luma(mix3(ground, c, t)) - luma(ground)).abs() > 0.09 {
            hi = t;
        } else {
            lo = t;
        }
    }
    mix3(ground, c, lo)
}

/// A colour no longer saturated red (R over R + G + B, linear, under 0.75):
/// towards a warm white until it is not (cells that change with the music
/// in a red scheme were red flashes).
fn no_red(c: V3) -> V3 {
    let mut out = c;
    for _ in 0..8 {
        let l = [out[0].powf(2.2), out[1].powf(2.2), out[2].powf(2.2)];
        let sum = l[0] + l[1] + l[2];
        // (a red too dark to flash - under 5 of WCAG's 20 - left as it is)
        if sum <= 0.0 || l[0] / sum < 0.75 || (l[0] - l[1] - l[2]) * 320.0 < 5.0 {
            break;
        }
        out = mix3(out, [1.0, 0.93, 0.84], 0.12);
    }
    out
}

/// Relative luminance of an sRGB colour (approximately: power 2.2).
fn luma(c: V3) -> f32 {
    0.2126 * c[0].max(0.0).powf(2.2)
        + 0.7152 * c[1].max(0.0).powf(2.2)
        + 0.0722 * c[2].max(0.0).powf(2.2)
}

fn terrain_height(u: &Uniforms, x: f32, z: f32) -> f32 {
    let ridge = u.band((x.abs() * 0.08).clamp(0.0, 1.0));
    fbm([x * 0.18, z * 0.18]) * 2.2 + ridge * (0.8 + 1.2 * u.bass()) * smoothstep(1.0, 6.0, x.abs())
}

fn flyover(u: &Uniforms, uv: [f32; 2], aspect: f32, steps: usize) -> V3 {
    let p = [(uv[0] - 0.5) * aspect, 0.5 - uv[1]];
    let t = u.time() * 1.5 + 1.5 * u.energy_flow();
    let origin = [0.0, 2.6, t];
    let dl = len2(len2(p[0], p[1] - 0.18), 1.0);
    let dir = [p[0] / dl, (p[1] - 0.18) / dl, 1.0 / dl];
    let horizon = mix3(u.colour(0), u.colour(3), 0.35);
    let sky = mix3(
        horizon,
        scale(u.colour(0), 0.7),
        ((p[1] + 0.12) * 2.2).clamp(0.0, 1.0),
    );
    let sl = len2(0.12, 1.0);
    let sun_dir = [0.0, 0.12 / sl, 1.0 / sl];
    let facing = (dir[0] * sun_dir[0] + dir[1] * sun_dir[1] + dir[2] * sun_dir[2]).max(0.0);
    let sun = smoothstep(0.9975, 0.9985, facing);
    let mut d = 0.1f32;
    let mut before = 0.1f32;
    let mut hit = false;
    let at = |d: f32| {
        [
            origin[0] + dir[0] * d,
            origin[1] + dir[1] * d,
            origin[2] + dir[2] * d,
        ]
    };
    let mut pos = origin;
    for _ in 0..steps {
        pos = at(d);
        let h = terrain_height(u, pos[0], pos[2]);
        let gap = pos[1] - h;
        if gap < 0.01 * d {
            hit = true;
            break;
        }
        before = d;
        d += (gap * 0.45).max(0.02 * d);
        if d > 40.0 {
            break;
        }
    }
    if hit {
        let gap = |x: f32| {
            let q = at(x);
            q[1] - terrain_height(u, q[0], q[2])
        };
        let (mut a0, mut a1) = (before, d);
        let (mut g0, mut g1) = (gap(a0), gap(a1));
        for _ in 0..4 {
            if (g1 - g0).abs() < 1e-5 {
                break;
            }
            let a2 = (a1 - g1 * (a1 - a0) / (g1 - g0)).clamp(before, d * 1.1);
            (a0, g0) = (a1, g1);
            a1 = a2;
            g1 = gap(a1);
        }
        d = a1;
        pos = at(d);
    }
    if !hit {
        let disc = mix3(
            u.colour(3),
            mix3(u.colour(3), u.colour(4), 0.6),
            ((p[1] - 0.05) * 6.0).clamp(0.0, 1.0),
        );
        return add(
            mix3(sky, disc, sun),
            scale(u.colour(3), facing.powf(40.0) * (0.35 + 0.4 * u.pulse())),
        );
    }
    let e = 0.05;
    let n = [
        terrain_height(u, pos[0] - e, pos[2]) - terrain_height(u, pos[0] + e, pos[2]),
        2.0 * e,
        terrain_height(u, pos[0], pos[2] - e) - terrain_height(u, pos[0], pos[2] + e),
    ];
    let nl = len2(len2(n[0], n[1]), n[2]);
    let n = [n[0] / nl, n[1] / nl, n[2] / nl];
    let ll = len2(len2(0.3, 0.8), 0.6);
    let light = ((n[0] * 0.3 + n[1] * 0.8 + n[2] * 0.6) / ll).clamp(0.0, 1.0);
    let height = (pos[1] / 3.0).clamp(0.0, 1.0);
    let land = scale(
        mix3(u.colour(0), u.colour(1), 0.22 + 0.45 * height),
        0.3 + 0.8 * light,
    );
    let g = [fract(pos[0] * 0.5), fract(pos[2] * 0.5)];
    let gd = g[0].min(1.0 - g[0]).min(g[1].min(1.0 - g[1])) * 2.0;
    let grid = (1.0 - smoothstep(0.0, 0.008 + 0.005 * d, gd)) * (-d * 0.05).exp();
    let level = u.band((pos[0].abs() * 0.08).clamp(0.0, 1.0));
    let lines = scale(
        mix3(u.colour(2), u.colour(3), level),
        grid * (0.55 + 0.9 * level + 0.4 * u.pulse()),
    );
    let rim = scale(
        u.colour(3),
        (1.0 - n[1]).clamp(0.0, 1.0).powi(2) * 0.5 * (0.5 + level),
    );
    let fog = 1.0 - (-d * 0.09).exp();
    mix3(add(add(land, lines), rim), horizon, fog)
}

fn crystals(u: &Uniforms, p: [f32; 2]) -> V3 {
    let t = u.time() * 0.7;
    let q = [p[0] * 3.2, p[1] * 3.2];
    let cell = [q[0].floor(), q[1].floor()];
    let (mut f1, mut f2) = (9.0f32, 9.0f32);
    let mut id = [0.0f32; 2];
    for j in -1..=1 {
        for i in -1..=1 {
            let g = [cell[0] + i as f32, cell[1] + j as f32];
            let o = [hash(g), hash([g[0] + 17.3, g[1] + 17.3])];
            let at = [
                g[0] + 0.5 + 0.4 * (t + std::f32::consts::TAU * o[0]).sin(),
                g[1] + 0.5 + 0.4 * (t + std::f32::consts::TAU * o[1]).sin(),
            ];
            let dist = len2(q[0] - at[0], q[1] - at[1]);
            if dist < f1 {
                f2 = f1;
                f1 = dist;
                id = g;
            } else if dist < f2 {
                f2 = dist;
            }
        }
    }
    let which = hash([id[0] + 3.1, id[1] + 3.1]);
    let level = u.band(which);
    let edge = 1.0 - smoothstep(0.0, 0.08 + 0.05 * u.pulse(), f2 - f1);
    // (each cell lit evenly, its edges softer: cells of a size, each light
    // in the middle and dark at the rim, lay in a grating over 47 %)
    // (each cell its own colour, all of one luminance: colourful, moving,
    // and no grating of light and dark)
    let raw = palette(u, 0.2 + 0.75 * level);
    let k = (luma(palette(u, 0.55)).max(0.02) / luma(raw).max(1e-4)).powf(1.0 / 2.2);
    let even = [
        (raw[0] * k).min(1.0),
        (raw[1] * k).min(1.0),
        (raw[2] * k).min(1.0),
    ];
    let face = scale(no_red(even), 0.93 + 0.07 * (1.0 - f1));
    let dx = q[0] - id[0] - 0.5 + 1e-4;
    let dy = q[1] - id[1] - 0.5 + 1e-4;
    let l = len2(dx, dy);
    let facet = 0.5 + 0.5 * (dx / l * 0.6 - dy / l * 0.8);
    add(
        scale(face, 0.96 + 0.04 * facet),
        scale(u.colour(4), edge * (0.02 + 0.04 * level)),
    )
}

// -- the faces added since (faces.wgsl's, line for line) ---------------------------

fn water(u: &Uniforms, uv: [f32; 2], aspect: f32, picture: Option<&Picture>) -> V3 {
    let Some(pc) = picture else {
        return u.colour(0);
    };
    let (dx, dy) = (1.5 / pc.width.max(1) as f32, 1.5 / pc.height.max(1) as f32);
    let h = pc.at(uv[0], uv[1]);
    let l = pc.at(uv[0] - dx, uv[1]);
    let r = pc.at(uv[0] + dx, uv[1]);
    let t = pc.at(uv[0], uv[1] - dy);
    let b = pc.at(uv[0], uv[1] + dy);
    let slope = [(r - l) * 6.0, (b - t) * 6.0];
    let q = [
        uv[0] * aspect * 2.2 + slope[0] * 0.9,
        uv[1] * 2.2 + slope[1] * 0.9,
    ];
    let time = u.time();
    let floor = fbm([q[0] + time * 0.02, q[1]]);
    let n = [uv[0] * aspect * 2.2, uv[1] * 2.2];
    let net = (noise([n[0] * 2.3 + time * 0.05, n[1] * 2.3])
        - noise([n[0] * 3.1 + 7.3, n[1] * 3.1 + 2.1 - time * 0.04]))
    .abs();
    let caustic = (1.0 - net * 2.5).clamp(0.0, 1.0).powi(2);
    let light = (-(slope[0] * 0.6 + slope[1] * 0.8)).clamp(-1.0, 1.0);
    let lit = light.max(0.0);
    let shade = (-light).max(0.0);
    let crest = ((h - 0.5) * 3.0).clamp(0.0, 1.0);
    let ground = u.colour(0);
    let k = 1.0 - 0.65 * smoothstep(0.6, 0.95, (luma(u.colour(3)) - luma(ground)).abs());
    let c = if luma(ground) > 0.4 {
        let ink = mix3(u.colour(2), u.colour(3), 0.5);
        let pool = mix3(
            mix3(ground, u.colour(1), 0.32 + 0.15 * floor),
            ground,
            0.3 * uv[1],
        );
        let ring = smoothstep(0.22, 0.6, len2(slope[0], slope[1]));
        let c = mix3(
            pool,
            ink,
            ((shade * 0.28 + crest * 0.1 + ring * 0.1) * k + caustic * 0.36).clamp(0.0, 0.85),
        );
        mix3(c, [1.0; 3], (lit * 0.28 + lit.powi(4) * 0.3) * k)
    } else {
        let pool = mix3(
            mix3(ground, u.colour(1), 0.5 + 0.2 * floor),
            ground,
            0.3 * uv[1],
        );
        let c = add(pool, scale(u.colour(2), caustic * 0.1));
        let c = add(
            add(c, scale(u.colour(3), lit * 0.55 * k)),
            scale(c, -shade * 0.5 * k),
        );
        add(
            add(c, scale(u.colour(4), lit.powi(3) * 0.55 * k)),
            scale(u.colour(2), crest * 0.22 * k),
        )
    };
    no_red(toward_grey(c))
}

/// A colour near saturated red taken smoothly toward a grey as light
/// (faces.wgsl's `toward_grey`).
fn toward_grey(c: V3) -> V3 {
    let l = [
        c[0].max(0.0).powf(2.2),
        c[1].max(0.0).powf(2.2),
        c[2].max(0.0).powf(2.2),
    ];
    let sum = l[0] + l[1] + l[2];
    if sum <= 0.0 {
        return c;
    }
    let k = smoothstep(0.5, 0.7, l[0] / sum);
    let g = luma(c).powf(1.0 / 2.2);
    mix3(c, [g, g, g], k * 0.85)
}

fn attractor(u: &Uniforms, uv: [f32; 2], picture: Option<&Picture>) -> V3 {
    let Some(pc) = picture else {
        return u.colour(0);
    };
    let size = [pc.width.max(1) as f32, pc.height.max(1) as f32];
    let d = pc.at(uv[0], uv[1]);
    let mut halo = 0.0;
    for k in 0..4 {
        let a = k as f32 * std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_4;
        halo += pc.at(
            uv[0] + a.cos() * 2.5 / size[0],
            uv[1] + a.sin() * 2.5 / size[1],
        );
    }
    halo *= 0.25;
    let c = |i| u.colour(i);
    let body = mix3(c(1), c(2), smoothstep(0.1, 0.6, d));
    let mut out = mix3(c(0), body, smoothstep(0.0, 0.35, d));
    out = mix3(out, c(3), smoothstep(0.45, 0.9, d) * 0.7);
    out = add(out, scale(c(4), d.powi(3) * 0.35));
    out = mix3(
        out,
        mix3(c(0), c(1), 0.6),
        smoothstep(0.0, 0.3, halo) * (1.0 - smoothstep(0.0, 0.2, d)) * 0.35,
    );
    no_red(out)
}

fn chladni(q: [f32; 2], n: f32, m: f32, sign: f32) -> f32 {
    let pi = std::f32::consts::PI;
    (n * pi * q[0]).cos() * (m * pi * q[1]).cos()
        + sign * (m * pi * q[0]).cos() * (n * pi * q[1]).cos()
}

fn plate(u: &Uniforms, q: [f32; 2]) -> f32 {
    let (e, k) = (u.extra(1), u.extra(2));
    let a = chladni(q, e[0], e[1], k[1]);
    let b = chladni(q, e[2], e[3], k[2]);
    a + (b - a) * k[0]
}

fn cymatics(u: &Uniforms, p: [f32; 2], pixel: f32, aspect: f32) -> V3 {
    let half = [0.84, 0.84];
    let edge = (p[0].abs() - half[0]).max(p[1].abs() - half[1]) / 0.84 + 1.0;
    let t = u.time();
    let level = u.extra(2)[3];
    let shake = 0.005 + 0.003 * level + 0.018 * u.pulse();
    let q = [
        p[0] / half[0] + shake * (t * 7.0 + p[1] * 9.0).sin(),
        p[1] / half[1] + shake * (t * 6.0 + p[0] * 8.0).cos(),
    ];
    let ground = u.colour(0);
    let light = luma(ground) > 0.4;
    if edge > 1.03 {
        let shade = 1.0 - 0.3 * (1.0 - smoothstep(1.03, 1.14, edge));
        let tone = u.band((0.5 + p[0] / (2.0 * aspect)).clamp(0.0, 1.0));
        let spill = tone * (-(edge - 1.0) * 1.6).exp() * 0.09 * smoothstep(1.03, 1.2, edge);
        if light {
            return mix3(scale(ground, shade), u.colour(1), spill * 0.8);
        }
        return add(
            scale(ground, shade),
            scale(mix3(u.colour(1), u.colour(2), tone), spill),
        );
    }
    let f = plate(u, q);
    let e = 0.01;
    let gx = (plate(u, [q[0] + e, q[1]]) - f) / e / half[0];
    let gy = (plate(u, [q[0], q[1] + e]) - f) / e / half[1];
    let d = f.abs() / len2(gx, gy).max(0.6);
    let width = (0.003 + 0.004 * level) * 0.84;
    let core = 1.0 - smoothstep(width, width + 1.5 * pixel, d);
    let grain = noise([p[0] * 107.0 + t * 3.0, p[1] * 107.0]);
    let scatter = (1.0 - smoothstep(0.0, width * (5.0 + 12.0 * u.pulse()) + 2.0 * pixel, d))
        * smoothstep(0.55 - 0.3 * u.pulse(), 0.8, grain);
    let cover = (3.0 * width / pixel).clamp(0.0, 1.0)
        * if light {
            0.55 + 0.15 * ((0.012 - pixel) / 0.006).clamp(0.0, 1.0)
        } else {
            1.0
        };
    let sand = (core * (0.8 + 0.2 * grain) + scatter * 0.55).clamp(0.0, 1.0) * cover;
    let metal = add(
        mix3(ground, u.colour(1), 0.2),
        scale(u.colour(4), 0.06 * (1.0 - 0.5 * len2(q[0], q[1]))),
    );
    let tone = u.band((len2(q[0], q[1]) * 0.75).clamp(0.0, 1.0));
    let lit = mix3(
        metal,
        mix3(u.colour(3), u.colour(4), 0.3 + 0.4 * tone),
        0.85,
    );
    let mut c = mix3(metal, lit, sand * (0.7 + 0.15 * u.energy() + 0.15 * tone));
    c = mix3(
        c,
        mix3(metal, u.colour(4), 0.3),
        smoothstep(0.985, 1.0, edge) * (1.0 - smoothstep(1.015, 1.03, edge)),
    );
    c = mix3(c, ground, smoothstep(1.015, 1.03, edge));
    no_red(c)
}

fn land(u: &Uniforms, p: [f32; 2]) -> f32 {
    let t = u.time() * 0.02;
    let q = [p[0] * 0.8 + t, p[1] * 0.8 - 0.7 * t];
    let mut h = (0.5 * noise(q)
        + 0.25 * noise([q[0] * 2.03 + 1.7, q[1] * 2.03 + 9.2])
        + 0.125 * noise([q[0] * 4.1 + 4.6, q[1] * 4.1 + 2.9]))
        * 0.9;
    let (e1, e2) = (u.extra(1), u.extra(2));
    let hills = [e1[0], e1[1], e1[2], e1[3], e2[0], e2[1]];
    for (k, hill) in hills.iter().enumerate() {
        let fk = k as f32;
        let c = [
            (fk - 2.5) * 0.55 + 0.25 * (t * 3.0 + fk * 1.9).sin(),
            0.45 * (fk * 2.3 + t * 2.0).sin(),
        ];
        let d = [p[0] - c[0], p[1] - c[1]];
        h += hill * 0.42 * (-(d[0] * d[0] + d[1] * d[1]) * 3.5).exp();
    }
    h
}

fn contours(u: &Uniforms, p: [f32; 2], pixel: f32) -> V3 {
    let h = land(u, p);
    let hx = land(u, [p[0] + pixel, p[1]]) - h;
    let hy = land(u, [p[0], p[1] + pixel]) - h;
    let slope = len2(hx, hy) / pixel;
    let spacing = 0.05;
    let v = h / spacing;
    let apart = spacing / slope.max(1e-5);
    let d = (fract(v + 0.5) - 0.5).abs() * apart;
    let line = (1.0 - smoothstep(0.004, 0.004 + pixel, d))
        * ((apart - 0.027) / 0.04).clamp(0.0, 1.0)
        * (0.012 / pixel).clamp(0.0, 1.0);
    let major = 1.0
        - smoothstep(
            0.02,
            0.12,
            (fract((v + 0.5).floor() / 5.0 + 0.5) - 0.5).abs(),
        );
    let high = (h * 0.9).clamp(0.0, 1.0);
    let tint = palette(u, 0.12 + 0.62 * high);
    let light = luma(u.colour(0)) > 0.4;
    let stark = smoothstep(0.6, 0.95, (luma(u.colour(3)) - luma(u.colour(0))).abs());
    let fill = mix3(
        u.colour(0),
        tint,
        if light {
            let (pale, deep) = (0.3 + 0.3 * high, 0.18 + 0.42 * high);
            pale + (deep - pale) * stark
        } else {
            0.55
        },
    );
    let shade = (0.96 + (hy - hx) / pixel * if light { 0.1 } else { 0.3 }).clamp(0.86, 1.05);
    let ink = if light {
        let land = scale(fill, shade);
        let lf = luma(land).max(0.001);
        scale(
            land,
            ((lf - 0.05 - 0.04 * major).max(0.0) / lf).powf(1.0 / 2.2),
        )
    } else {
        mix3(fill, u.colour(4), 0.17 + 0.17 * major)
    };
    no_red(mix3(scale(fill, shade), ink, line))
}

/// The feedback face's next pixel from its last frame `last` (w x h).
/// One of the presets' waves (faces.wgsl's `wave_layer`).
fn wave_layer(u: &Uniforms, kind: i32, p: [f32; 2], aspect: f32, t: f32) -> V3 {
    use std::f32::consts::{PI, TAU};
    let r = len2(p[0], p[1]);
    let a = p[1].atan2(p[0]);
    let hue = u.extra(2)[0];
    match kind {
        1 => {
            let x = (p[0].abs() / (0.5 * aspect)).clamp(0.0, 1.0);
            let level = u.band(x);
            let y = (0.06 + 0.22 * level) * (p[0] * 9.0 + t * 2.0).sin();
            let line = (-(p[1] - y).abs() * (150.0 - 60.0 * u.pulse())).exp() * (0.4 + 0.9 * level);
            scale(cycle(u, p[0] * 0.4 + t * hue), line)
        }
        2 => {
            let level = u.band(a.abs() / PI);
            let arms = 0.5 + 0.5 * (3.0 * a - r.max(1e-3).ln() * 5.0 + t * 1.5).cos();
            let lit = arms.powf(24.0)
                * smoothstep(0.03, 0.12, r)
                * (-r * 2.5).exp()
                * (0.5 + 1.2 * level);
            scale(cycle(u, a / TAU + r + t * hue), lit)
        }
        3 => {
            let mut c = [0.0; 3];
            for k in 0..5 {
                let fk = k as f32;
                let level = u.band((fk + 0.5) / 5.0);
                let turn = t * (0.6 + 0.1 * fk) + fk * TAU / 5.0;
                let reach = 0.22 + 0.15 * level;
                let d = len2(p[0] - turn.cos() * reach, p[1] - turn.sin() * reach);
                c = add(
                    c,
                    scale(
                        cycle(u, fk / 5.0 + t * hue),
                        (-d * (70.0 - 30.0 * u.pulse())).exp() * (0.5 + level),
                    ),
                );
            }
            c
        }
        4 => {
            let level = u.band(a.abs() / PI);
            let petals = 0.5 + 0.5 * (5.0 * a + t * 0.7).cos();
            let edge = (0.12 + 0.16 * level) * (0.55 + 0.45 * petals) + 0.04 * u.bass();
            let line = (-(r - edge).abs() * 140.0).exp() * (0.4 + 0.8 * level);
            scale(cycle(u, a / TAU * 2.0 + r + t * hue), line)
        }
        5 => {
            let k = ((a / TAU + 0.5) * 16.0).floor();
            let centre = (k + 0.5) / 16.0 * TAU - PI;
            let level = u.band(centre.abs() / PI);
            let side = (a - centre).sin().abs() * r;
            let reach = 0.08 + 0.32 * level;
            let lit = (-side * 220.0).exp()
                * smoothstep(0.04, 0.07, r)
                * (1.0 - smoothstep(reach - 0.03, reach, r))
                * (0.35 + 0.8 * level);
            scale(cycle(u, k / 16.0 + t * hue), lit)
        }
        _ => {
            let level = u.band(a.abs() / PI);
            let ring = 0.18 + 0.22 * level + 0.06 * u.bass();
            let line = (-(r - ring).abs() * (160.0 - 80.0 * u.pulse())).exp() * (0.4 + 0.9 * level);
            scale(cycle(u, a / TAU + t * hue), line)
        }
    }
}

pub fn feedback_pixel(
    u: &Uniforms,
    uv: [f32; 2],
    aspect: f32,
    last: &[V3],
    w: usize,
    h: usize,
) -> V3 {
    let t = u.time();
    let p = [(uv[0] - 0.5) * aspect, uv[1] - 0.5];
    let motion = u.extra(0);
    let look = u.extra(1);
    let mut q = rot(p, motion[0]);
    q = [q[0] * motion[1], q[1] * motion[1]];
    q = [
        q[0] + motion[2] * (q[1] * look[3] + t).sin(),
        q[1] + motion[2] * (q[0] * look[3] - t * 0.8).cos(),
    ];
    let mirror = u.extra(2)[1];
    q[0] += (q[0].abs() - q[0]) * mirror.clamp(0.0, 1.0);
    q[1] += (q[1].abs() - q[1]) * (mirror - 1.0).clamp(0.0, 1.0);
    let src = [q[0] / aspect + 0.5, q[1] + 0.5];
    let mut prev = if (0.0..=1.0).contains(&src[0]) && (0.0..=1.0).contains(&src[1]) {
        sample(last, w, h, src)
    } else {
        [0.0; 3]
    };
    prev = scale(prev, motion[3]);
    prev = [
        prev[0] * 0.985 + prev[2] * 0.012,
        prev[1] * 0.99 + prev[0] * 0.008,
        prev[2] * 0.985 + prev[1] * 0.012,
    ];
    let wave = mix3(
        wave_layer(u, (look[0] + 0.5) as i32, p, aspect, t),
        wave_layer(u, (look[1] + 0.5) as i32, p, aspect, t),
        look[2],
    );
    let cell = [
        (uv[0] * aspect * 90.0).floor() + (t * 8.0).floor(),
        (uv[1] * 90.0).floor() + (t * 8.0).floor(),
    ];
    let spark = smoothstep(0.996 - 0.01 * u.pulse(), 1.0, hash(cell));
    let layer = add(wave, scale(u.colour(4), spark * u.pulse()));
    let floor = 1.5 / 255.0;
    let out = add(
        [
            (prev[0] - floor).max(0.0),
            (prev[1] - floor).max(0.0),
            (prev[2] - floor).max(0.0),
        ],
        layer,
    );
    [
        out[0].clamp(0.0, 1.0),
        out[1].clamp(0.0, 1.0),
        out[2].clamp(0.0, 1.0),
    ]
}

fn sample(img: &[V3], w: usize, h: usize, uv: [f32; 2]) -> V3 {
    if img.len() != w * h || w == 0 {
        return [0.0; 3];
    }
    let fx = (uv[0] * w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
    let fy = (uv[1] * h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
    let (i, j) = (fx as usize, fy as usize);
    let (i2, j2) = ((i + 1).min(w - 1), (j + 1).min(h - 1));
    let (tx, ty) = (fx - i as f32, fy - j as f32);
    let top = mix3(img[j * w + i], img[j * w + i2], tx);
    let bottom = mix3(img[j2 * w + i], img[j2 * w + i2], tx);
    mix3(top, bottom, ty)
}

/// Where the shader faces are drawn, as measured: the GPU's cost of a pixel
/// and the CPU's (its threads together), each in ms.
#[derive(Clone, Copy, Debug)]
pub struct Choice {
    pub gpu: bool,
    pub gpu_ms_per_pixel: f32,
    pub cpu_ms_per_pixel: f32,
}

impl Choice {
    /// Too slow to draw a face of the usual size (620 x 300) in 4 ms: the
    /// window's frame rate is lowered too.
    pub fn slow(&self) -> bool {
        self.slow_for(self.gpu)
    }

    /// As [`Choice::slow`], for the GPU's way (`gpu`) or the CPU's.
    pub fn slow_for(&self, gpu: bool) -> bool {
        let per_pixel = if gpu {
            self.gpu_ms_per_pixel
        } else {
            self.cpu_ms_per_pixel
        };
        per_pixel * 620.0 * 300.0 > 4.0
    }
}

/// Measure a shader face (the liquid's noise: among the dearest) on the GPU
/// (320 x 180, a few frames) and on the CPU (160 x 90): the cheaper draws.
pub fn choose(render_state: &egui_wgpu::RenderState) -> Choice {
    let mut u = Uniforms::default();
    u.v[4] = [Shader::Liquid as u32 as f32, 0.0, 0.0, 1.0];
    for i in 0..5 {
        u.v[5 + i] = [0.2 * i as f32, 0.5, 0.8 - 0.1 * i as f32, 1.0];
    }
    // (the first frame builds the pipeline's state: not counted). What
    // counts is the CPU's time where there is one to count: a software
    // rasterizer spends it on several threads while the clock runs once
    let _ = measure(render_state, u, (320, 180), 1);
    let cpu_before = process_cpu();
    let (gpu_wall, _) = measure(render_state, u, (320, 180), 12);
    let gpu_ms = match (cpu_before, process_cpu()) {
        (Some(a), Some(b)) => (b - a) * 1000.0 / 12.0,
        _ => gpu_wall,
    };
    let started = Instant::now();
    let (w, h) = (160, 90);
    for y in 0..h {
        for x in 0..w {
            let uv = [(x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32];
            std::hint::black_box(shade(Shader::Liquid as u32, &u, uv, 16.0 / 9.0, None, 0.01));
        }
    }
    // (its threads share the work, and its time: the time a pixel)
    let cpu = started.elapsed().as_secs_f32() * 1000.0 / (w * h) as f32;
    let gpu = (gpu_ms as f32 / (320.0 * 180.0)).max(0.0);
    Choice::of(gpu, cpu)
}

impl Choice {
    /// The cheaper way by what a pixel costs each (ms).
    pub fn of(gpu: f32, cpu: f32) -> Choice {
        let choice = Choice {
            gpu: gpu.is_finite() && gpu <= cpu,
            gpu_ms_per_pixel: gpu,
            cpu_ms_per_pixel: cpu,
        };
        // (a "GPU" too slow for a face of the usual size is a software
        // rasterizer - the CPU's work either way: there the CPU's thread draws,
        // its picture shown scaled up; the GPU's way cost twice the frame in a
        // large window - 2560 x 1440 on lavapipe: a frame every two seconds at
        // half a core, the CPU's way a frame a second at 0.4: review 14)
        Choice {
            gpu: choice.gpu && !choice.slow_for(true),
            ..choice
        }
    }
}

/// The CPU seconds this process has used, where the system tells (Linux).
pub fn process_cpu() -> Option<f64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let rest = stat.rsplit(')').next()?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let ticks = |i: usize| fields.get(i).and_then(|v| v.parse::<f64>().ok());
    Some((ticks(11)? + ticks(12)?) / 100.0)
}

/// What the GPU takes for one of its faces at `size` (pixels): rendered
/// `frames` times into a texture of its own (nothing read back), timed by
/// the GPU's timestamp queries where the device offers them, else by the
/// clock round each submission. Milliseconds a frame, and whether the
/// GPU's own timestamps measured it. (Tests and measurements.)
#[doc(hidden)]
pub fn measure(
    render_state: &egui_wgpu::RenderState,
    mut u: Uniforms,
    size: (u32, u32),
    frames: u32,
) -> (f64, bool) {
    let device = &render_state.device;
    let queue = &render_state.queue;
    let renderer = render_state.renderer.read();
    let Some(faces) = renderer.callback_resources.get::<Faces>() else {
        return (f64::NAN, false);
    };
    let target = Faces::texture(device, size, render_state.target_format, true);
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    u.v[0] = [0.0, 0.0, size.0 as f32, size.1 as f32];
    u.v[1] = u.v[0];
    u.v[3][3] = 0.0;
    u.v[4][1] = if faces.linear { 1.0 } else { 0.0 };
    let mut bytes = [0u8; VEC4S * 16];
    for (k, v) in u.v.iter().flatten().enumerate() {
        bytes[k * 4..k * 4 + 4].copy_from_slice(&v.to_ne_bytes());
    }
    let slot = &faces.slots[SLOTS - 1];
    queue.write_buffer(&slot.uniforms, 0, &bytes);
    let timestamps = device.features().contains(wgpu::Features::TIMESTAMP_QUERY);
    let queries = timestamps.then(|| {
        device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("axross-amp face timing"),
            ty: wgpu::QueryType::Timestamp,
            count: 2 * frames,
        })
    });
    let resolve = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("axross-amp face timing"),
        size: 16 * frames as u64,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("axross-amp face timing"),
        size: 16 * frames as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let started = Instant::now();
    for frame in 0..frames {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let writes = queries.as_ref().map(|q| wgpu::RenderPassTimestampWrites {
                query_set: q,
                beginning_of_pass_write_index: Some(2 * frame),
                end_of_pass_write_index: Some(2 * frame + 1),
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axross-amp face timing"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: writes,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&faces.pipeline);
            pass.set_bind_group(0, &slot.group, &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some(q) = &queries
            && frame + 1 == frames
        {
            encoder.resolve_query_set(q, 0..2 * frames, &resolve, 0);
            encoder.copy_buffer_to_buffer(&resolve, 0, &read, 0, 16 * frames as u64);
        }
        queue.submit([encoder.finish()]);
        let _ = device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
    }
    let wall = started.elapsed().as_secs_f64() * 1000.0 / frames as f64;
    if queries.is_none() {
        return (wall, false);
    }
    let slice = read.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    let Ok(data) = slice.get_mapped_range() else {
        return (wall, false);
    };
    let ticks: Vec<u64> = data
        .as_chunks::<8>()
        .0
        .iter()
        .map(|b| u64::from_ne_bytes(*b))
        .collect();
    drop(data);
    read.unmap();
    let period = queue.get_timestamp_period() as f64;
    let total: f64 = ticks
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[a, b]| b.saturating_sub(*a) as f64 * period / 1e6)
        .sum();
    (total / frames as f64, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tunnel_has_no_seam() {
        // the bands from quiet to loud round the tunnel: wherever the angle
        // wraps, the colour must go on smoothly
        let mut u = Uniforms::default();
        for k in 0..32 {
            u.v[10 + k / 4][k % 4] = k as f32 / 31.0;
        }
        for (i, c) in [
            [0.05, 0.05, 0.08],
            [0.2, 0.8, 0.4],
            [0.9, 0.8, 0.1],
            [0.9, 0.3, 0.2],
            [1.0, 1.0, 1.0],
        ]
        .iter()
        .enumerate()
        {
            u.v[5 + i] = [c[0], c[1], c[2], 1.0];
        }
        u.v[4][0] = Shader::Tunnel as u32 as f32;
        for time in [0.0f32, 1.3, 7.7] {
            u.v[2][0] = time;
            let steps = 4000;
            let mut last: Option<V3> = None;
            let mut worst = 0.0f32;
            for k in 0..=steps {
                let a = k as f32 / steps as f32 * std::f32::consts::TAU;
                // a circle at a third of the height round the middle; the
                // radius where no groove lies across it
                let uv = [0.5 + a.cos() * 0.3 / (16.0 / 9.0), 0.5 + a.sin() * 0.3];
                let c = shade(5, &u, uv, 16.0 / 9.0, None, 0.001);
                if let Some(l) = last {
                    worst = worst.max((0..3).map(|i| (c[i] - l[i]).abs()).fold(0.0, f32::max));
                }
                last = Some(c);
            }
            // (a seam where the bands jump: 0.55; the grooves: 0.05)
            assert!(
                worst < 0.15,
                "a step of {worst} round the tunnel at {time} s"
            );
        }
    }

    #[test]
    fn magma_runs_from_black_to_light() {
        let dark = magma(0.0);
        let light = magma(1.0);
        assert!(dark.iter().all(|v| *v < 0.05), "{dark:?}");
        assert!(light[0] > 0.9 && light[1] > 0.9, "{light:?}");
        // and brighter all the way
        let lum = |c: V3| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        for k in 0..20 {
            assert!(lum(magma((k + 1) as f32 / 20.0)) > lum(magma(k as f32 / 20.0)));
        }
    }

    /// A software rasterizer's "GPU" (too slow for a face of the usual
    /// size) leaves the shader faces to the CPU's thread, even where a
    /// pixel measured cheaper on it (review 14: plasma at 2560 x 1440 on
    /// lavapipe took half a core at a frame every two seconds that way).
    #[test]
    fn a_software_gpu_leaves_the_shader_faces_to_the_cpu() {
        // lavapipe's, as measured here
        let soft = Choice::of(3.76e-4, 8.04e-4);
        assert!(!soft.gpu);
        assert!(soft.slow());
        // a real GPU: its way
        let real = Choice::of(2e-6, 8e-4);
        assert!(real.gpu && !real.slow());
    }
}
