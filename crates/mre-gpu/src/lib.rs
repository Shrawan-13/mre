//! The wgpu renderer.
//!
//! It implements the same [`Backend`] contract as the software rasteriser, so
//! layout, widgets, damage tracking and Lua are untouched. Everything in a
//! frame is one instanced draw into a persistent canvas texture, followed by
//! one blit to the swapchain and exactly one submit.
//!
//! The persistent texture is what keeps damage tracking meaningful on a GPU:
//! unchanged pixels stay in the texture, the frame only draws the bands the
//! scene marked dirty, and the blit presents the whole result. A scroll that
//! cannot be handled as a copy simply falls back to a repaint — the CPU path
//! makes the same decision for fractional offsets.
//!
//! Text uses the reference rasteriser's glyph coverage to build an atlas, so
//! both backends render the same hinted outlines at the same subpixel phases.

mod atlas;

use atlas::{Atlas, GlyphKey};
use mre_core::{Point, Rect, Rgba8};
use mre_paint::{Backend, BackendKind, Paint};
use mre_raster::glyph_coverage;
use mre_text::{ShapedText, TextEngine};
use std::fmt;
use std::sync::Arc;

const KIND_FILL: f32 = 0.0;
const KIND_STROKE: f32 = 1.0;
const KIND_LINE: f32 = 2.0;
const KIND_GLYPH: f32 = 3.0;
/// Quarter-pixel phases, exactly as in the reference rasteriser.
const PHASE_STEPS: u16 = 16;
const INITIAL_INSTANCES: usize = 4096;
const INSTANCE_BYTES: usize = 96;

/// One instanced quad. 24 floats = 96 bytes, `Copy` so a frame is one upload.
/// Fields are read by the GPU, not by Rust, which `dead_code` cannot see.
#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    rect: [f32; 4],
    color: [f32; 4],
    /// radius, stroke width, kind, device-space flag
    params: [f32; 4],
    /// line endpoints for `KIND_LINE`
    line: [f32; 4],
    /// clip as x0, y0, x1, y1
    clip: [f32; 4],
    /// atlas rectangle, normalised
    uv: [f32; 4],
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    resolution: [f32; 2],
    scale: f32,
    _pad: f32,
}

/// GPU initialisation failures. The window host treats these as "use software".
#[derive(Debug)]
pub enum GpuError {
    /// No surface could be created for the window.
    Surface(String),
    /// No adapter compatible with the surface exists.
    Adapter,
    /// A device could not be opened.
    Device(String),
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Surface(error) => write!(f, "GPU surface: {error}"),
            Self::Adapter => write!(f, "no compatible GPU adapter"),
            Self::Device(error) => write!(f, "GPU device: {error}"),
        }
    }
}

impl std::error::Error for GpuError {}

/// What hardware the renderer actually picked. A "GPU backend" can still be
/// backed by a CPU adapter (lavapipe, SwiftShader), which is important to know
/// before claiming hardware acceleration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuInfo {
    /// Adapter name reported by the driver.
    pub name: String,
    /// API in use: Vulkan, Metal, DX12, GL or WebGPU.
    pub backend: String,
    /// Discrete, integrated, virtual or CPU.
    pub device_type: String,
    /// Driver name and version.
    pub driver: String,
    /// True when the adapter is a software renderer.
    pub software: bool,
}

impl fmt::Display for GpuInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}, {}, driver {}){}",
            self.name,
            self.backend,
            self.device_type,
            self.driver,
            if self.software {
                " [SOFTWARE ADAPTER]"
            } else {
                ""
            }
        )
    }
}

impl From<&wgpu::AdapterInfo> for GpuInfo {
    fn from(info: &wgpu::AdapterInfo) -> Self {
        Self {
            name: info.name.clone(),
            backend: format!("{:?}", info.backend),
            device_type: format!("{:?}", info.device_type),
            driver: if info.driver_info.is_empty() {
                info.driver.clone()
            } else {
                format!("{} {}", info.driver, info.driver_info)
                    .trim()
                    .to_string()
            },
            software: matches!(
                info.device_type,
                wgpu::DeviceType::Cpu | wgpu::DeviceType::Other
            ),
        }
    }
}

