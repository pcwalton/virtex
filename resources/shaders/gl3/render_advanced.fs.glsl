#version 330

// virtex/resources/shaders/render_advanced.fs.glsl

precision highp float;

uniform sampler2D uMetadata;
uniform sampler2D uTileCache;
uniform int uCacheSeedA;
uniform int uCacheSeedB;
uniform int uCacheSize;
uniform vec2 uTileSize;

in vec2 vTexCoord;

out vec4 cFragColor;

// Murmurhash3
uint hash32(uint h) {
    h ^= h >> 16u;
    h *= 0x85ebca6bu;
    h ^= h >> 13u;
    h *= 0xc2b2ae35u;
    h ^= h >> 16u;
    return h;
}

uint packTileDescriptor(uvec2 tilePosition, int mipLevel) {
    return (uint(mipLevel) & 0x3fu) | (tilePosition.x << 6u) | (tilePosition.y << 19u);
}

float getMipLevel(vec2 texCoord) {
    vec2 dUVDX = dFdx(texCoord), dUVDY = dFdy(texCoord);
    float deltaMaxSq = max(dot(dUVDX, dUVDX), dot(dUVDY, dUVDY));
    return -0.5 * log2(deltaMaxSq);
}

void main() {
    int neededMipLevel = int(ceil(getMipLevel(vTexCoord)));
    vec2 scaledTexCoord = vTexCoord * pow(2.0, float(neededMipLevel)) / uTileSize;
    uvec2 neededTileOrigin = uvec2(floor(scaledTexCoord));

    // FIXME(pcwalton): If this fails, keep searching.
    uint hash = hash32(packTileDescriptor(neededTileOrigin, neededMipLevel));
    ivec2 metadataCoord = ivec2(int((hash ^ uint(uCacheSeedA)) % uint(uCacheSize)), 0);
    vec4 tileMetadata = texelFetch(uMetadata, metadataCoord, 0);
    if (tileMetadata.xyz != vec3(ivec2(neededTileOrigin), neededMipLevel)) {
        metadataCoord = ivec2(int((hash ^ uint(uCacheSeedB)) % uint(uCacheSize)), 2);
        tileMetadata = texelFetch(uMetadata, metadataCoord, 0);
        if (tileMetadata.xyz != vec3(ivec2(neededTileOrigin), neededMipLevel)) {
            cFragColor = vec4(0.0);
            return;
        }
    }

    vec4 tileRect = texelFetch(uMetadata, metadataCoord + ivec2(0, 1), 0);
    vec4 fragColor = texture(uTileCache, mix(tileRect.xy, tileRect.zw, fract(scaledTexCoord)));
    cFragColor = fragColor;
}
