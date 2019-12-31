#version 330

// virtex/resources/shaders/cloth_render.vs.glsl

precision highp float;

uniform mat4 uTransform;
uniform vec2 uTextureSize;
uniform sampler2D uVertexPositions;
uniform vec2 uVertexPositionsSize;

in vec2 aPosition;

out vec2 vTexCoord;

void main() {
    vec3 displacement = texture(uVertexPositions, (aPosition + 0.5) / uVertexPositionsSize).xyz;
    vec4 ndcPosition = uTransform * vec4(displacement + vec3(aPosition, 0.0), 1.0);

    vec2 texCoord = aPosition / uVertexPositionsSize;
    texCoord.y = 1.0 - texCoord.y;
    vTexCoord = texCoord * uTextureSize;

    gl_Position = ndcPosition;
}
