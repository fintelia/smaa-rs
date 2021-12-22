use naga::FastHashMap;

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

#[derive(Copy, Clone)]
pub enum ShaderStage {
    EdgeDetectionVS,
    LumaEdgeDetectionPS,

    BlendingWeightVS,
    BlendingWeightPS,

    NeighborhoodBlendingVS,
    NeighborhoodBlendingPS,

    #[allow(unused)]
    NeighborhoodBlendingAcesTonemapPS,
}
impl ShaderStage {
    fn is_vertex_shader(&self) -> bool {
        match *self {
            ShaderStage::EdgeDetectionVS
            | ShaderStage::BlendingWeightVS
            | ShaderStage::NeighborhoodBlendingVS => true,

            ShaderStage::LumaEdgeDetectionPS
            | ShaderStage::BlendingWeightPS
            | ShaderStage::NeighborhoodBlendingPS
            | ShaderStage::NeighborhoodBlendingAcesTonemapPS => false,
        }
    }
    fn as_str(&self) -> &'static str {
        match *self {
            ShaderStage::EdgeDetectionVS => {
                "layout(location = 0) out float4 offset0;
                 layout(location = 1) out float4 offset1;
                 layout(location = 2) out float4 offset2;
                 layout(location = 3) out float2 texcoord;
                 void main() {
                     if(gl_VertexIndex == 0) gl_Position = vec4(-1, -1, 1, 1);
                     if(gl_VertexIndex == 1) gl_Position = vec4(-1,  3, 1, 1);
        	         if(gl_VertexIndex == 2) gl_Position = vec4( 3, -1, 1, 1);
                     texcoord = gl_Position.xy * vec2(0.5) + vec2(0.5);
                     float4 offset[3];
                     SMAAEdgeDetectionVS(texcoord, offset);
                     offset0=offset[0];
                     offset1=offset[1];
                     offset2=offset[2];
                 }"
            }
            ShaderStage::BlendingWeightVS => {
                "layout(location = 0) out float2 pixcoord;
                 layout(location = 1) out float4 offset0;
                 layout(location = 2) out float4 offset1;
                 layout(location = 3) out float4 offset2;
                 layout(location = 4) out float2 texcoord;
                 void main() {
                     if(gl_VertexIndex == 0) gl_Position = vec4(-1, -1, 1, 1);
                     if(gl_VertexIndex == 1) gl_Position = vec4(-1,  3, 1, 1);
        	         if(gl_VertexIndex == 2) gl_Position = vec4( 3, -1, 1, 1);
                     texcoord = gl_Position.xy * vec2(0.5) + vec2(0.5);
                     float4 offset[3];
                     SMAABlendingWeightCalculationVS(texcoord, pixcoord, offset);
                     offset0=offset[0];
                     offset1=offset[1];
                     offset2=offset[2];
                 }"
            }
            ShaderStage::NeighborhoodBlendingVS => {
                "layout(location = 0) out float4 offset;
                 layout(location = 1) out float2 texcoord;
                 void main() {
                     if(gl_VertexIndex == 0) gl_Position = vec4(-1, -1, 1, 1);
                     if(gl_VertexIndex == 1) gl_Position = vec4(-1,  3, 1, 1);
        	         if(gl_VertexIndex == 2) gl_Position = vec4( 3, -1, 1, 1);
                     texcoord = gl_Position.xy * vec2(0.5) + vec2(0.5);
                     SMAANeighborhoodBlendingVS(texcoord, offset);
                 }"
            }
            ShaderStage::LumaEdgeDetectionPS => {
                "layout(location = 0) in float4 offset0;
                 layout(location = 1) in float4 offset1;
                 layout(location = 2) in float4 offset2;
                 layout(location = 3) in float2 texcoord;
                 layout(set = 0, binding = 2) uniform texture2D colorTex;
                 layout(location = 0) out float2 OutColor;
                 void main() {
                    float4 offset[3];
                    offset[0] = offset0;
                    offset[1] = offset1;
                    offset[2] = offset2;
                    OutColor = SMAALumaEdgeDetectionPS(texcoord, offset, colorTex);
                 }"
            }
            ShaderStage::BlendingWeightPS => {
                "layout(location = 0) in float2 pixcoord;
                 layout(location = 1) in float4 offset0;
                 layout(location = 2) in float4 offset1;
                 layout(location = 3) in float4 offset2;
                 layout(location = 4) in float2 texcoord;
                 layout(set = 0, binding = 2) uniform texture2D edgesTex;
                 layout(set = 0, binding = 3) uniform texture2D areaTex;
                 layout(set = 0, binding = 4) uniform texture2D searchTex;
                 layout(location = 0) out float4 OutColor;
                 void main() {
                     vec4 subsampleIndices = vec4(0);
                     float4 offset[3];
                     offset[0] = offset0;
                     offset[1] = offset1;
                     offset[2] = offset2;
                     OutColor = SMAABlendingWeightCalculationPS(texcoord, pixcoord, offset,
                         edgesTex, areaTex, searchTex, subsampleIndices);
                 }"
            }
            ShaderStage::NeighborhoodBlendingPS => {
                "layout(location = 0) in float4 offset;
                 layout(location = 1) in float2 texcoord;
                 layout(set = 0, binding = 2) uniform texture2D colorTex;
                 layout(set = 0, binding = 3) uniform texture2D blendTex;
                 layout(location = 0) out float4 OutColor;
                 void main() {
                     OutColor = SMAANeighborhoodBlendingPS(texcoord, offset, colorTex, blendTex);
                 }"
            }
            // See: https://knarkowicz.wordpress.com/2016/01/06/aces-filmic-tone-mapping-curve
            ShaderStage::NeighborhoodBlendingAcesTonemapPS => {
                "layout(location = 0) in float4 offset;
                 layout(location = 1) in float2 texcoord;
                 layout(set = 0, binding = 2) uniform texture2D colorTex;
                 layout(set = 0, binding = 3) uniform texture2D blendTex;
                 layout(location = 0) out float4 OutColor;
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
    pub quality: ShaderQuality,
}
impl ShaderSource {
    fn get_stage(&self, stage: ShaderStage) -> String {
        format!(
            "#version 450 core
            #extension GL_EXT_samplerless_texture_functions: require
            #define SMAA_GLSL_4
            #define SMAA_PRESET_{0}
            #define SMAA_INCLUDE_{1} 0
            #define SMAA_RT_METRICS uniforms.rt
            layout(set = 0, binding = 0) uniform sampler linearSampler;
            layout(set = 0, binding = 1) uniform UniformBlock {{
                vec4 rt;
            }} uniforms;
            {2}
            {3}",
            self.quality.as_str(),
            if stage.is_vertex_shader() { "PS" } else { "VS" },
            include_str!("../third_party/smaa/SMAA.hlsl"),
            stage.as_str(),
        )
    }
    pub fn get_shader(
        &self,
        device: &wgpu::Device,
        stage: ShaderStage,
        name: &'static str,
    ) -> wgpu::ShaderModule {
        let source = self.get_stage(stage);

        std::fs::write(name, &source).unwrap();

        let mut entry_points = FastHashMap::default();
        entry_points.insert(
            "main".to_string(),
            if stage.is_vertex_shader() {
                naga::ShaderStage::Vertex
            } else {
                naga::ShaderStage::Fragment
            },
        );

        let mut parser = naga::front::glsl::Parser::default();
        let module = parser
            .parse(
                &naga::front::glsl::Options {
                    defines: Default::default(),
                    stage: if stage.is_vertex_shader() {
                        naga::ShaderStage::Vertex
                    } else {
                        naga::ShaderStage::Fragment
                    },
                },
                &source,
            )
            .unwrap();

        let module_info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::empty(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();

        let spirv = naga::back::spv::write_vec(
            &module,
            &module_info,
            &Default::default(),
            Some(&naga::back::spv::PipelineOptions {
                entry_point: "main".to_string(),
                shader_stage: if stage.is_vertex_shader() {
                    naga::ShaderStage::Vertex
                } else {
                    naga::ShaderStage::Fragment
                },
            }),
        )
        .unwrap();

        device.create_shader_module(&wgpu::ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::SpirV(spirv.into()),
        })
    }
}
