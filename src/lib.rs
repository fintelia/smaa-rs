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
//! let instance = wgpu::Instance::new(wgpu::BackendBit::all());
//! let surface = unsafe { instance.create_surface(&window) };
//! let adapter = instance.request_adapter(&Default::default()).await.unwrap();
//! let (device, queue) = adapter.request_device(&Default::default(), None).await?;
//! let swapchain_format = adapter.get_swap_chain_preferred_format(&surface);
//! let mut swap_chain = device.create_swap_chain(&surface, &wgpu::SwapChainDescriptor {
//!     usage: wgpu::TextureUsage::RENDER_ATTACHMENT,
//!     format: swapchain_format,
//!     width: window.inner_size().width,
//!     height: window.inner_size().height,
//!     present_mode: wgpu::PresentMode::Mailbox,
//! });
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
//!             let output_frame = swap_chain.get_current_frame().unwrap().output;
//!             let frame = smaa_target.start_frame(&device, &queue, &output_frame.view);
//!
//!             // Render the scene into `*frame`.
//!             // [...]
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

struct SmaaTargetInner {
    /// Render target for actual frame data.
    color_target_view: wgpu::TextureView,

    edges_target: wgpu::TextureView,
    blend_target: wgpu::TextureView,

    edge_detect: wgpu::RenderPipeline,
    blend_weight: wgpu::RenderPipeline,
    neighborhood_blending: wgpu::RenderPipeline,

    edge_detect_bind_group: wgpu::BindGroup,
    blend_weight_bind_group: wgpu::BindGroup,
    neighborhood_blending_bind_group: wgpu::BindGroup,
}

/// Wraps a color buffer, which it can resolve into an antialiased image using the
/// [Subpixel Morphological Antialiasing (SMAA)](http://www.iryoku.com/smaa) algorithm.
pub struct SmaaTarget {
    inner: Option<SmaaTargetInner>,
}

impl SmaaTarget {
    /// Create a new `SmaaTarget`.
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

        let size = wgpu::Extent3d {
            width,
            height,
            depth: 1,
        };
        let texture_desc = wgpu::TextureDescriptor {
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsage::RENDER_ATTACHMENT | wgpu::TextureUsage::SAMPLED,
            label: None,
        };

        let color_target_view = device
            .create_texture(&wgpu::TextureDescriptor {
                format,
                ..texture_desc
            })
            .create_view(&wgpu::TextureViewDescriptor {
                label: Some("smaa.color_target.view"),
                ..Default::default()
            });

        let edges_target = device
            .create_texture(&wgpu::TextureDescriptor {
                format: wgpu::TextureFormat::Rg8Unorm,
                label: Some("smaa.texture.edge_target"),
                ..texture_desc
            })
            .create_view(&wgpu::TextureViewDescriptor {
                label: Some("smaa.texture_view.edge_target"),
                ..Default::default()
            });

        let blend_target = device
            .create_texture(&wgpu::TextureDescriptor {
                format: wgpu::TextureFormat::Rgba8Unorm,
                label: Some("smaa.texture.blend_target"),
                ..texture_desc
            })
            .create_view(&wgpu::TextureViewDescriptor {
                label: Some("smaa.texture_view.blend_target"),
                ..Default::default()
            });

        let area_texture = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: Some("smaa.texture.area"),
                size: wgpu::Extent3d {
                    width: AREATEX_WIDTH,
                    height: AREATEX_HEIGHT,
                    depth: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rg8Unorm,
                usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::COPY_DST,
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
                    depth: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::COPY_DST,
            },
            &SEARCHTEX_BYTES,
        );

        let source = ShaderSource {
            width,
            height,
            quality: ShaderQuality::High,
        };

        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("smaa.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let edge_detect_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("smaa.bind_group_layout.edge_detect"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::Sampler {
                            filtering: true,
                            comparison: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let edge_detect_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("smaa.bind_group.edge_detect"),
            layout: &edge_detect_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&linear_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&color_target_view),
                },
            ],
        });
        let edge_detect_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("smaa.pipeline_layout.edge_detect"),
            bind_group_layouts: &[&edge_detect_bind_group_layout],
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
                color_blend: wgpu::BlendState::REPLACE,
                alpha_blend: wgpu::BlendState::REPLACE,
                write_mask: wgpu::ColorWrite::ALL,
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
        });

        let blend_weight_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("smaa.bind_group_layout.blend_weight"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::Sampler {
                            filtering: true,
                            comparison: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Uint,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Uint,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Uint,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let blend_weight_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("smaa.bind_group.blend_weight"),
            layout: &blend_weight_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&linear_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&edges_target),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &area_texture.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(
                        &search_texture.create_view(&Default::default()),
                    ),
                },
            ],
        });
        let blend_weight_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("smaa.pipeline_layout.blend_weight"),
            bind_group_layouts: &[&blend_weight_bind_group_layout],
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
                color_blend: wgpu::BlendState::REPLACE,
                alpha_blend: wgpu::BlendState::REPLACE,
                write_mask: wgpu::ColorWrite::ALL,
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
        });

        let neighborhood_blending_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("smaa.bind_group_layout.neighborhood_blending"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::Sampler {
                            filtering: true,
                            comparison: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Uint,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let neighborhood_blending_bind_group =
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("smaa.bind_group.neighborhood_blending"),
                layout: &neighborhood_blending_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&color_target_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&blend_target),
                    },
                ],
            });
        let neighborhood_blending_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("smaa.pipeline_layout.neighborhood_blending"),
                bind_group_layouts: &[&neighborhood_blending_bind_group_layout],
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
                color_blend: wgpu::BlendState::REPLACE,
                alpha_blend: wgpu::BlendState::REPLACE,
                write_mask: wgpu::ColorWrite::ALL,
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
            });

        SmaaTarget {
            inner: Some(SmaaTargetInner {
                color_target_view,

                edges_target,
                blend_target,

                edge_detect,
                blend_weight,
                neighborhood_blending,

                edge_detect_bind_group,
                blend_weight_bind_group,
                neighborhood_blending_bind_group,
            }),
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
            Some(ref inner) => &inner.color_target_view,
        }
    }
}
impl<'a> Drop for SmaaFrame<'a> {
    fn drop(&mut self) {
        if let Some(ref mut target) = self.target.inner {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("smaa.command_encoder"),
                });
            {
                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                        attachment: &target.edges_target,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: true,
                        },
                    }],
                    depth_stencil_attachment: None,
                    label: Some("smaa.render_pass.edge_detect"),
                });
                rpass.set_pipeline(&target.edge_detect);
                rpass.set_bind_group(0, &target.edge_detect_bind_group, &[]);
                rpass.draw(0..3, 0..1);
            }
            {
                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                        attachment: &target.blend_target,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: true,
                        },
                    }],
                    depth_stencil_attachment: None,
                    label: Some("smaa.render_pass.blend_weight"),
                });
                rpass.set_pipeline(&target.blend_weight);
                rpass.set_bind_group(0, &target.blend_weight_bind_group, &[]);
                rpass.draw(0..3, 0..1);
            }
            {
                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                        attachment: self.output_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: true,
                        },
                    }],
                    depth_stencil_attachment: None,
                    label: Some("smaa.render_pass.neighborhood_blending"),
                });
                rpass.set_pipeline(&target.neighborhood_blending);
                rpass.set_bind_group(0, &target.neighborhood_blending_bind_group, &[]);
                rpass.draw(0..3, 0..1);
            }
            self.queue.submit(Some(encoder.finish()));
        }
    }
}
