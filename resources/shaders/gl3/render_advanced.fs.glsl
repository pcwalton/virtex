#version 330

// virtex/resources/shaders/render_advanced.fs.glsl

precision highp float;

uniform sampler2D uMetadata;
uniform sampler2D uTileCache;
uniform int uCacheSize;
uniform vec2 uTileSize;

in vec2 vTexCoord;

out vec4 cFragColor;

float getMipLevel(vec2 texCoord) {
    vec2 dUVDX = dFdx(texCoord), dUVDY = dFdy(texCoord);
    float deltaMaxSq = max(dot(dUVDX, dUVDX), dot(dUVDY, dUVDY));
    return -0.5 * log2(deltaMaxSq);
}

void main() {
    float neededMipLevel = max(ceil(getMipLevel(vTexCoord)), 0.0);
    vec2 scaledTexCoord = vTexCoord * pow(2.0, neededMipLevel) / uTileSize;
    vec2 neededTileOrigin = floor(scaledTexCoord);

    // FIXME(pcwalton): Optimize this.
    int tileIndex = 0;
    while (tileIndex < uCacheSize) {
        vec4 tileMetadata = texelFetch(uMetadata, ivec2(tileIndex, 0), 0);
        if (tileMetadata.xyz == vec3(neededTileOrigin, neededMipLevel))
            break;
        tileIndex++;
    }

    vec4 tileRect = texelFetch(uMetadata, ivec2(tileIndex, 1), 0);
    vec4 fragColor = texture(uTileCache, mix(tileRect.xy, tileRect.zw, fract(scaledTexCoord)));
    cFragColor = fragColor;
}
