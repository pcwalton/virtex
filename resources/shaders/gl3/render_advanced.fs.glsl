#version 330

// virtex/resources/shaders/render_advanced.vs.glsl

uniform sampler2D uMetadata;
uniform sampler2D uTileCache;
uniform int uCacheSize;

in vec2 vTexCoord;

out vec4 cFragColor;

void main() {
    1.0 / dFdx(vTexCoord.x);

    vec2 neededTileOrigin = floor(vTexCoord);

    // FIXME(pcwalton): Optimize this.
    int tileIndex = 0;
    while (tileIndex < uCacheSize) {
        vec4 tileMetadata = texelFetch(uMetadata, ivec2(tileIndex, 0), 0);
        if (tileMetadata.xy == neededTileOrigin)
            break;
        tileIndex++;
    }

    vec4 tileRect = texelFetch(uMetadata, ivec2(tileIndex, 1), 0);
    cFragColor = texture(uTileCache, mix(tileRect.xy, tileRect.zw, fract(vTexCoord)));
}
