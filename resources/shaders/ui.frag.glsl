#version 450

layout(location = 0) in vec4 fragColor;

layout(location = 0) out vec4 outColor;

layout(push_constant) uniform FragConstants {
    // offset the 2 `vec2`s used in the vertex shader
    layout(offset=16) uint use_texture;
};

void main() {
    if (use_texture != 0) {
        outColor = vec4(1.0, 0, 0.78, 1.0);
    } else {
        outColor = fragColor;
    }
}
