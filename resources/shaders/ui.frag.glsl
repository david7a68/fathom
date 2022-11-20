#version 450

layout(location = 0) in vec4 fragColor;
layout(location = 1) in vec2 fragUV;

layout(binding = 0) uniform sampler2D texSampler;

layout(location = 0) out vec4 outColor;

layout(push_constant) uniform FragConstants {
    // offset the 2 `vec2`s used in the vertex shader
    layout(offset=16) uint use_texture;
};

void main() {
    if (use_texture != 0) {
        outColor = texture(texSampler, fragUV) * fragColor;
    } else {
        outColor = fragColor;
    }
}
