//! A library for post process antialiasing for the wgpu graphics API, based on the [SMAA
//! reference implementation](https://github.com/iryoku/smaa).
//!
//! # Example
//!
//! ```
//! # extern crate gfx_smaa;
//! # extern crate piston_window;
//! # use piston_window::*;
//! # use gfx_smaa::SmaaTarget;
//! # fn main(){
//! // create window
//! let mut window: PistonWindow = WindowSettings::new("SMAA", (640, 480)).build().unwrap();
//!
//! // create target
//! let mut target = SmaaTarget::<_>::new(&mut window.factory,
//!                                       window.output_color.clone(),
//!                                       640, 480).unwrap();
//!
//! // main loop
//! while let Some(e) = window.next() {
//!     window.draw_3d(&e, |window| {
//!         // clear depth and color buffers.
//!         window.encoder.clear_depth(&target.output_depth(), 1.0);
//!         window.encoder.clear(&target.output_color(), [0.0, 0.0, 0.0, 1.0]);
//!
//!         // Render the scene.
//!         // [...]
//!
//!         // Perform actual antialiasing operation and write the result to the screen.
//!         target.resolve(&mut window.encoder);
//!      });
//! #     break; // don't want test to run forever.
//! }
//! # }

#![deny(missing_docs)]

use failure::Error;

mod shader;
use shader::{ShaderQuality, ShaderSource, ShaderStage};

#[path = "../third_party/smaa/Textures/AreaTex.rs"]
mod area_tex;
use area_tex::*;

#[path = "../third_party/smaa/Textures/SearchTex.rs"]
mod search_tex;
use search_tex::*;

use wgpu::util::DeviceExt;

/// Which tone mapping function to use. Currently, only one such function is supported, but more may
/// be added in the future.
pub enum ToneMappingFunction {
    /// Use the equation from https://knarkowicz.wordpress.com/2016/01/06/aces-filmic-tone-mapping-curve
    AcesNormalized,
}

/// A `SmaaTarget` wraps a color and depth buffer, which it can resolve into an antialiased image
/// using the [Subpixel Morphological Antialiasing (SMAA)](http://www.iryoku.com/smaa) algorithm.
pub struct SmaaTarget {
    /// Render target for actual frame data.
    color_target: wgpu::Texture,

    edges_target: wgpu::TextureView,
    blend_target: wgpu::TextureView,

    edge_detect: wgpu::RenderPipeline,
    blend_weight: wgpu::RenderPipeline,
    neighborhood_blending: wgpu::RenderPipeline,

    edge_detect_bind_group: wgpu::BindGroup,
    blend_weight_bind_group: wgpu::BindGroup,
    neighborhood_blending_bind_group: wgpu::BindGroup,
}

