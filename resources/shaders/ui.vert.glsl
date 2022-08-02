#version 450

layout(location = 0) in ivec2 position;
layout(location = 1) in vec4 color;

layout(push_constant) uniform PushConstants {
    vec2 scale;
    vec2 translate;
};

layout(location = 0) out vec4 fragColor;

void main() {
    gl_Position = vec4(position * scale + translate, 0.0, 1.0);
    fragColor = color;
}
