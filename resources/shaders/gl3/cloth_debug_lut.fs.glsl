#version 330

// virtex/resources/shaders/cloth_debug_lut.fs.glsl

precision highp float;

uniform sampler2D uPositions;
uniform float uPositionScale;

in vec2 vTexCoord;

out vec4 cFragColor;

void main() {
    cFragColor = vec4(texture(uPositions, vTexCoord).rgb * vec3(0.02), 1.0);
}
