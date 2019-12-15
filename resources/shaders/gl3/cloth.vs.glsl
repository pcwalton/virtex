#version 330

// virtex/resources/shaders/cloth.vs.glsl

precision highp float;

uniform mat4 uTransform;

in vec4 aPosition;

void main() {
    vec4 ndcPosition = uTransform * aPosition;
    gl_Position = ndcPosition;
}
