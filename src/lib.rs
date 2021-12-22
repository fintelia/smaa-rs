//! Post-process antialiasing for wgpu-rs, using the [SMAA reference implementation](https://github.com/iryoku/smaa).
//!
//! # Example
//!
//! ```
//! # use smaa::{SmaaMode, SmaaTarget};
//! # use winit::event::Event;
//! # use winit::event_loop::EventLoop;
//! # use winit::window::Window;
//! # fn main() { futures::executor::block_on(run()); }
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! // Initialize wgpu
//! let event_loop = EventLoop::new();
//! let window = winit::window::Window::new(&event_loop).unwrap();
//! let instance = wgpu::Instance::new(wgpu::Backends::PRIMARY);
//! let surface = unsafe { instance.create_surface(&window) };
//! let adapter = instance.request_adapter(&Default::default()).await.unwrap();
//! let (device, queue) = adapter.request_device(&Default::default(), None).await?;
//! let swapchain_format = surface.get_preferred_format(&adapter)
//!     .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb);
//! let mut config = wgpu::SurfaceConfiguration {
//!     usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
//!     format: swapchain_format,
//!     width: window.inner_size().width,
//!     height: window.inner_size().height,
//!     present_mode: wgpu::PresentMode::Mailbox,
//! };
//! surface.configure(&device, &config);
//!
//! // Create SMAA target
//! let mut smaa_target = SmaaTarget::new(
//!     &device,
//!     &queue,
//!     window.inner_size().width,
//!     window.inner_size().height,
//!     swapchain_format,
//!     SmaaMode::Smaa1X,
//! );
//!
//! // Main loop
//! event_loop.run(move |event, _, control_flow| {
//! #    *control_flow = winit::event_loop::ControlFlow::Exit;
//!     match event {
//!         Event::RedrawRequested(_) => {
//!             let output_frame = surface.get_current_texture().unwrap();
//!             let output_view = output_frame.texture.create_view(&Default::default());
//!             {
//!                 let frame = smaa_target.start_frame(&device, &queue, &output_view);
//!
//!                 // Render the scene into `*frame`.
//!                 // [...]
//!
//!             }
//!             output_frame.present();
//!         }
//!         _ => {}
//!     }
//! });
//! # }

#![deny(missing_docs)]

mod shader;
use shader::{ShaderQuality, ShaderSource, ShaderStage};

#[path = "../third_party/smaa/Textures/AreaTex.rs"]
mod area_tex;
use area_tex::*;

#[path = "../third_party/smaa/Textures/SearchTex.rs"]
mod search_tex;
use search_tex::*;

use wgpu::util::DeviceExt;

/// Anti-aliasing mode. Higher values produce nicer results but run slower.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SmaaMode {
    /// Do not perform antialiasing.
    Disabled,
    /// Use SMAA 1x.
    Smaa1X,
}

struct BindGroupLayouts {
    edge_detect_bind_group_layout: wgpu::BindGroupLayout,
    blend_weight_bind_group_layout: wgpu::BindGroupLayout,
    neighborhood_blending_bind_group_layout: wgpu::BindGroupLayout,
}
struct Pipelines {
    edge_detect: wgpu::RenderPipeline,
    blend_weight: wgpu::RenderPipeline,
    neighborhood_blending: wgpu::RenderPipeline,
}
struct Resources {
    area_texture: wgpu::Texture,
    search_texture: wgpu::Texture,
    linear_sampler: wgpu::Sampler,
}
struct Targets {
    rt_uniforms: wgpu::Buffer,
    color_target: wgpu::TextureView,
    edges_target: wgpu::TextureView,
    blend_target: wgpu::TextureView,
}
struct BindGroups {
    edge_detect_bind_group: wgpu::BindGroup,
    blend_weight_bind_group: wgpu::BindGroup,
    neighborhood_blending_bind_group: wgpu::BindGroup,
}

