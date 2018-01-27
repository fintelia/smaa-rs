
pub enum ShaderQuality {
    Low,
    Medium,
    High,
    Ultra,
}
impl ShaderQuality {
    fn as_str(&self) -> &'static str {
        match *self {
            ShaderQuality::Low => "LOW",
            ShaderQuality::Medium => "MEDIUM",
            ShaderQuality::High => "HIGH",
            ShaderQuality::Ultra => "ULTRA",
        }
    }
}

pub enum ShaderStage {
    EdgeDetectionVS,
    LumaEdgeDetectionPS,

    BlendingWeightVS,
    BlendingWeightPS,

    NeighborhoodBlendingVS,
    NeighborhoodBlendingPS,
}

pub(crate) struct ShaderSource {
    pub width: u16,
    pub height: u16,
    pub quality: ShaderQuality,
}
impl ShaderSource {
    pub fn get_stage(&self, stage: ShaderStage) -> String {
        format!(
            "#version 330
            #define SMAA_RT_METRICS float4(1.0 / {0}.0, 1.0 / {1}.0, {0}.0, {1}.0)
            #define SMAA_GLSL_3
            #define SMAA_PRESET_{2}
            {3}",
            self.width,
            self.height,
            self.quality.as_str(),
            include_str!("../third_party/smaa/SMAA.hlsl")
        )
    }
}