/// Requests an adapter without a surface, for diagnostics and CI probes.
pub fn probe() -> Result<GpuInfo, GpuError> {
    let instance = wgpu::Instance::default();
    // Prefer real hardware. Headless machines often have no adapter at all,
    // but a software Vulkan driver (lavapipe) can still exercise the wgpu
    // path; report it honestly via `software` rather than claiming a GPU.
    let hardware = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }));
    let adapter = match hardware {
        Ok(adapter) => adapter,
        Err(_) => pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: true,
        }))
        .map_err(|_| GpuError::Adapter)?,
    };
    Ok(GpuInfo::from(&adapter.get_info()))
}

/// Diagnostics for the GPU path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuStats {
    /// Draw calls recorded this frame (the instanced draw plus the blit).
    pub calls: u64,
    /// Quads recorded this frame.
    pub instances: usize,
    /// Distinct glyphs in the atlas so far.
    pub glyphs: usize,
}

pub struct GpuBackend {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    canvas: wgpu::Texture,
    canvas_view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    blit_bind_group: wgpu::BindGroup,
    globals: wgpu::Buffer,
    instances: wgpu::Buffer,
    instance_capacity: usize,
    atlas: Atlas,
    info: GpuInfo,
    ops: Vec<Instance>,
    clip: Vec<Rect>,
    clear_all: Option<Rgba8>,
    scale: f32,
    width: u32,
    height: u32,
    calls: u64,
}

impl GpuBackend {
    /// Creates a renderer for a winit window. The window is moved into the
    /// surface so the surface can outlive the borrow of the event loop.
    pub fn for_window(
        window: Arc<winit::window::Window>,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<Self, GpuError> {
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window)
            .map_err(|error| GpuError::Surface(error.to_string()))?;
        Self::from_surface(instance, surface, width, height, scale)
    }

    /// Creates a renderer from a surface supplied by a host.
    pub fn from_surface(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<Self, GpuError> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|_| GpuError::Adapter)?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("mre device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| GpuError::Device(error.to_string()))?;
        let info = GpuInfo::from(&adapter.get_info());

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .or_else(|| capabilities.formats.first().copied())
            .ok_or_else(|| GpuError::Surface("no surface formats".into()))?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: capabilities.present_modes[0],
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let (pipeline, blit_pipeline, bind_group_layout) =
            Self::pipelines(&device, wgpu::TextureFormat::Rgba8Unorm, format);
        let canvas = Self::make_canvas(&device, config.width, config.height);
        let canvas_view = canvas.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas = Atlas::new(&device);
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mre globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let instances = Self::make_instance_buffer(&device, INITIAL_INSTANCES);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mre glyph sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mre draw bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(atlas.view()),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let blit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mre blit bind group"),
            layout: &blit_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&canvas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Ok(Self {
            surface,
            device,
            queue,
            width: config.width,
            height: config.height,
            config,
            canvas,
            canvas_view,
            pipeline,
            blit_pipeline,
            bind_group,
            blit_bind_group,
            globals,
            instances,
            instance_capacity: INITIAL_INSTANCES,
            atlas,
            info,
            ops: Vec::with_capacity(INITIAL_INSTANCES),
            clip: Vec::new(),
            // The freshly allocated texture contains undefined pixels.
            clear_all: Some(Rgba8::TRANSPARENT),
            scale: if scale > 0.0 { scale } else { 1.0 },
            calls: 0,
        })
    }

    /// The adapter this renderer is actually drawing with.
    #[must_use]
    pub fn info(&self) -> &GpuInfo {
        &self.info
    }

    /// Live diagnostics, for the host's status line and for gates.
    #[must_use]
    pub fn stats(&self) -> GpuStats {
        GpuStats {
            calls: self.calls,
            instances: self.ops.len(),
            glyphs: self.atlas.len(),
        }
    }

