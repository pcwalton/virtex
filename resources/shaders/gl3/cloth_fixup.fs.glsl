#version 330

// virtex/resources/shaders/cloth_fixup.fs.glsl

precision highp float;

uniform sampler2D uLastPositions;
uniform vec2 uFramebufferSize;
uniform float uMaxStretch;

in vec2 vTexCoord;

out vec4 cFragColor;

void check(vec2 offset,
           vec3 thisPosition,
           inout float outNeighborStretch,
           inout vec2 outNeighborOffset,
           inout vec3 outNeighborPosition) {
    vec2 neighborTexCoord = vTexCoord + offset / uFramebufferSize;
    if (all(greaterThan(neighborTexCoord, vec2(0.0))) &&
        all(lessThan(neighborTexCoord, vec2(1.0)))) {
        vec3 neighborPosition = texture(uLastPositions, neighborTexCoord).xyz + vec3(offset, 0.0);
        float neighborStretch = length(neighborPosition - thisPosition) / length(offset);
        float stretchFactor = abs(neighborStretch - 1.0);
        float currentStretchFactor = abs(outNeighborStretch - 1.0);
        if (stretchFactor > max(uMaxStretch, currentStretchFactor)) {
            outNeighborStretch = neighborStretch;
            outNeighborOffset = offset;
            outNeighborPosition = neighborPosition;
        }
    }
}

// FIXME(pcwalton): Iterate through *edges*, not *vertices*.
void main() {
    vec2 fragCoord = floor(vTexCoord * uFramebufferSize);
    vec3 thisPosition = texture(uLastPositions, vTexCoord).xyz;

    vec3 newPosition = thisPosition;
    if (fragCoord.y + 1.0 < uFramebufferSize.y ||
        (fragCoord.x > 0.0 && fragCoord.x + 1.0 < uFramebufferSize.x)) {
        float neighborStretch = 1.0;
        vec2 neighborOffset = vec2(0.0);
        vec3 neighborPosition = vec3(0.0);

        check(vec2(-1.0, -1.0), thisPosition, neighborStretch, neighborOffset, neighborPosition);
        check(vec2( 0.0, -1.0), thisPosition, neighborStretch, neighborOffset, neighborPosition);
        check(vec2( 1.0, -1.0), thisPosition, neighborStretch, neighborOffset, neighborPosition);
        check(vec2(-1.0,  0.0), thisPosition, neighborStretch, neighborOffset, neighborPosition);
        check(vec2( 1.0,  0.0), thisPosition, neighborStretch, neighborOffset, neighborPosition);
        check(vec2(-1.0,  1.0), thisPosition, neighborStretch, neighborOffset, neighborPosition);
        check(vec2( 0.0,  1.0), thisPosition, neighborStretch, neighborOffset, neighborPosition);
        check(vec2( 1.0,  1.0), thisPosition, neighborStretch, neighborOffset, neighborPosition);

        if (neighborOffset != vec2(0.0)) {
            float fixupDistance = length(neighborOffset);
            if (neighborStretch < 1.0 - uMaxStretch)
                fixupDistance *= 1.0 - uMaxStretch;
            else if (neighborStretch > 1.0 + uMaxStretch)
                fixupDistance *= 1.0 + uMaxStretch;

            newPosition = neighborPosition +
                vec3(fixupDistance) * normalize(thisPosition - neighborPosition);
        }
    }

    cFragColor = vec4(newPosition, 0.0);
}
