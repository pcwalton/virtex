#version 330

// virtex/resources/shaders/render_simple.vs.glsl

precision highp float;

uniform vec4 uTileRect;
uniform vec4 uTileTexRect;
uniform vec2 uFramebufferSize;
uniform mat2 uTransform;
uniform vec2 uTranslation;

in vec2 aPosition;

out vec2 vTexCoord;

void main() {
    vec2 pixelPosition = mix(uTileRect.xy, uTileRect.zw, aPosition);
    pixelPosition = uTransform * pixelPosition + uTranslation;
    vec2 ndcPosition = pixelPosition / uFramebufferSize * vec2(2.0) - vec2(1.0);
    ndcPosition.y = -ndcPosition.y;
    gl_Position = vec4(ndcPosition, 0.0, 1.0);
    vTexCoord = mix(uTileTexRect.xy, uTileTexRect.zw, aPosition);
}
