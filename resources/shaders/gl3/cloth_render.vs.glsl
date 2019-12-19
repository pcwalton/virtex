#version 330

// virtex/resources/shaders/cloth_render.vs.glsl

precision highp float;

uniform mat4 uTransform;
uniform sampler2D uVertexPositions;
uniform vec2 uVertexPositionsSize;

in vec2 aPosition;

void main() {
    vec3 position = texture(uVertexPositions, (aPosition + 0.5) / uVertexPositionsSize).xyz;
    vec4 ndcPosition = uTransform * vec4(position, 1.0);
    gl_Position = ndcPosition;
}
