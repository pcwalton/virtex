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

bool lookupTileMetadataInSubtable(vec3 neededMetadata,
                                  uint hash,
                                  uint cacheSeed,
                                  int table,
                                  out ivec2 outMetadataCoord) {
    ivec2 metadataCoord = ivec2(int((hash ^ cacheSeed) % uint(uCacheSize)), table * 2);
    vec3 tileMetadata = texelFetch(uMetadata, metadataCoord, 0).rgb;
    outMetadataCoord = metadataCoord;
    return tileMetadata == neededMetadata;
}

bool lookupTileMetadata(int neededMipLevel,
                        out ivec2 outMetadataCoord,
                        out vec2 outScaledTexCoord) {
    vec2 scaledTexCoord = vTexCoord * pow(2.0, float(neededMipLevel)) / uTileSize;
    uvec2 neededTileOrigin = uvec2(floor(scaledTexCoord));
    vec3 neededMetadata = vec3(ivec2(neededTileOrigin), neededMipLevel);

    uint hash = hash32(packTileDescriptor(neededTileOrigin, neededMipLevel));
    ivec2 metadataCoord;
    bool found = lookupTileMetadataInSubtable(neededMetadata,
                                              hash,
                                              uint(uCacheSeedA),
                                              0,
                                              metadataCoord);
    if (!found) {
        found = lookupTileMetadataInSubtable(neededMetadata,
                                             hash,
                                             uint(uCacheSeedB),
                                             1,
                                             metadataCoord);
    }

    outMetadataCoord = metadataCoord;
    outScaledTexCoord = scaledTexCoord;
    return found;
}

vec4 getColor(ivec2 metadataCoord, vec2 scaledTexCoord) {
    vec4 tileRect = texelFetch(uMetadata, metadataCoord + ivec2(0, 1), 0);
    return texture(uTileCache, mix(tileRect.xy, tileRect.zw, fract(scaledTexCoord)));
}

void main() {
    float desiredMipLevel = getMipLevel(vTexCoord);
    ivec2 lodRange = ivec2(uLODRange);
    ivec2 desiredMipLevels = clamp(ivec2(floor(desiredMipLevel), ceil(desiredMipLevel)),
                                   lodRange.x,
                                   lodRange.y);

    int lowerMipLevel, upperMipLevel;
    ivec2 lowerMetadataCoord, upperMetadataCoord;
    vec2 lowerScaledTexCoord, upperScaledTexCoord;
    bool lowerFound = false, upperFound = false;
    for (lowerMipLevel = desiredMipLevels.x;
         !lowerFound && lowerMipLevel >= lodRange.x;
         lowerMipLevel--)
        lowerFound = lookupTileMetadata(lowerMipLevel, lowerMetadataCoord, lowerScaledTexCoord);
    for (upperMipLevel = desiredMipLevels.y;
         !upperFound && upperMipLevel <= lodRange.y;
         upperMipLevel++)
        upperFound = lookupTileMetadata(upperMipLevel, upperMetadataCoord, upperScaledTexCoord);

    vec4 lowerColor, upperColor;
    if (lowerFound)
        lowerColor = getColor(lowerMetadataCoord, lowerScaledTexCoord);
    if (upperFound)
        upperColor = getColor(upperMetadataCoord, upperScaledTexCoord);

    if (lowerFound && upperFound) {
        float fraction = (desiredMipLevel - float(lowerMipLevel)) /
            float(upperMipLevel - lowerMipLevel);
        cFragColor = mix(lowerColor, upperColor, fraction);
    } else if (lowerFound) {
        cFragColor = lowerColor;
    } else if (upperFound) {
        cFragColor = upperColor;
    } else {
        // TODO(pcwalton): Background color.
        cFragColor = vec4(0.0);
    }
}
