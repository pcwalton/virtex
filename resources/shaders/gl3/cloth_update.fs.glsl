#version 330

// virtex/resources/shaders/cloth_update.fs.glsl

precision highp float;

uniform vec4 uGravity;
uniform float uSpring;
uniform sampler2D uLastPositions;
uniform vec2 uFramebufferSize;

in vec2 vTexCoord;

out vec4 cFragColor;

void accumulate(vec2 offset, vec3 thisPosition, inout vec3 outAccel) {
    vec2 neighborTexCoord = vTexCoord + offset / uFramebufferSize;
    if (all(greaterThan(neighborTexCoord, vec2(0.0))) &&
        all(lessThan(neighborTexCoord, vec2(1.0)))) {
        vec3 neighborPosition = texture(uLastPositions, neighborTexCoord).xyz + vec3(offset, 0.0);
        vec3 vector = neighborPosition - thisPosition;
        float vectorLength = length(vector);
        float force = uSpring * (vectorLength - length(offset));
        outAccel += vec3(force) * -vector / vectorLength;
    }
}

void main() {
    vec2 fragCoord = floor(vTexCoord * uFramebufferSize);
    vec3 thisPosition = texture(uLastPositions, vTexCoord).xyz;

    vec3 accel = vec3(0.0);
    if (fragCoord.y + 1.0 < uFramebufferSize.y ||
        (fragCoord.x > 0.0 && fragCoord.x + 1.0 < uFramebufferSize.x)) {
        accel += uGravity.xyz;
        accumulate(vec2(-1.0, -1.0), thisPosition, accel);
        accumulate(vec2( 0.0, -1.0), thisPosition, accel);
        accumulate(vec2( 1.0, -1.0), thisPosition, accel);
        accumulate(vec2(-1.0,  0.0), thisPosition, accel);
        accumulate(vec2( 1.0,  0.0), thisPosition, accel);
        accumulate(vec2(-1.0,  1.0), thisPosition, accel);
        accumulate(vec2( 0.0,  1.0), thisPosition, accel);
        accumulate(vec2( 1.0,  1.0), thisPosition, accel);
    }

    // Verlet integration
    cFragColor = vec4(thisPosition * vec3(2.0) + accel, 0.0);
}
