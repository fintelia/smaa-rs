//! A library for post process antialiasing for the gfx-rs graphics API, based on the [SMAA
//! reference implementation](https://github.com/iryoku/smaa).
//!
//! # Example
//!
//! ```
//! # extern crate piston_window;
//! # extern crate gfx_smaa;
//! # use piston_window::*;
//! # use gfx_smaa::SmaaTarget;
//! # fn main(){
//! // create window
//! let mut window: PistonWindow = WindowSettings::new("SMAA", (640, 480)).build().unwrap();
//!
//! // create target
//! let mut target = SmaaTarget::new(&mut window.factory,
//!                                  window.output_color.clone(),
//!                                  640, 480).unwrap();
//!
//! // main loop
//! while let Some(e) = window.next() {
//!     window.draw_3d(&e, |window| {
//!         // clear depth and color buffers.
//!         window.encoder.clear_depth(&target.output_stencil(), 1.0);
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

#![feature(nll)]
#![deny(missing_docs)]

#[macro_use]
extern crate gfx;

extern crate failure;
extern crate gfx_core;

use gfx::{Resources, PipelineState, Factory, RenderTarget, TextureSampler};
use gfx::format::{R8, R8_G8, Unorm, Rgba8, Srgba8, DepthStencil};
use gfx::handle::{RenderTargetView, DepthStencilView};
use gfx::memory;
use gfx::traits::FactoryExt;
use gfx::texture::{AaMode, Kind, Mipmap, FilterMethod, WrapMode, SamplerInfo};
use gfx_core::command;
use failure::Error;

mod shader;
use shader::{ShaderSource, ShaderStage, ShaderQuality};

#[path = "../third_party/smaa/Textures/AreaTex.rs"]
mod area_tex;
use area_tex::*;

#[path = "../third_party/smaa/Textures/SearchTex.rs"]
mod search_tex;
use search_tex::*;

/// Module containing gfx pipelines. Needed to prevent them from being visible from outside of this
/// crate.
mod pipelines {
    use super::*;
    gfx_pipeline!(edge_detection_pipe {
        color_tex: TextureSampler<[f32; 4]> = "colorTex",
        output: RenderTarget<(R8_G8, Unorm)> = "OutColor",
    });
    gfx_pipeline!(blending_weight_pipe {
        edges_tex: TextureSampler<[f32; 2]> = "edgesTex",
        area_tex: TextureSampler<[f32; 2]> = "areaTex",
        search_tex: TextureSampler<f32> = "searchTex",
        output: RenderTarget<Rgba8> = "OutColor",
    });
    gfx_pipeline!(neighborhood_blending_pipe {
        color_tex: TextureSampler<[f32; 4]> = "colorTex",
        blend_tex: TextureSampler<[f32; 4]> = "blendTex",
        output: RenderTarget<Srgba8> = "OutColor",
    });
}
use pipelines::*;

/// A `SmaaTarget` wraps a color and depth buffer, which it can resolve into an antialiased image
/// using the [Subpixel Morphological Antialiasing (SMAA)](http://www.iryoku.com/smaa) algorithm.
pub struct SmaaTarget<R: Resources> {
    /// Render target for actual frame data.
    color_target: RenderTargetView<R, Srgba8>,

    /// Associated depth stencil target.
    depth_target: DepthStencilView<R, DepthStencil>,

    // Internal render targets used to compute antialiasing.
    edges_target: RenderTargetView<R, (R8_G8, Unorm)>,
    blend_target: RenderTargetView<R, Rgba8>,

    // Pipeline state objects.
    edge_detection_pso: PipelineState<R, edge_detection_pipe::Meta>,
    blending_weight_pso: PipelineState<R, blending_weight_pipe::Meta>,
    neighborhood_blending_pso: PipelineState<R, neighborhood_blending_pipe::Meta>,

    // Pipeline state data.
    edge_detection_data: edge_detection_pipe::Data<R>,
    blending_weight_data: blending_weight_pipe::Data<R>,
    neighborhood_blending_data: neighborhood_blending_pipe::Data<R>,
}