impl SmaaTarget {
    /// Create a new `SmaaTarget`.
    fn new_internal(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        tone_mapping: Option<ToneMappingFunction>,
    ) -> Result<Self, Error> {
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

        let color_target = device.create_texture(&wgpu::TextureDescriptor {
            format: wgpu::TextureFormat::Rgba16Float,
            ..texture_desc
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
                    resource: wgpu::BindingResource::TextureView(
                        &color_target.create_view(&Default::default()),
                    ),
                },
            ],
        });
        let edge_detect_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("smaa.pipeline_layout.edge_detect"),
            bind_group_layouts: &[&edge_detect_bind_group_layout],
            push_constant_ranges: &[],
        });
        let edge_detect_shader_vert = wgpu::ProgrammableStageDescriptor {
            module: &source.get_shader(
                device,
                ShaderStage::EdgeDetectionVS,
                "smaa.shader.edge_detect.vert",
            )?,
            entry_point: "main",
        };
        let edge_detect_shader_frag = wgpu::ProgrammableStageDescriptor {
            module: &source.get_shader(
                device,
                ShaderStage::LumaEdgeDetectionPS,
                "smaa.shader.edge_detect.frag",
            )?,
            entry_point: "main",
        };
        let edge_detect = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("smaa.pipeline.edge_detect"),
            layout: Some(&edge_detect_layout),
            vertex_stage: edge_detect_shader_vert,
            fragment_stage: Some(edge_detect_shader_frag),
            rasterization_state: Some(Default::default()),
            primitive_topology: wgpu::PrimitiveTopology::TriangleList,
            color_states: &[wgpu::ColorStateDescriptor {
                format: wgpu::TextureFormat::Rg8Unorm,
                color_blend: wgpu::BlendDescriptor::REPLACE,
                alpha_blend: wgpu::BlendDescriptor::REPLACE,
                write_mask: wgpu::ColorWrite::ALL,
            }],
            depth_stencil_state: None,
            vertex_state: wgpu::VertexStateDescriptor {
                index_format: None,
                vertex_buffers: &[],
            },
            sample_count: 1,
            sample_mask: !0,
            alpha_to_coverage_enabled: false,
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
        let blend_weight_shader_vert = wgpu::ProgrammableStageDescriptor {
            module: &source.get_shader(
                device,
                ShaderStage::BlendingWeightVS,
                "smaa.shader.blending_weight.vert",
            )?,
            entry_point: "main",
        };
        let blend_weight_shader_frag = wgpu::ProgrammableStageDescriptor {
            module: &source.get_shader(
                device,
                ShaderStage::BlendingWeightPS,
                "smaa.shader.blending_weight.frag",
            )?,
            entry_point: "main",
        };
        let blend_weight = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("smaa.pipeline.blend_weight"),
            layout: Some(&blend_weight_layout),
            vertex_stage: blend_weight_shader_vert,
            fragment_stage: Some(blend_weight_shader_frag),
            rasterization_state: Some(Default::default()),
            primitive_topology: wgpu::PrimitiveTopology::TriangleList,
            color_states: &[wgpu::ColorStateDescriptor {
                format: wgpu::TextureFormat::Rgba8Unorm,
                color_blend: wgpu::BlendDescriptor::REPLACE,
                alpha_blend: wgpu::BlendDescriptor::REPLACE,
                write_mask: wgpu::ColorWrite::ALL,
            }],
            depth_stencil_state: None,
            vertex_state: wgpu::VertexStateDescriptor {
                index_format: None,
                vertex_buffers: &[],
            },
            sample_count: 1,
            sample_mask: !0,
            alpha_to_coverage_enabled: false,
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
                        resource: wgpu::BindingResource::TextureView(
                            &color_target.create_view(&Default::default()),
                        ),
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
        let neighborhood_blending_vert = wgpu::ProgrammableStageDescriptor {
            module: &source.get_shader(
                device,
                ShaderStage::NeighborhoodBlendingVS,
                "smaa.shader.neighborhood_blending.vert",
            )?,
            entry_point: "main",
        };
        let neighborhood_blending_frag = wgpu::ProgrammableStageDescriptor {
            module: &source.get_shader(
                device,
                match tone_mapping {
                    Some(ToneMappingFunction::AcesNormalized) => {
                        ShaderStage::NeighborhoodBlendingAcesTonemapPS
                    }
                    None => ShaderStage::NeighborhoodBlendingPS,
                },
                "smaa.shader.neighborhood_blending.frag",
            )?,
            entry_point: "main",
        };
        let neighborhood_blending =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("smaa.pipeline.neighborhood_blending"),
                layout: Some(&neighborhood_blending_layout),
                vertex_stage: neighborhood_blending_vert,
                fragment_stage: Some(neighborhood_blending_frag),
                rasterization_state: Some(Default::default()),
                primitive_topology: wgpu::PrimitiveTopology::TriangleList,
                color_states: &[wgpu::ColorStateDescriptor {
                    format,
                    color_blend: wgpu::BlendDescriptor::REPLACE,
                    alpha_blend: wgpu::BlendDescriptor::REPLACE,
                    write_mask: wgpu::ColorWrite::ALL,
                }],
                depth_stencil_state: None,
                vertex_state: wgpu::VertexStateDescriptor {
                    index_format: None,
                    vertex_buffers: &[],
                },
                sample_count: 1,
                sample_mask: !0,
                alpha_to_coverage_enabled: false,
            });

        Ok(Self {
            color_target,

            edges_target,
            blend_target,

            edge_detect,
            blend_weight,
            neighborhood_blending,

            edge_detect_bind_group,
            blend_weight_bind_group,
            neighborhood_blending_bind_group,
        })
    }

    /// Create a new `SmaaTarget`.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Result<Self, Error> {
        Self::new_internal(device, queue, width, height, format, None)
    }

    /// Create a new `SmaaTarget` that also applies tone mapping to the final image.
    pub fn with_tone_mapping(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        tone_mapping: ToneMappingFunction,
    ) -> Result<Self, Error> {
        Self::new_internal(device, queue, width, height, format, Some(tone_mapping))
    }

    /// Get the color buffer associated with this target.
    pub fn color_target(&self) -> &wgpu::Texture {
        &self.color_target
    }

    /// Do a multisample resolve, outputing to the frame buffer specified at creation time.
    pub fn resolve(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        output_view: &wgpu::TextureView,
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("smaa.command_encoder"),
        });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                    attachment: &self.edges_target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: true,
                    },
                }],
                depth_stencil_attachment: None,
                label: Some("smaa.render_pass.edge_detect"),
            });
            rpass.set_pipeline(&self.edge_detect);
            rpass.set_bind_group(0, &self.edge_detect_bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                    attachment: &self.blend_target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: true,
                    },
                }],
                depth_stencil_attachment: None,
                label: Some("smaa.render_pass.blend_weight"),
            });
            rpass.set_pipeline(&self.blend_weight);
            rpass.set_bind_group(0, &self.blend_weight_bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                    attachment: output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: true,
                    },
                }],
                depth_stencil_attachment: None,
                label: Some("smaa.render_pass.neighborhood_blending"),
            });
            rpass.set_pipeline(&self.neighborhood_blending);
            rpass.set_bind_group(0, &self.neighborhood_blending_bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }
}
