extern crate failure;
extern crate gfx;

mod shader;
use shader::{ShaderSource, ShaderStage, ShaderQuality};

#[path = "../third_party/smaa/Textures/AreaTex.rs"]
mod area_tex;

#[path = "../third_party/smaa/Textures/SearchTex.rs"]
mod search_tex;

use gfx::{Resources, Factory, ShaderSet};
use gfx::format::{R8_G8, Unorm, Rgba8};
use gfx::handle::RenderTargetView;
use gfx::traits::FactoryExt;
use failure::Error;

pub enum SmaaLevel {
    Disabled,
    Smaa_1x,
}

pub struct SmaaTarget<R: Resources> {
    level: SmaaLevel,
    edges_target: RenderTargetView<R, (R8_G8, Unorm)>,
    blend_target: RenderTargetView<R, Rgba8>,

    edge_detection_shader: ShaderSet<R>,
    blending_weight_shader: ShaderSet<R>,
    neighborhood_blending_shader: ShaderSet<R>,
}

impl<R: Resources> SmaaTarget<R> {
    pub fn new<F: Factory<R>>(
        factory: &mut F,
        width: u16,
        height: u16,
        level: SmaaLevel,
    ) -> Result<Self, Error> {

        let edges_target = factory.create_render_target(width, height)?.2;
        let blend_target = factory.create_render_target(width, height)?.2;

        let ss = ShaderSource {
            width,
            height,
            quality: ShaderQuality::High,
        };

        #[cfg_attr(rustfmt, rustfmt_skip)]
        Ok(Self {
            level,
            edges_target,
            blend_target,
            edge_detection_shader: factory.create_shader_set(
                ss.get_stage(ShaderStage::EdgeDetectionVS).as_ref(),
                ss.get_stage(ShaderStage::LumaEdgeDetectionPS).as_ref(),
            )?,
            blending_weight_shader: factory.create_shader_set(
                ss.get_stage(ShaderStage::BlendingWeightVS).as_ref(),
                ss.get_stage(ShaderStage::BlendingWeightPS).as_ref(),
            )?,
            neighborhood_blending_shader: factory.create_shader_set(
                ss.get_stage(ShaderStage::NeighborhoodBlendingVS).as_ref(),
                ss.get_stage(ShaderStage::NeighborhoodBlendingPS).as_ref(),
            )?,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
