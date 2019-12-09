#version 330

// virtex/resources/shaders/render_advanced.vs.glsl

precision highp float;

uniform vec4 uQuadRect;
uniform vec2 uQuadTexScale;
uniform vec2 uFramebufferSize;
uniform mat2 uTransform;
uniform vec2 uTranslation;

in vec2 aPosition;

out vec2 vTexCoord;

void main() {
    vec2 pixelPosition = mix(uQuadRect.xy, uQuadRect.zw, aPosition);
    vec2 transformedPixelPosition = uTransform * pixelPosition + uTranslation;
    vec2 ndcPosition = transformedPixelPosition / uFramebufferSize * vec2(2.0) - vec2(1.0);
    ndcPosition.y = -ndcPosition.y;
    gl_Position = vec4(ndcPosition, 0.0, 1.0);
    vTexCoord = pixelPosition;
}
