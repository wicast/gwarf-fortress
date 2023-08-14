//Const
const PI = 3.1415926;

//Vertex Buffer
struct VertexInput {
    @location(0) position: vec3<f32>,
};
struct UV {
    @location(2) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    
};

@vertex
fn vs_main(
    vert: VertexInput,
    uv: UV,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(vert.position, 1.0);
    out.uv = uv.uv;
    return out;
}

// Fragment Uniform
struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0)
var<uniform> camera: Camera;

struct Light {
    position: vec3<f32>,
    color: vec3<f32>,
}
@group(2) @binding(0)
var<uniform> light: Light;

@group(1) @binding(0)
var s: sampler;
@group(1) @binding(1)
var pos_gb: texture_2d<f32>;
@group(1) @binding(2)
var normal_gb: texture_2d<f32>;
@group(1) @binding(3)
var albedo_gb: texture_2d<f32>;
@group(1) @binding(4)
var s1: sampler;

struct FragOut {
    @location(0) color: vec4<f32>
}

@fragment
fn fs_main(in: VertexOutput) -> FragOut {
    var out: FragOut;

    let albedo_metallic = textureSample(albedo_gb, s1, in.uv);
    let albedo = pow(albedo_metallic.rgb, vec3<f32>(2.2));
    let pos = textureSample(pos_gb, s, in.uv).rgb;
    let normal_rough = textureSample(normal_gb, s, in.uv);
    let normal = normal_rough.rgb;
    let metallic = albedo_metallic.a;
    let roughness = normal_rough.a;


    let N = normal;
    let V = normalize(camera.view_pos.xyz - pos);

    // calculate reflectance at normal incidence; if dia-electric (like plastic) use F0 
    // of 0.04 and if it's a metal, use the albedo color as F0 (metallic workflow)    
    var F0 = vec3(0.04);
    F0 = mix(F0, albedo, metallic);

    // reflectance equation
    var Lo = vec3(0.0);

        {    
        // calculate per-light radiance
        let L = normalize(light.position - pos);
        let H = normalize(V + L);
        let distance = length(light.position - pos);
        let attenuation = 1.0 / (distance * distance);
        let radiance = light.color * attenuation;

        // Cook-Torrance BRDF
        let NDF = DistributionGGX(N, H, roughness);
        let G = GeometrySmith(N, V, L, roughness);
        let F = fresnelSchlick(max(dot(H, V), 0.0), F0);

        let numerator = NDF * G * F;
        let denominator = 4.0 * max(dot(N, V), 0.0) * max(dot(N, L), 0.0) + 0.0001; // + 0.0001 to prevent divide by zero
        let specular = numerator / denominator;
        
        // kS is equal to Fresnel
        let kS = F;
        // for energy conservation, the diffuse and specular light can't
        // be above 1.0 (unless the surface emits light); to preserve this
        // relationship the diffuse component (kD) should equal 1.0 - kS.
        var kD = vec3(1.0) - kS;
        // multiply kD by the inverse metalness such that only non-metals 
        // have diffuse lighting, or a linear blend if partly metal (pure metals
        // have no diffuse light).
        kD *= 1.0 - metallic;

        // scale light by NdotL
        let NdotL = max(dot(N, L), 0.0);        

        // add to outgoing radiance Lo
        Lo += (kD * albedo / PI + specular) * radiance * NdotL;  // note that we already multiplied the BRDF by the Fresnel (kS) so we won't multiply by kS again
    }
    
    // ambient lighting (note that the next IBL tutorial will replace 
    // this ambient lighting with environment lighting).
    let ambient = vec3(0.03) * albedo;//TODO * ao;

    var color = ambient + Lo;

    // HDR tonemapping
    color = color / (color + vec3(1.0));
    // gamma correct
    color = pow(color, vec3(1.0 / 2.2));

    out.color = vec4<f32>(color, 1.0);
    return out;
}

// filament 
fn D_GGX(NoH: f32, a: f32) -> f32 {
    let a2 = a * a;
    let f = (NoH * a2 - NoH) * NoH + 1.0;
    return a2 / (PI * f * f);
}

fn F_Schlick(u: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3(1.0) - f0) * pow(1.0 - u, 5.0);
}


fn V_SmithGGXCorrelatedFast(NoV: f32, NoL: f32, a: f32) -> f32 {
    let a2 = a * a;
    let GGXL = NoV * sqrt((-NoL * a2 + NoL) * NoL + a2);
    let GGXV = NoL * sqrt((-NoV * a2 + NoV) * NoV + a2);
    return 0.5 / (GGXV + GGXL);
}

fn Fd_Lambert() -> f32 {
    return 1.0 / PI;
}

fn BRDF(v: vec3<f32>, l: vec3<f32>, n: vec3<f32>, a: f32, f0: vec3<f32>, perceptualRoughness: f32, diffuseColor: vec3<f32>) {
    let h = normalize(v + l);

    let NoV = abs(dot(n, v)) + 1e-5;
    let NoL = clamp(dot(n, l), 0.0, 1.0);
    let NoH = clamp(dot(n, h), 0.0, 1.0);
    let LoH = clamp(dot(l, h), 0.0, 1.0);

    // perceptually linear roughness to roughness (see parameterization)
    let roughness = perceptualRoughness * perceptualRoughness;

    let D = D_GGX(NoH, a);
    let F = F_Schlick(LoH, f0);
    let V = V_SmithGGXCorrelatedFast(NoV, NoL, roughness);

    // specular BRDF
    let Fr = (D * V) * F;

    // diffuse BRDF
    let Fd = diffuseColor * Fd_Lambert();

    // apply lighting...
}


// Learn OpenGL
fn DistributionGGX(N: vec3<f32>, H: vec3<f32>, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let NdotH = max(dot(N, H), 0.0);
    let NdotH2 = NdotH * NdotH;

    let nom = a2;
    var denom = (NdotH2 * (a2 - 1.0) + 1.0);
    denom = PI * denom * denom;

    return nom / denom;
}

fn GeometrySchlickGGX(NdotV: f32, roughness: f32) -> f32 {
    let r = (roughness + 1.0);
    let k = (r * r) / 8.0;

    let nom = NdotV;
    let denom = NdotV * (1.0 - k) + k;

    return nom / denom;
}
fn GeometrySmith(N: vec3<f32>, V: vec3<f32>, L: vec3<f32>, roughness: f32) -> f32 {
    let NdotV = max(dot(N, V), 0.0);
    let NdotL = max(dot(N, L), 0.0);
    let ggx2 = GeometrySchlickGGX(NdotV, roughness);
    let ggx1 = GeometrySchlickGGX(NdotL, roughness);

    return ggx1 * ggx2;
}

fn fresnelSchlick(cosTheta: f32, F0: vec3<f32>) -> vec3<f32> {
    return F0 + (1.0 - F0) * pow(clamp(1.0 - cosTheta, 0.0, 1.0), 5.0);
}