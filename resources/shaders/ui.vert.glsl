#version 450

layout(location = 0) in ivec2 position;
layout(location = 1) in vec4 color;
layout(location = 2) in ivec2 uv;

layout(push_constant) uniform VertConstants {
    vec2 scale;
    vec2 translate;
};

layout(location = 0) out vec4 fragColor;

void main() {
    gl_Position = vec4(position * scale + translate, 0.0, 1.0);
    fragColor = color;
}
