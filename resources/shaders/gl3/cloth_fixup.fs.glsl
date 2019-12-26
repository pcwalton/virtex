#version 330

// virtex/resources/shaders/cloth_fixup.fs.glsl

precision highp float;

uniform sampler2D uLastPositions;
uniform vec2 uFramebufferSize;
uniform vec2 uNeighborOffset;
uniform float uMaxStretch;

in vec2 vTexCoord;

out vec4 cFragColor;

void main() {
    vec2 fragCoord = floor(vTexCoord * uFramebufferSize);
    vec3 thisPosition = texture(uLastPositions, vTexCoord).xyz;

    vec3 newPosition = thisPosition;
    if (fragCoord.y + 1.0 < uFramebufferSize.y ||
        (fragCoord.x > 0.0 && fragCoord.x + 1.0 < uFramebufferSize.x)) {
        vec2 neighborOffset = uNeighborOffset;
        //neighborOffset *= vec2(ivec2(fragCoord.xy) % ivec2(2) * ivec2(2) - ivec2(1));
        if (int(fragCoord.x) % 2 == 1)
            neighborOffset.x = -neighborOffset.x;
        if (int(fragCoord.y) % 2 == 1)
            neighborOffset.y = -neighborOffset.y;

        vec2 neighborTexCoord = vTexCoord + neighborOffset / uFramebufferSize;
        if (all(greaterThan(neighborTexCoord, vec2(0.0))) &&
            all(lessThan(neighborTexCoord, vec2(1.0)))) {
            vec3 neighborPosition = texture(uLastPositions, neighborTexCoord).xyz +
                vec3(neighborOffset, 0.0);
            float neighborStretch = length(neighborPosition - thisPosition);
            float minStretch = length(neighborOffset) * (1.0 - uMaxStretch);
            float maxStretch = length(neighborOffset) * (1.0 + uMaxStretch);

            float fixupDistance = 1.0;
            if (neighborStretch > maxStretch) {
                fixupDistance = maxStretch;
                newPosition = mix(thisPosition, neighborPosition, 0.5) +
                    vec3(fixupDistance * 0.5) * normalize(thisPosition - neighborPosition);
            } else if (neighborStretch < minStretch) {
                fixupDistance = minStretch;
                newPosition = mix(thisPosition, neighborPosition, 0.5) +
                    vec3(fixupDistance * 0.5) * normalize(thisPosition - neighborPosition);
            }
        }
    }

    cFragColor = vec4(newPosition, 0.0);
}