impl BindGroupLayouts {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            edge_detect_bind_group_layout: device.create_bind_group_layout(
                &wgpu::BindGroupLayoutDescriptor {
                    label: Some("smaa.bind_group_layout.edge_detect"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                    ],
                },
            ),
            blend_weight_bind_group_layout: device.create_bind_group_layout(
                &wgpu::BindGroupLayoutDescriptor {
                    label: Some("smaa.bind_group_layout.blend_weight"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 3,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 4,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                    ],
                },
            ),
            neighborhood_blending_bind_group_layout: device.create_bind_group_layout(
                &wgpu::BindGroupLayoutDescriptor {
                    label: Some("smaa.bind_group_layout.neighborhood_blending"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 3,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                    ],
                },
            ),
        }
    }
}

impl Pipelines {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        layouts: &BindGroupLayouts,
    ) -> Self {
        let source = ShaderSource {
            quality: ShaderQuality::High,
        };

        let edge_detect_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("smaa.pipeline_layout.edge_detect"),
            bind_group_layouts: &[&layouts.edge_detect_bind_group_layout],
            push_constant_ranges: &[],
        });
        let edge_detect_shader_vert = wgpu::VertexState {
            module: &source.get_shader(
                device,
                ShaderStage::EdgeDetectionVS,
                "smaa.shader.edge_detect.vert",
            ),
            entry_point: "main",
            buffers: &[],
        };
        let edge_detect_shader_frag = wgpu::FragmentState {
            module: &source.get_shader(
                device,
                ShaderStage::LumaEdgeDetectionPS,
                "smaa.shader.edge_detect.frag",
            ),
            entry_point: "main",
            targets: &[wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rg8Unorm,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent::REPLACE,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            }],
        };
        let edge_detect = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("smaa.pipeline.edge_detect"),
            layout: Some(&edge_detect_layout),
            vertex: edge_detect_shader_vert,
            fragment: Some(edge_detect_shader_frag),
            primitive: Default::default(),
            multisample: Default::default(),
            depth_stencil: None,
            multiview: None,
        });

        let blend_weight_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("smaa.pipeline_layout.blend_weight"),
            bind_group_layouts: &[&layouts.blend_weight_bind_group_layout],
            push_constant_ranges: &[],
        });
        let blend_weight_shader_vert = wgpu::VertexState {
            module: &source.get_shader(
                device,
                ShaderStage::BlendingWeightVS,
                "smaa.shader.blending_weight.vert",
            ),
            entry_point: "main",
            buffers: &[],
        };
        let blend_weight_shader_frag = wgpu::FragmentState {
            module: &source.get_shader(
                device,
                ShaderStage::BlendingWeightPS,
                "smaa.shader.blending_weight.frag",
            ),
            entry_point: "main",
            targets: &[wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent::REPLACE,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            }],
        };
        let blend_weight = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("smaa.pipeline.blend_weight"),
            layout: Some(&blend_weight_layout),
            vertex: blend_weight_shader_vert,
            fragment: Some(blend_weight_shader_frag),
            primitive: Default::default(),
            multisample: Default::default(),
            depth_stencil: None,
            multiview: None,
        });

        let neighborhood_blending_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("smaa.pipeline_layout.neighborhood_blending"),
                bind_group_layouts: &[&layouts.neighborhood_blending_bind_group_layout],
                push_constant_ranges: &[],
            });
        let neighborhood_blending_vert = wgpu::VertexState {
            module: &source.get_shader(
                device,
                ShaderStage::NeighborhoodBlendingVS,
                "smaa.shader.neighborhood_blending.vert",
            ),
            entry_point: "main",
            buffers: &[],
        };
        let neighborhood_blending_frag = wgpu::FragmentState {
            module: &source.get_shader(
                device,
                ShaderStage::NeighborhoodBlendingPS,
                "smaa.shader.neighborhood_blending.frag",
            ),
            entry_point: "main",
            targets: &[wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent::REPLACE,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            }],
        };
        let neighborhood_blending =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("smaa.pipeline.neighborhood_blending"),
                layout: Some(&neighborhood_blending_layout),
                vertex: neighborhood_blending_vert,
                fragment: Some(neighborhood_blending_frag),
                primitive: Default::default(),
                multisample: Default::default(),
                depth_stencil: None,
                multiview: None,
            });

        Self {
            edge_detect,
            blend_weight,
            neighborhood_blending,
        }
    }
}
impl Targets {
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Self {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture_desc = wgpu::TextureDescriptor {
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            label: None,
        };

        let mut uniform_data = Vec::new();
        for f in &[
            1.0 / width as f32,
            1.0 / height as f32,
            width as f32,
            height as f32,
        ] {
            uniform_data.extend_from_slice(&f.to_ne_bytes());
        }
        let rt_uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("smaa.uniforms"),
            usage: wgpu::BufferUsages::UNIFORM,
            contents: &uniform_data,
        });

        Self {
            rt_uniforms,
            color_target: device
                .create_texture(&wgpu::TextureDescriptor {
                    format,
                    ..texture_desc
                })
                .create_view(&wgpu::TextureViewDescriptor {
                    label: Some("smaa.color_target.view"),
                    ..Default::default()
                }),
            edges_target: device
                .create_texture(&wgpu::TextureDescriptor {
                    format: wgpu::TextureFormat::Rg8Unorm,
                    label: Some("smaa.texture.edge_target"),
                    ..texture_desc
                })
                .create_view(&wgpu::TextureViewDescriptor {
                    label: Some("smaa.texture_view.edge_target"),
                    ..Default::default()
                }),

            blend_target: device
                .create_texture(&wgpu::TextureDescriptor {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    label: Some("smaa.texture.blend_target"),
                    ..texture_desc
                })
                .create_view(&wgpu::TextureViewDescriptor {
                    label: Some("smaa.texture_view.blend_target"),
                    ..Default::default()
                }),
        }
    }
}
impl Resources {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let area_texture = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: Some("smaa.texture.area"),
                size: wgpu::Extent3d {
                    width: AREATEX_WIDTH,
                    height: AREATEX_HEIGHT,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rg8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            },
            &AREATEX_BYTES,
        );

        let search_texture = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: Some("smaa.texture.search"),
                size: wgpu::Extent3d {
                    width: SEARCHTEX_WIDTH,
                    height: SEARCHTEX_HEIGHT,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            },
            &SEARCHTEX_BYTES,
        );

        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("smaa.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        Self {
            area_texture,
            search_texture,
            linear_sampler,
        }
    }
}