impl<R: Resources> SmaaTarget<R> {
    /// Create a new `SmaaTarget`.
    pub fn new<F: Factory<R>>(
        factory: &mut F,
        frame_buffer: RenderTargetView<R, Srgba8>,
        width: u16,
        height: u16,
    ) -> Result<Self, Error> {
        let depth_target = factory.create_depth_stencil(width, height)?.2;
        let (_, color_view, color_target) = factory.create_render_target(width, height)?;
        let (_, edges_view, edges_target) = factory.create_render_target(width, height)?;
        let (_, blend_view, blend_target) = factory.create_render_target(width, height)?;

        let area_texture = factory
            .create_texture_immutable::<(R8_G8, Unorm)>(
                Kind::D2(AREATEX_WIDTH, AREATEX_HEIGHT, AaMode::Single),
                Mipmap::Provided,
                &[memory::cast_slice(&AREATEX_BYTES)],
            )?
            .1;
        let search_texture = factory
            .create_texture_immutable::<(R8, Unorm)>(
                Kind::D2(SEARCHTEX_WIDTH, SEARCHTEX_HEIGHT, AaMode::Single),
                Mipmap::Provided,
                &[&SEARCHTEX_BYTES],
            )?
            .1;

        let ss = ShaderSource {
            width,
            height,
            quality: ShaderQuality::High,
        };

        let texture_sampler =
            factory.create_sampler(SamplerInfo::new(FilterMethod::Bilinear, WrapMode::Clamp));
        let rasterizer = gfx::state::Rasterizer {
            front_face: gfx::state::FrontFace::Clockwise,
            cull_face: gfx::state::CullFace::Nothing,
            method: gfx::state::RasterMethod::Fill,
            offset: None,
            samples: None,
        };

        let edge_detection_shader = factory.create_shader_set(
            ss.get_stage(ShaderStage::EdgeDetectionVS).as_ref(),
            ss.get_stage(ShaderStage::LumaEdgeDetectionPS)
                .as_ref(),
        )?;
        let blending_weight_shader = factory.create_shader_set(
            ss.get_stage(ShaderStage::BlendingWeightVS).as_ref(),
            ss.get_stage(ShaderStage::BlendingWeightPS).as_ref(),
        )?;
        let neigborhood_blending_shader = factory.create_shader_set(
            ss.get_stage(ShaderStage::NeighborhoodBlendingVS)
                .as_ref(),
            ss.get_stage(ShaderStage::NeighborhoodBlendingPS)
                .as_ref(),
        )?;

        Ok(Self {
            color_target,
            depth_target,
            edge_detection_pso: factory.create_pipeline_state(
                &edge_detection_shader,
                gfx::Primitive::TriangleList,
                rasterizer,
                edge_detection_pipe::new(),
            )?,
            blending_weight_pso: factory.create_pipeline_state(
                &blending_weight_shader,
                gfx::Primitive::TriangleList,
                rasterizer,
                blending_weight_pipe::new(),
            )?,
            neighborhood_blending_pso: factory.create_pipeline_state(
                &neigborhood_blending_shader,
                gfx::Primitive::TriangleList,
                rasterizer,
                neighborhood_blending_pipe::new(),
            )?,
            edge_detection_data: edge_detection_pipe::Data {
                color_tex: (color_view.clone(), texture_sampler.clone()),
                output: edges_target.clone(),
            },
            blending_weight_data: blending_weight_pipe::Data {
                edges_tex: (edges_view, texture_sampler.clone()),
                area_tex: (area_texture.clone(), texture_sampler.clone()),
                search_tex: (search_texture.clone(), texture_sampler.clone()),
                output: blend_target.clone(),
            },
            neighborhood_blending_data: neighborhood_blending_pipe::Data {
                color_tex: (color_view.clone(), texture_sampler.clone()),
                blend_tex: (blend_view, texture_sampler),
                output: frame_buffer,
            },
            edges_target,
            blend_target,
        })
    }

    /// Get the color buffer associated with this target.
    pub fn output_color(&self) -> &RenderTargetView<R, Srgba8> {
        &self.color_target
    }

    /// Get the depth/stencil buffer associated with this target.
    pub fn output_stencil(&self) -> &DepthStencilView<R, DepthStencil> {
        &self.depth_target
    }

    /// Do a multisample resolve, outputing to the frame buffer specified at creation time.
    pub fn resolve<C: command::Buffer<R>>(&mut self, encoder: &mut gfx::Encoder<R, C>) {
        let slice = gfx::Slice {
            start: 0,
            end: 6,
            base_vertex: 0,
            instances: None,
            buffer: gfx::IndexBuffer::Auto,
        };

        encoder.draw(&slice, &self.edge_detection_pso, &self.edge_detection_data);
        encoder.draw(
            &slice,
            &self.blending_weight_pso,
            &self.blending_weight_data,
        );
        encoder.draw(
            &slice,
            &self.neighborhood_blending_pso,
            &self.neighborhood_blending_data,
        );

        encoder.clear(&self.edges_target, [0.0, 0.0]);
        encoder.clear(&self.blend_target, [0.0, 0.0, 0.0, 0.0]);
    }
}
