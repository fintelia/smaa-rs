
#[allow(dead_code)]
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

    NeighborhoodBlendingAcesTonemapPS,
}
impl ShaderStage {
    fn is_vertex_shader(&self) -> bool {
        match *self {
            ShaderStage::EdgeDetectionVS |
            ShaderStage::BlendingWeightVS |
            ShaderStage::NeighborhoodBlendingVS => true,

            ShaderStage::LumaEdgeDetectionPS |
            ShaderStage::BlendingWeightPS |
            ShaderStage::NeighborhoodBlendingPS |
            ShaderStage::NeighborhoodBlendingAcesTonemapPS => false,
        }
    }
    fn as_str(&self) -> &'static str {
        match *self {
            ShaderStage::EdgeDetectionVS => {
                "out float4 offset[3];
                 out float2 texcoord;
                 void main() {
                     if(gl_VertexID == 0) gl_Position = vec4(-1, -1, 1, 1);
                     if(gl_VertexID == 1) gl_Position = vec4(-1,  1, 1, 1);
        	         if(gl_VertexID == 2) gl_Position = vec4( 1,  1, 1, 1);
        	         if(gl_VertexID == 3) gl_Position = vec4(-1, -1, 1, 1);
        	         if(gl_VertexID == 5) gl_Position = vec4( 1,  1, 1, 1);
        	         if(gl_VertexID == 4) gl_Position = vec4( 1, -1, 1, 1);
                     texcoord = gl_Position.xy * 0.5 + vec2(0.5);
                     SMAAEdgeDetectionVS(texcoord, offset);
                 }"
            }
            ShaderStage::BlendingWeightVS => {
                "out float2 pixcoord;
                 out float4 offset[3];
                 out float2 texcoord;
                 void main() {
                     if(gl_VertexID == 0) gl_Position = vec4(-1, -1, 1, 1);
                     if(gl_VertexID == 1) gl_Position = vec4(-1,  1, 1, 1);
        	         if(gl_VertexID == 2) gl_Position = vec4( 1,  1, 1, 1);
        	         if(gl_VertexID == 3) gl_Position = vec4(-1, -1, 1, 1);
        	         if(gl_VertexID == 5) gl_Position = vec4( 1,  1, 1, 1);
        	         if(gl_VertexID == 4) gl_Position = vec4( 1, -1, 1, 1);
                     texcoord = gl_Position.xy * 0.5 + vec2(0.5);
                     SMAABlendingWeightCalculationVS(texcoord, pixcoord, offset);
                 }"
            }
            ShaderStage::NeighborhoodBlendingVS => {
                "out float4 offset;
                 out float2 texcoord;
                 void main() {
                     if(gl_VertexID == 0) gl_Position = vec4(-1, -1, 1, 1);
                     if(gl_VertexID == 1) gl_Position = vec4(-1,  1, 1, 1);
        	         if(gl_VertexID == 2) gl_Position = vec4( 1,  1, 1, 1);
        	         if(gl_VertexID == 3) gl_Position = vec4(-1, -1, 1, 1);
        	         if(gl_VertexID == 5) gl_Position = vec4( 1,  1, 1, 1);
        	         if(gl_VertexID == 4) gl_Position = vec4( 1, -1, 1, 1);
                     texcoord = gl_Position.xy * 0.5 + vec2(0.5);
                     SMAANeighborhoodBlendingVS(texcoord, offset);
                 }"
            }
            ShaderStage::LumaEdgeDetectionPS => {
                "in float4 offset[3];
                 in float2 texcoord;
                 uniform sampler2D colorTex;
                 out float2 OutColor;
                 void main() {
                     OutColor = SMAALumaEdgeDetectionPS(texcoord, offset, colorTex);
                 }"
            }
            ShaderStage::BlendingWeightPS => {
                "in float2 pixcoord;
                 in float4 offset[3];
                 in float2 texcoord;
                 uniform sampler2D edgesTex;
                 uniform sampler2D areaTex;
                 uniform sampler2D searchTex;
                 out float4 OutColor;
                 void main() {
                     vec4 subsampleIndices = vec4(0);
                     OutColor = SMAABlendingWeightCalculationPS(texcoord, pixcoord, offset,
                         edgesTex, areaTex, searchTex, subsampleIndices);
                 }"
            }
            ShaderStage::NeighborhoodBlendingPS => {
                "in float4 offset;
                 in float2 texcoord;
                 uniform sampler2D colorTex;
                 uniform sampler2D blendTex;
                 out float4 OutColor;
                 void main() {
                     OutColor = SMAANeighborhoodBlendingPS(texcoord, offset, colorTex, blendTex);
                 }"
            }
            // See: https://knarkowicz.wordpress.com/2016/01/06/aces-filmic-tone-mapping-curve
            ShaderStage::NeighborhoodBlendingAcesTonemapPS => {
                "in float4 offset;
                 in float2 texcoord;
                 uniform sampler2D colorTex;
                 uniform sampler2D blendTex;
                 out float4 OutColor;
                 void main() {
                     float a = 2.51f;
                     float b = 0.03f;
                     float c = 2.43f;
                     float d = 0.59f;
                     float e = 0.14f;
                     OutColor = SMAANeighborhoodBlendingPS(texcoord, offset, colorTex, blendTex);
                     vec3 x = OutColor.rgb;
                     OutColor.rgb = clamp((x*(a*x+b))/(x*(c*x+d)+e), vec3(0), vec3(1));
                 }"
            }
        }
    }
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
            #define SMAA_INCLUDE_{3} 0
            {4}
            {5}",
            self.width,
            self.height,
            self.quality.as_str(),
            if stage.is_vertex_shader() {"PS"} else {"VS"},
            include_str!("../third_party/smaa/SMAA.hlsl"),
            stage.as_str(),
        )
    }
}
