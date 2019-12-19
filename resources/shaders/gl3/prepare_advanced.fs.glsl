#version 330

// virtex/resources/shaders/prepare_advanced.fs.glsl

precision highp float;

uniform vec2 uTileSize;
uniform vec2 uViewportScaleFactor;

in vec2 vTexCoord;

out vec4 cFragColor;

float getMipLevel(vec2 texCoord) {
    vec2 dUVDX = dFdx(texCoord), dUVDY = dFdy(texCoord);
    float deltaMaxSq = max(dot(dUVDX, dUVDX), dot(dUVDY, dUVDY));
    return -0.5 * log2(deltaMaxSq);
}

void main() {
    float neededMipLevel = ceil(getMipLevel(vTexCoord * uViewportScaleFactor));
    vec2 scaledTexCoord = vTexCoord * pow(2.0, neededMipLevel) / uTileSize;
    vec2 neededTileOrigin = floor(scaledTexCoord);

    cFragColor = vec4(neededTileOrigin, neededMipLevel, 1.0);
}
