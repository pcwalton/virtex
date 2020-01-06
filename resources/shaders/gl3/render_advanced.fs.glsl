#version 330

// virtex/resources/shaders/render_advanced.fs.glsl

precision highp float;

uniform sampler2D uMetadata;
uniform sampler2D uTileCache;
uniform int uCacheSeedA;
uniform int uCacheSeedB;
uniform int uCacheSize;
uniform vec2 uTileSize;
uniform vec2 uLODRange;
uniform vec4 uBackgroundColor;

in vec2 vTexCoord;

out vec4 cFragColor;

// Murmurhash3
uint VTHash32(uint h) {
    h ^= h >> 16u;
    h *= 0x85ebca6bu;
    h ^= h >> 13u;
    h *= 0xc2b2ae35u;
    h ^= h >> 16u;
    return h;
}

uint VTPackTileDescriptor(uvec2 tilePosition, int mipLevel) {
    return (uint(mipLevel) & 0x3fu) | (tilePosition.x << 6u) | (tilePosition.y << 19u);
}

float VTGetMipLevel(vec2 texCoord) {
    vec2 dUVDX = dFdx(texCoord), dUVDY = dFdy(texCoord);
    float deltaMaxSq = max(dot(dUVDX, dUVDX), dot(dUVDY, dUVDY));
    return -0.5 * log2(deltaMaxSq);
}

bool VTLookupTileMetadataInSubtable(sampler2D metadata,
                                    vec3 neededMetadata,
                                    uint hash,
                                    uint cacheSeed,
                                    uint cacheSize,
                                    int table,
                                    out ivec2 outMetadataCoord) {
    ivec2 metadataCoord = ivec2(int((hash ^ cacheSeed) % cacheSize), table * 2);
    vec3 tileMetadata = texelFetch(metadata, metadataCoord, 0).rgb;
    outMetadataCoord = metadataCoord;
    return tileMetadata == neededMetadata;
}

bool VTLookupTileMetadata(sampler2D metadata,
                          vec2 texCoord,
                          int neededMipLevel,
                          uint cacheSeedA,
                          uint cacheSeedB,
                          uint cacheSize,
                          out ivec2 outMetadataCoord,
                          out vec2 outScaledTexCoord) {
    vec2 scaledTexCoord = texCoord * exp2(float(neededMipLevel)) / uTileSize;
    uvec2 neededTileOrigin = uvec2(floor(scaledTexCoord));
    vec3 neededMetadata = vec3(ivec2(neededTileOrigin), neededMipLevel);

    uint hash = VTHash32(VTPackTileDescriptor(neededTileOrigin, neededMipLevel));
    ivec2 metadataCoord;
    bool found = VTLookupTileMetadataInSubtable(metadata,
                                                neededMetadata,
                                                hash,
                                                cacheSeedA,
                                                cacheSize,
                                                0,
                                                metadataCoord);
    if (!found) {
        found = VTLookupTileMetadataInSubtable(metadata,
                                               neededMetadata,
                                               hash,
                                               cacheSeedB,
                                               cacheSize,
                                               1,
                                               metadataCoord);
    }

    outMetadataCoord = metadataCoord;
    outScaledTexCoord = scaledTexCoord;
    return found;
}

vec4 VTSampleColor(sampler2D metadata,
                   ivec2 metadataCoord,
                   sampler2D tileCache,
                   vec2 scaledTexCoord) {
    vec4 tileRect = texelFetch(metadata, metadataCoord + ivec2(0, 1), 0);
    return texture(tileCache, mix(tileRect.xy, tileRect.zw, fract(scaledTexCoord)));
}

vec4 VTGetColor(sampler2D metadata,
                sampler2D tileCache,
                vec2 texCoord,
                uint cacheSeedA,
                uint cacheSeedB,
                uint cacheSize,
                ivec2 lodRange,
                vec4 backgroundColor) {
    float desiredMipLevel = VTGetMipLevel(texCoord);
    ivec2 desiredMipLevels = clamp(ivec2(floor(desiredMipLevel), ceil(desiredMipLevel)),
                                   lodRange.x,
                                   lodRange.y);

    int lowerMipLevel, upperMipLevel;
    ivec2 lowerMetadataCoord, upperMetadataCoord;
    vec2 lowerScaledTexCoord, upperScaledTexCoord;
    bool lowerFound = false, upperFound = false;
    for (lowerMipLevel = desiredMipLevels.x;
         !lowerFound && lowerMipLevel >= lodRange.x;
         lowerMipLevel--) {
        lowerFound = VTLookupTileMetadata(metadata,
                                          texCoord,
                                          lowerMipLevel,
                                          cacheSeedA,
                                          cacheSeedB,
                                          cacheSize,
                                          lowerMetadataCoord,
                                          lowerScaledTexCoord);
    }
    for (upperMipLevel = desiredMipLevels.y;
         !upperFound && upperMipLevel <= lodRange.y;
         upperMipLevel++) {
        upperFound = VTLookupTileMetadata(metadata,
                                          texCoord,
                                          upperMipLevel,
                                          cacheSeedA,
                                          cacheSeedB,
                                          cacheSize,
                                          upperMetadataCoord,
                                          upperScaledTexCoord);
    }

    vec4 lowerColor, upperColor;
    if (lowerFound)
        lowerColor = VTSampleColor(metadata, lowerMetadataCoord, tileCache, lowerScaledTexCoord);
    if (upperFound)
        upperColor = VTSampleColor(metadata, upperMetadataCoord, tileCache, upperScaledTexCoord);

    vec4 fragColor = backgroundColor;
    if (lowerFound && upperFound) {
        float t = (desiredMipLevel - float(lowerMipLevel)) / float(upperMipLevel - lowerMipLevel);
        fragColor = mix(lowerColor, upperColor, t);
    } else if (lowerFound) {
        fragColor = lowerColor;
    } else if (upperFound) {
        fragColor = upperColor;
    }
    return fragColor;
}

void main() {
    cFragColor = VTGetColor(uMetadata,
                            uTileCache,
                            vTexCoord,
                            uint(uCacheSeedA),
                            uint(uCacheSeedB),
                            uint(uCacheSize),
                            ivec2(uLODRange),
                            uBackgroundColor);
}