    fn make_canvas(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mre persistent canvas"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn make_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mre instances"),
            size: (capacity * INSTANCE_BYTES) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn pipelines(
        device: &wgpu::Device,
        canvas_format: wgpu::TextureFormat,
        surface_format: wgpu::TextureFormat,
    ) -> (
        wgpu::RenderPipeline,
        wgpu::RenderPipeline,
        wgpu::BindGroupLayout,
    ) {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mre shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mre draw layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
        let attributes: Vec<wgpu::VertexAttribute> = (0..6)
            .map(|index| wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: (index * 16) as u64,
                shader_location: index as u32,
            })
            .collect();
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mre draw pipeline layout"),
            bind_group_layouts: &[&draw_layout],
            push_constant_ranges: &[],
        });
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mre instanced shapes and glyphs"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_BYTES as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: canvas_format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mre blit layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mre blit pipeline layout"),
            bind_group_layouts: &[&blit_layout],
            push_constant_ranges: &[],
        });
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mre present"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("blit_vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("blit_fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        (pipeline, blit_pipeline, draw_layout)
    }

    fn effective_clip(&self) -> Rect {
        let mut clip = Rect::new(
            0.0,
            0.0,
            self.width as f32 / self.scale,
            self.height as f32 / self.scale,
        );
        for rect in &self.clip {
            let x = clip.x.max(rect.x);
            let y = clip.y.max(rect.y);
            let right = (clip.x + clip.w).min(rect.x + rect.w);
            let bottom = (clip.y + clip.h).min(rect.y + rect.h);
            clip = Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0));
        }
        clip
    }

    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, rect: Rect, color: Rgba8, params: [f32; 4], line: [f32; 4], uv: [f32; 4]) {
        let device_space = params[3] > 0.5;
        let clip = self.effective_clip();
        let factor = if device_space { self.scale } else { 1.0 };
        let clip = [
            clip.x * factor,
            clip.y * factor,
            (clip.x + clip.w) * factor,
            (clip.y + clip.h) * factor,
        ];
        self.ops.push(Instance {
            rect: [rect.x, rect.y, rect.w, rect.h],
            color: rgba(color),
            params,
            line,
            clip,
            uv,
        });
        self.calls += 1;
    }

    fn upload(&mut self) {
        if self.ops.len() > self.instance_capacity {
            let mut capacity = self.instance_capacity;
            while capacity < self.ops.len() {
                capacity *= 2;
            }
            self.instances = Self::make_instance_buffer(&self.device, capacity);
            self.instance_capacity = capacity;
        }
        if !self.ops.is_empty() {
            self.queue
                .write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.ops));
        }
        self.queue.write_buffer(
            &self.globals,
            0,
            bytemuck::cast_slice(&[Globals {
                resolution: [self.width as f32, self.height as f32],
                scale: self.scale,
                _pad: 0.0,
            }]),
        );
    }

    fn recreate_canvas(&mut self) {
        self.canvas = Self::make_canvas(&self.device, self.width, self.height);
        self.canvas_view = self
            .canvas
            .create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = self
            .device
            .create_sampler(&wgpu::SamplerDescriptor::default());
        self.blit_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mre blit bind group"),
            layout: &self.blit_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.canvas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        self.clear_all = Some(Rgba8::TRANSPARENT);
    }
}

/// Upload colour in linear light. `Rgba8` stores sRGB8 by contract; the
/// intermediate canvas is `Rgba8Unorm` (linear) and the swapchain format is
/// sRGB, which encodes once at present. Uploading the sRGB bytes directly made
/// every authored colour noticeably brighter than its hex value.
fn rgba(color: Rgba8) -> [f32; 4] {
    color.to_linear()
}

fn to_wgpu_color(color: Rgba8) -> wgpu::Color {
    let [r, g, b, a] = rgba(color);
    wgpu::Color {
        r: f64::from(r),
        g: f64::from(g),
        b: f64::from(b),
        a: f64::from(a),
    }
}

