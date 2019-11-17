#version 330

// virtex/resources/shaders/render_simple.fs.glsl

uniform sampler2D uTileCache;
uniform float uOpacity;

in vec2 vTexCoord;

out vec4 cFragColor;

void main() {
    cFragColor = texture(uTileCache, vTexCoord) * uOpacity;
}
