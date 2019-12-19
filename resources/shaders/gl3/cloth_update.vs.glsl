#version 330

// virtex/resources/shaders/cloth_update.vs.glsl

precision highp float;

in vec2 aPosition;

out vec2 vTexCoord;

void main() {
    vTexCoord = aPosition;
    gl_Position = vec4(mix(vec2(-1.0), vec2(1.0), aPosition), 0.0, 1.0);
}
