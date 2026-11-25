// Every primitive — rects, rounded rects, strokes, circles, lines and glyphs —
// is one instanced quad through one pipeline. Sorting by pipeline is therefore
// a non-issue: there is exactly one draw call for the whole UI per frame.
//
// `kind`: 0 = shape fill, 1 = shape stroke, 2 = line, 3 = glyph.
// `device_space`: 1 means rect/clip are already in physical pixels (glyphs).

struct Globals {
    resolution: vec2f,
    scale: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var glyph_atlas: texture_2d<f32>;
@group(0) @binding(2) var glyph_sampler: sampler;

struct VOut {
    @builtin(position) pos: vec4f,
    @location(0) screen: vec2f,
    @location(1) local: vec2f,
    @location(2) size: vec2f,
    @location(3) color: vec4f,
    @location(4) params: vec4f,
    @location(5) clip: vec4f,
    @location(6) uv: vec2f,
};

fn to_ndc(p: vec2f) -> vec4f {
    return vec4f(
        p.x / globals.resolution.x * 2.0 - 1.0,
        1.0 - p.y / globals.resolution.y * 2.0,
        0.0,
        1.0,
    );
}

@vertex
fn vs(
    @builtin(vertex_index) vertex: u32,
    @location(0) rect: vec4f,
    @location(1) color: vec4f,
    @location(2) params: vec4f,
    @location(3) line: vec4f,
    @location(4) clip: vec4f,
    @location(5) uv: vec4f,
) -> VOut {
    let corners = array<vec2f, 6>(
        vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0),
        vec2f(0.0, 1.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0),
    );
    let c = corners[vertex];
    let device = select(globals.scale, 1.0, params.w > 0.5);

    var out: VOut;
    var p: vec2f;
    out.uv = vec2f(0.0);

    if (params.z >= 3.0) {
        // A glyph: rect and uv are already in device pixels.
        p = rect.xy + c * rect.zw;
        out.uv = mix(uv.xy, uv.zw, c);
        out.local = vec2f(0.0);
        out.size = rect.zw;
    } else if (params.z >= 2.0) {
        // A line is a quad around the segment; the fragment shader computes
        // the distance from the centre line for a soft edge.
        let a = line.xy * globals.scale;
        let b = line.zw * globals.scale;
        let direction = b - a;
        let len = max(length(direction), 0.001);
        let normal = vec2f(-direction.y, direction.x) / len;
        let half = max(params.y * globals.scale * 0.5, 0.5) + 0.5;
        p = mix(a, b, c.x) + normal * ((c.y * 2.0 - 1.0) * half);
        out.local = vec2f(c.x * len, (c.y * 2.0 - 1.0) * half);
        out.size = vec2f(len, half * 2.0);
    } else {
        let size = max(rect.zw * globals.scale, vec2f(0.0));
        p = rect.xy * globals.scale + c * size;
        out.local = c * size;
        out.size = size;
    }

    out.pos = to_ndc(p);
    out.screen = p;
    out.color = color;
    // Radius and stroke are in device pixels after this point.
    out.params = vec4f(
        params.x * device,
        params.y * device,
        params.z,
        params.w,
    );
    out.clip = clip * device;
    return out;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4f {
    if (in.screen.x < in.clip.x || in.screen.y < in.clip.y
        || in.screen.x > in.clip.z || in.screen.y > in.clip.w) {
        discard;
    }

    // Sampled unconditionally: `textureSample` needs uniform control flow on
    // some drivers, and `kind` is a per-instance value. Shape instances carry
    // a harmless uv of (0, 0) and simply ignore this coverage.
    let coverage = textureSample(glyph_atlas, glyph_sampler, in.uv).r;
    var alpha = 1.0;
    if (in.params.z >= 3.0) {
        alpha = coverage;
    } else if (in.params.z >= 2.0) {
        let half = in.size.y * 0.5;
        let distance = abs(in.local.y - half);
        alpha = 1.0 - smoothstep(half - 1.0, half + 1.0, distance);
    } else {
        let radius = clamp(in.params.x, 0.0, min(in.size.x, in.size.y) * 0.5);
        let half = in.size * 0.5;
        let q = abs(in.local - half) - (half - vec2f(radius));
        let distance = length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - radius;
        if (in.params.y > 0.0) {
            alpha = 1.0 - smoothstep(-1.0, 1.0, abs(distance) - in.params.y * 0.5);
        } else {
            alpha = 1.0 - smoothstep(-1.0, 1.0, distance);
        }
    }

    let a = in.color.a * alpha;
    if (a <= 0.003) {
        discard;
    }
    return vec4f(in.color.rgb, a);
}

// The persistent canvas is copied to the swapchain in one full-screen pass.
@group(0) @binding(0) var canvas_texture: texture_2d<f32>;
@group(0) @binding(1) var canvas_sampler: sampler;

@vertex
fn blit_vs(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4f {
    let points = array<vec2f, 3>(
        vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0),
    );
    return vec4f(points[vertex], 0.0, 1.0);
}

@fragment
fn blit_fs(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let dimensions = vec2f(textureDimensions(canvas_texture));
    return textureSample(canvas_texture, canvas_sampler, position.xy / dimensions);
}