impl BindGroups {
    fn new(
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        resources: &Resources,
        targets: &Targets,
    ) -> Self {
        Self {
            edge_detect_bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("smaa.bind_group.edge_detect"),
                layout: &layouts.edge_detect_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&resources.linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &targets.rt_uniforms,
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&targets.color_target),
                    },
                ],
            }),

            blend_weight_bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("smaa.bind_group.blend_weight"),
                layout: &layouts.blend_weight_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&resources.linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &targets.rt_uniforms,
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&targets.edges_target),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(
                            &resources.area_texture.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(
                            &resources.search_texture.create_view(&Default::default()),
                        ),
                    },
                ],
            }),
            neighborhood_blending_bind_group: device.create_bind_group(
                &wgpu::BindGroupDescriptor {
                    label: Some("smaa.bind_group.neighborhood_blending"),
                    layout: &layouts.neighborhood_blending_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Sampler(&resources.linear_sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &targets.rt_uniforms,
                                offset: 0,
                                size: None,
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&targets.color_target),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&targets.blend_target),
                        },
                    ],
                },
            ),
        }
    }
}

struct SmaaTargetInner {
    pipelines: Pipelines,
    layouts: BindGroupLayouts,
    resources: Resources,
    targets: Targets,
    bind_groups: BindGroups,

