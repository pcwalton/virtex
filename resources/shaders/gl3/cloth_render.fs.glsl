#version 330

// virtex/resources/shaders/cloth_render.fs.glsl

precision highp float;

uniform sampler2D uTexture;

in vec2 vTexCoord;

out vec4 cFragColor;

void main() {
    cFragColor = vec4(texture(uTexture, vTexCoord).rgb, 1.0);
}