impl Backend for GpuBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Gpu
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn scale(&self) -> f32 {
        self.scale
    }

    fn set_scale(&mut self, scale: f32) {
        if scale > 0.0 {
            self.scale = scale;
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if (width, height) == (self.width, self.height) {
            return;
        }
        self.width = width;
        self.height = height;
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.recreate_canvas();
    }

    fn calls(&self) -> u64 {
        self.calls
    }

    fn clear(&mut self, color: Rgba8) {
        // A real clear, not a blend: transparent colours must erase what was
        // in the texture before this frame.
        self.clear_all = Some(color);
    }

    fn clear_rect(&mut self, rect: Rect, color: Rgba8) {
        self.push(rect, color, [0.0, 0.0, KIND_FILL, 0.0], [0.0; 4], [0.0; 4]);
    }

    fn push_clip(&mut self, rect: Rect) {
        self.clip.push(rect);
    }

    fn pop_clip(&mut self) {
        self.clip.pop();
    }

    fn fill_rect(&mut self, rect: Rect, paint: Paint) {
        self.push(
            rect,
            paint.color,
            [0.0, 0.0, KIND_FILL, 0.0],
            [0.0; 4],
            [0.0; 4],
        );
    }

    fn fill_rounded_rect(&mut self, rect: Rect, radius: f32, paint: Paint) {
        self.push(
            rect,
            paint.color,
            [radius, 0.0, KIND_FILL, 0.0],
            [0.0; 4],
            [0.0; 4],
        );
    }

    fn stroke_rect(&mut self, rect: Rect, width: f32, paint: Paint) {
        self.stroke_rounded_rect(rect, 0.0, width, paint);
    }

    fn stroke_rounded_rect(&mut self, rect: Rect, radius: f32, width: f32, paint: Paint) {
        self.push(
            rect,
            paint.color,
            [radius, width, KIND_STROKE, 0.0],
            [0.0; 4],
            [0.0; 4],
        );
    }

    fn fill_circle(&mut self, center: Point, radius: f32, paint: Paint) {
        let rect = Rect::new(
            center.x - radius,
            center.y - radius,
            radius * 2.0,
            radius * 2.0,
        );
        self.push(
            rect,
            paint.color,
            [radius, 0.0, KIND_FILL, 0.0],
            [0.0; 4],
            [0.0; 4],
        );
    }

    fn stroke_circle(&mut self, center: Point, radius: f32, width: f32, paint: Paint) {
        let rect = Rect::new(
            center.x - radius,
            center.y - radius,
            radius * 2.0,
            radius * 2.0,
        );
        self.push(
            rect,
            paint.color,
            [radius, width, KIND_STROKE, 0.0],
            [0.0; 4],
            [0.0; 4],
        );
    }

    fn draw_line(&mut self, from: Point, to: Point, width: f32, paint: Paint) {
        let bounds = Rect::new(
            from.x.min(to.x),
            from.y.min(to.y),
            (from.x - to.x).abs(),
            (from.y - to.y).abs(),
        );
        self.push(
            bounds,
            paint.color,
            [0.0, width, KIND_LINE, 0.0],
            [from.x, from.y, to.x, to.y],
            [0.0; 4],
        );
    }

    fn draw_text(
        &mut self,
        engine: &TextEngine,
        shaped: &ShapedText,
        size: f32,
        origin: Point,
        color: Rgba8,
    ) {
        if color.a() == 0 || shaped.glyphs.is_empty() {
            return;
        }
        self.calls += 1;
        let scale = self.scale;
        let size_dev = size * scale;
        let scale_key = (scale * 64.0).round() as u32;
        let atlas_size = Atlas::SIZE as f32;
        for glyph in &shaped.glyphs {
            let pen_x = (origin.x + glyph.x + glyph.x_offset) * scale;
            let pen_y = ((origin.y + glyph.y - glyph.y_offset) * scale).round();
            let fraction = pen_x - pen_x.floor();
            let phase = ((fraction * PHASE_STEPS as f32).round() as i32)
                .rem_euclid(PHASE_STEPS as i32) as u8;
            let phase_offset = f32::from(phase) / PHASE_STEPS as f32;
            let key = GlyphKey {
                font: glyph.font.0,
                glyph: glyph.glyph_id,
                size: (size_dev * 64.0).round().max(0.0) as u32,
                scale: scale_key,
                phase,
            };
            let entry = match self.atlas.get(key) {
                Some(entry) => entry,
                None => {
                    let coverage = glyph_coverage(
                        engine,
                        scale,
                        glyph.font,
                        glyph.glyph_id,
                        size_dev,
                        phase_offset,
                    );
                    let Some(coverage) = coverage else { continue };
                    let Some(entry) = self.atlas.insert(&self.queue, key, &coverage) else {
                        continue;
                    };
                    entry
                }
            };
            // Placed exactly where the reference rasteriser blits it: the
            // phase is baked into the coverage, so only the rounded position
            // and the bitmap's origin are needed.
            let x = (pen_x + entry.left - phase_offset).round() as i32;
            let y = (pen_y + entry.top).round() as i32;
            let atlas = entry.rect;
            let rect = Rect::new(x as f32, y as f32, atlas.w as f32, atlas.h as f32);
            self.push(
                rect,
                color,
                [0.0, 0.0, KIND_GLYPH, 1.0],
                [0.0; 4],
                [
                    atlas.x as f32 / atlas_size,
                    atlas.y as f32 / atlas_size,
                    (atlas.x + atlas.w) as f32 / atlas_size,
                    (atlas.y + atlas.h) as f32 / atlas_size,
                ],
            );
        }
    }

    fn copy_region(&mut self, _rect: Rect, _dx: i32, _dy: i32) -> bool {
        // A swapchain cannot be read back cheaply and wgpu forbids overlapping
        // texture-to-texture copies. Reporting failure makes the shared paint
        // pass fall back to repainting the union of old and new positions —
        // the same decision the CPU path makes for a fractional offset.
        false
    }

    fn finish(&mut self) {
        self.upload();
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(error) => {
                // A lost or out-of-date surface recovers on the next resize;
                // never queue work against it.
                eprintln!("GPU surface unavailable: {error}");
                self.ops.clear();
                return;
            }
        };
        let swap_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("mre frame"),
            });

        {
            let load = match self.clear_all.take() {
                Some(color) => wgpu::LoadOp::Clear(to_wgpu_color(color)),
                None => wgpu::LoadOp::Load,
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mre paint"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.canvas_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if !self.ops.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.instances.slice(..));
                pass.draw(0..6, 0..self.ops.len() as u32);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mre present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &swap_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &self.blit_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        self.calls += 1;
        self.ops.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_layout_is_24_floats() {
        assert_eq!(core::mem::size_of::<Instance>(), INSTANCE_BYTES);
        assert_eq!(core::mem::align_of::<Instance>(), 4);
    }

    #[test]
    fn srgb_colours_are_linearised_at_upload() {
        // #808080 is ~0.216 in linear light, not 0.502. The sRGB swapchain
        // encodes it back to 128 on present; uploading the raw byte doubled
        // the gamma and made dark palettes look washed out.
        let mid = rgba(Rgba8::from_rgba(128, 128, 128, 255));
        assert!((mid[0] - 0.21586).abs() < 0.002, "mid grey: {:?}", mid);
        assert!((mid[1] - 0.21586).abs() < 0.002, "mid grey: {:?}", mid);
        assert!((mid[2] - 0.21586).abs() < 0.002, "mid grey: {:?}", mid);
        assert!((mid[3] - 1.0).abs() < 1e-6, "alpha is not gamma-encoded");

        let white = rgba(Rgba8::WHITE);
        assert!((white[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn kinds_are_distinct() {
        assert_ne!(KIND_FILL, KIND_STROKE);
        assert_ne!(KIND_STROKE, KIND_LINE);
        assert_ne!(KIND_LINE, KIND_GLYPH);
    }

    #[test]
    fn pipelines_create_on_an_available_adapter() {
        // No adapter on a headless machine is not a failure; the point of this
        // test is that when an adapter exists, the real driver accepts both
        // pipelines built from our WGSL. Skipped runs are printed, not hidden.
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: true,
        }))
        .ok();
        let Some(adapter) = adapter else {
            eprintln!("no GPU adapter available; pipeline creation skipped");
            return;
        };
        let (device, _) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("mre pipeline test device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .expect("device from the selected adapter");
        let (_draw, _blit, _layout) = GpuBackend::pipelines(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        );
        eprintln!(
            "pipelines created on: {}",
            GpuInfo::from(&adapter.get_info())
        );
    }

    #[test]
    fn shader_parses_and_validates() {
        let source = include_str!("shader.wgsl");
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|error| {
            panic!("shader failed to parse: {}", error.emit_to_string(source))
        });
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator.validate(&module).expect("shader must validate");
        assert!(module.entry_points.iter().any(|entry| entry.name == "vs"));
        assert!(module.entry_points.iter().any(|entry| entry.name == "fs"));
        assert!(
            module
                .entry_points
                .iter()
                .any(|entry| entry.name == "blit_vs")
        );
        assert!(
            module
                .entry_points
                .iter()
                .any(|entry| entry.name == "blit_fs")
        );
    }
}