    format: wgpu::TextureFormat,
}

/// Wraps a color buffer, which it can resolve into an antialiased image using the
/// [Subpixel Morphological Antialiasing (SMAA)](http://www.iryoku.com/smaa) algorithm.
pub struct SmaaTarget {
    inner: Option<SmaaTargetInner>,
}

impl SmaaTarget {
    /// Create a new `SmaaTarget`.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        mode: SmaaMode,
    ) -> Self {
        if let SmaaMode::Disabled = mode {
            return SmaaTarget { inner: None };
        }

        let layouts = BindGroupLayouts::new(device);
        let pipelines = Pipelines::new(device, format, &layouts);
        let resources = Resources::new(device, queue);
        let targets = Targets::new(device, width, height, format);
        let bind_groups = BindGroups::new(device, &layouts, &resources, &targets);

        SmaaTarget {
            inner: Some(SmaaTargetInner {
                layouts,
                pipelines,
                resources,
                targets,
                bind_groups,
                format,
            }),
        }
    }

    /// Resize the render target.
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if let Some(ref mut inner) = self.inner {
            inner.targets = Targets::new(device, width, height, inner.format);
            inner.bind_groups =
                BindGroups::new(device, &inner.layouts, &inner.resources, &inner.targets);
        }
    }

    /// Start rendering a frame. Dropping the returned frame object will resolve the scene into the provided output_view.
    pub fn start_frame<'a>(
        &'a mut self,
        device: &'a wgpu::Device,
        queue: &'a wgpu::Queue,
        output_view: &'a wgpu::TextureView,
    ) -> SmaaFrame<'a> {
        SmaaFrame {
            target: self,
            device,
            queue,
            output_view,
        }
    }
}

/// Frame that the scene should be rendered into; can be created by a SmaaTarget.
pub struct SmaaFrame<'a> {
    target: &'a mut SmaaTarget,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    output_view: &'a wgpu::TextureView,
}
impl<'a> std::ops::Deref for SmaaFrame<'a> {
    type Target = wgpu::TextureView;
    fn deref(&self) -> &Self::Target {
        match self.target.inner {
            None => self.output_view,
            Some(ref inner) => &inner.targets.color_target,
        }
    }
}
impl<'a> Drop for SmaaFrame<'a> {
    fn drop(&mut self) {
        if let Some(ref mut inner) = self.target.inner {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("smaa.command_encoder"),
                });
            {
                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[wgpu::RenderPassColorAttachment {
                        view: &inner.targets.edges_target,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: true,
                        },
                    }],
                    depth_stencil_attachment: None,
                    label: Some("smaa.render_pass.edge_detect"),
                });
                rpass.set_pipeline(&inner.pipelines.edge_detect);
                rpass.set_bind_group(0, &inner.bind_groups.edge_detect_bind_group, &[]);
                rpass.draw(0..3, 0..1);
            }
            {
                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[wgpu::RenderPassColorAttachment {
                        view: &inner.targets.blend_target,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: true,
                        },
                    }],
                    depth_stencil_attachment: None,
                    label: Some("smaa.render_pass.blend_weight"),
                });
                rpass.set_pipeline(&inner.pipelines.blend_weight);
                rpass.set_bind_group(0, &inner.bind_groups.blend_weight_bind_group, &[]);
                rpass.draw(0..3, 0..1);
            }
            {
                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[wgpu::RenderPassColorAttachment {
                        view: self.output_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: true,
                        },
                    }],
                    depth_stencil_attachment: None,
                    label: Some("smaa.render_pass.neighborhood_blending"),
                });
                rpass.set_pipeline(&inner.pipelines.neighborhood_blending);
                rpass.set_bind_group(0, &inner.bind_groups.neighborhood_blending_bind_group, &[]);
                rpass.draw(0..3, 0..1);
            }
            self.queue.submit(Some(encoder.finish()));
        }
    }
}
