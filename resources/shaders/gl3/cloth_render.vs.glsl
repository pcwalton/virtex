#version 330

// virtex/resources/shaders/cloth_render.vs.glsl

precision highp float;

uniform mat4 uTransform;
uniform sampler2D uVertexPositions;
uniform vec2 uVertexPositionsSize;

in vec2 aPosition;

out vec2 vTexCoord;

void main() {
    vec3 displacement = texture(uVertexPositions, (aPosition + 0.5) / uVertexPositionsSize).xyz;
    vec4 ndcPosition = uTransform * vec4(displacement + vec3(aPosition, 0.0), 1.0);
    vTexCoord = aPosition / uVertexPositionsSize;
    gl_Position = ndcPosition;
}
