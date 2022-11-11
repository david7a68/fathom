#version 450

layout (constant_id = 0) const int num_channels = 3;
layout (constant_id = 1) const int channel_range_max = 255;
// layout (constant_id = 2) const int source_color_space;
// layout (constant_id = 3) const int resample_mode;

layout (local_size_x = 32, local_size_y = 32, local_size_z = 1) in;

layout (set = 0, binding = 0) uniform CopyInfo {
    uvec2 source_extent;
    uvec2 target_offset;
};

layout (set = 0, binding = 1) readonly buffer Source {
    uint source[];
};

layout (set = 0, binding = 2, rgba16f) uniform image2D target;

vec4 get_source_color(uint column, uint row) {
    uint index = (column + row * source_extent.y) * num_channels;

    float r = source[index] / channel_range_max;
    
    float g = 0.0;
    if (num_channels > 1) {
        g = source[index + 1];
    }

    float b = 0.0;
    if (num_channels > 2) {
        b = source[index + 2];
    }

    float a = 1.0;
    if (num_channels > 3) {
        a = source[index + 3];
    }

    return vec4(r, g, b, a);
}

void main() {
    uint column = gl_GlobalInvocationID.x;
    uint row = gl_GlobalInvocationID.y;

    // Early return if we're addressing a pixel out of bounds
    if (column >= source_extent.x || row >= source_extent.y)
        return;

    vec4 source_color = get_source_color(column, row);

    imageStore(
        target,
        ivec2(target_offset.x + column, target_offset.y + row),
        source_color
    );
}
