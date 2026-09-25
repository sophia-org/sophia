use sophia_renderer_native_egl::{NativeCompositionSampling, native_composition_sampling};
use sophia_x_authority::x_fixed_glyph_rows;

#[path = "support/reference_sampling.rs"]
mod reference_sampling;
use reference_sampling::*;

/// The GLSL the renderer compiles, read as GLSL rather than through the Rust
/// that binds it. `tools/check_shaders.sh` compiles these same files.
const RECONSTRUCTION_FRAGMENT: &str = include_str!("../src/gl/shaders/sharp_reconstruction.frag");
const DIRECT_FRAGMENT: &str = include_str!("../src/gl/shaders/composition.frag");

#[test]
fn composition_sampling_preserves_exact_pixels_and_reconstructs_reductions() {
    assert_eq!(
        native_composition_sampling((2560, 1440), (2560, 1440)),
        NativeCompositionSampling::ExactNearest,
    );
    assert_eq!(
        native_composition_sampling((2560, 1440), (1920, 1080)),
        NativeCompositionSampling::SharpDownscale,
    );
    assert_eq!(
        native_composition_sampling((1920, 1200), (1920, 1080)),
        NativeCompositionSampling::SharpDownscale,
    );
    assert_eq!(
        native_composition_sampling((1280, 720), (2560, 1440)),
        NativeCompositionSampling::SharpUpscale,
    );
    // The physical rig produces this one: a 1280x1440 raster drawn at 1920x1080
    // enlarges in x while reducing in y. Keep that distinct in telemetry so
    // neither single-axis label overclaims what was rendered.
    assert_eq!(
        native_composition_sampling((1280, 1440), (1920, 1080)),
        NativeCompositionSampling::SharpMixed,
    );
}

#[test]
fn sampling_names_are_stable_evidence_values() {
    assert_eq!(
        NativeCompositionSampling::ExactNearest.reduced_name(),
        "exact_nearest"
    );
    assert_eq!(
        NativeCompositionSampling::SharpDownscale.reduced_name(),
        "sharp_downscale"
    );
    assert_eq!(
        NativeCompositionSampling::SharpUpscale.reduced_name(),
        "sharp_upscale"
    );
    assert_eq!(
        NativeCompositionSampling::SharpMixed.reduced_name(),
        "sharp_mixed"
    );
    assert_eq!(
        NativeCompositionSampling::LinearFallback.reduced_name(),
        "linear_fallback"
    );
}

/// Nothing about a rectangle can ask for the degraded path.
///
/// `LinearFallback` is a fact about whether a shader compiled, and the geometry
/// classifier deliberately cannot see that. Keeping it unreachable from here is
/// what stops a run reporting a fallback it never took, and what makes any
/// `linear_fallback` in the evidence mean the shader is genuinely missing.
#[test]
fn geometry_never_requests_the_degraded_path() {
    for source in [(1920u32, 1080u32), (2560, 1440), (1280, 1440), (640, 480)] {
        for target in [(1920u32, 1080u32), (2560, 1440), (1280, 1440), (640, 480)] {
            assert_ne!(
                native_composition_sampling(source, target),
                NativeCompositionSampling::LinearFallback,
                "{source:?} -> {target:?}"
            );
        }
    }
    assert!(NativeCompositionSampling::SharpDownscale.is_reconstructed());
    assert!(NativeCompositionSampling::SharpUpscale.is_reconstructed());
    assert!(NativeCompositionSampling::SharpMixed.is_reconstructed());
    assert!(!NativeCompositionSampling::ExactNearest.is_reconstructed());
    assert!(!NativeCompositionSampling::LinearFallback.is_reconstructed());
}

#[test]
fn sharp_reconstruction_preserves_mixed_case_bitmap_strokes_at_three_quarters_scale() {
    let width = 24usize;
    let height = 13usize;
    let mut source = vec![0.0f32; width * height];
    for (glyph, byte) in [b'A', b'a', b'Z', b'z'].into_iter().enumerate() {
        for (row, bits) in x_fixed_glyph_rows(byte).into_iter().enumerate() {
            for column in 0..6 {
                if bits & (1 << (5 - column)) != 0 {
                    source[row * width + glyph * 6 + column] = 1.0;
                }
            }
        }
    }

    let sharp = resample(&source, (width, height), (18, 10), Sample::Sharp);
    let linear = resample(&source, (width, height), (18, 10), Sample::Linear);
    let nearest = resample(&source, (width, height), (18, 10), Sample::Nearest);

    assert!(sharp.iter().any(|value| *value > 0.0 && *value < 1.0));
    assert!(sharp.iter().any(|value| *value > 0.85));
    assert_ne!(quantized(&sharp), quantized(&linear));
    assert_ne!(quantized(&sharp), quantized(&nearest));
    for range in [0..4, 4..9, 9..13, 13..18] {
        assert!(
            (0..10).any(|row| range.clone().any(|column| sharp[row * 18 + column] > 0.2)),
            "every mixed-case glyph retains visible coverage"
        );
    }

    let opaque_xrgb = sharp
        .iter()
        .map(|value| finish_sample([*value, *value, *value, 0.0], AlphaMode::Opaque, 1.0))
        .collect::<Vec<_>>();
    assert!(opaque_xrgb.iter().any(|pixel| pixel[0] > 0.85));
    assert!(opaque_xrgb.iter().all(|pixel| pixel[3] == 1.0));

    let bindings = include_str!("../src/gl/shaders.rs");
    let renderer = include_str!("../src/gl.rs");
    assert!(bindings.contains("SHARP_RECONSTRUCTION_FRAGMENT_SHADER"));
    assert!(RECONSTRUCTION_FRAGMENT.contains("for (int row = -1; row <= 2; row++)"));
    assert!(RECONSTRUCTION_FRAGMENT.contains("for (int column = -1; column <= 2; column++)"));
    // Asserted of each program rather than counted across a file holding both.
    // The count was only ever standing in for this, and it would have gone on
    // passing if one shader grew the uniform twice and the other lost it.
    assert!(RECONSTRUCTION_FRAGMENT.contains("uniform float source_is_opaque;"));
    assert!(DIRECT_FRAGMENT.contains("uniform float source_is_opaque;"));
    assert!(renderer.contains("get_uniform_location(program, \"source_is_opaque\")"));
}

/// The correction, reproduced end to end without a GPU.
///
/// Downscaling white-on-black text is the case that started this: averaging the
/// encoded bytes 0 and 255 gives 127, which is roughly a fifth of the light of
/// white rather than half of it, so every edge lands too dark. Filtering the
/// same glyphs in light and re-encoding puts them where they belong, and the
/// measurable consequence is that the anti-aliased population gets brighter.
///
/// Both paths run the identical kernel on identical input. The only difference
/// is the space the weights are applied in, which is the whole of the change.
#[test]
fn filtering_in_light_brightens_the_edges_gamma_space_filtering_darkens() {
    let width = 24usize;
    let height = 13usize;
    let source = glyph_bitmap(width, height);

    let gamma_space = resample(&source, (width, height), (18, 10), Sample::Sharp);
    let linear_light = resample_in_light(&source, (width, height), (18, 10));

    let edges_of = |values: &[f32]| -> Vec<f32> {
        values
            .iter()
            .copied()
            .filter(|value| *value > 0.001 && *value < 0.999)
            .collect()
    };
    let mean = |values: &[f32]| values.iter().sum::<f32>() / values.len() as f32;

    let gamma_edges = edges_of(&gamma_space);
    let light_edges = edges_of(&linear_light);
    assert!(
        !gamma_edges.is_empty() && !light_edges.is_empty(),
        "the fixture must actually produce partial coverage to compare"
    );

    let gamma_mean = mean(&gamma_edges);
    let light_mean = mean(&light_edges);
    assert!(
        light_mean > gamma_mean,
        "filtering in light must raise the edge population: gamma {gamma_mean} vs light {light_mean}"
    );

    // Not merely different -- different by enough to see. Below a few percent
    // this test would pass on floating-point noise and prove nothing.
    let lift = (light_mean - gamma_mean) / gamma_mean;
    assert!(
        lift > 0.05,
        "the lift must be a real correction, not rounding: {lift}"
    );

    // The arithmetic above is a mirror of the shader, so on its own it would
    // still pass with the shader reverted to weighting encoded bytes. These pin
    // the shader to the same decode: the tap is decoded where it is gathered,
    // and the sum is re-encoded once at the end rather than per tap.
    assert!(
        RECONSTRUCTION_FRAGMENT
            .contains("sum += to_light(texture2D(frame, coordinate), source_is_opaque) * weight;"),
        "every tap is decoded before it is weighted"
    );
    assert!(
        RECONSTRUCTION_FRAGMENT.contains("return vec4(encoded.rgb * encoded.rgb, encoded.a);"),
        "the opaque decode squares colour and leaves alpha alone"
    );
    assert!(
        RECONSTRUCTION_FRAGMENT
            .contains("return vec4(encoded.rgb * encoded.rgb / encoded.a, encoded.a);"),
        "the premultiplied decode divides out coverage and leaves alpha alone"
    );
    assert!(
        RECONSTRUCTION_FRAGMENT.contains("return sqrt(safe * alpha);"),
        "the premultiplied encode re-applies coverage under the root"
    );

    // Only genuinely saturated pixels are untouched -- those whose whole 4x4
    // footprint agreed, where the weights sum to one over a single value and the
    // round trip is exact. The band has to be tight: a pixel at 0.0001 is not
    // black but a faint ring, and it lifts to 0.01 under the square root, which
    // is the correction doing its job rather than a saturated pixel drifting.
    for (encoded, light) in gamma_space.iter().zip(linear_light.iter()) {
        if *encoded <= 1e-6 || *encoded >= 1.0 - 1e-6 {
            assert!(
                (encoded - light).abs() < 1e-5,
                "a saturated pixel moved: {encoded} vs {light}"
            );
        }
    }
}

/// Alpha is coverage, not light, and squaring it is the invisible way to get
/// this wrong: the image still looks plausible while every partially
/// transparent edge is wrong by the same factor the colour was corrected by.
#[test]
fn the_transfer_round_trip_leaves_alpha_untouched_and_survives_zero_coverage() {
    for alpha in [1.0f32, 0.75, 0.5, 0.25, 0.004] {
        for encoded in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let premultiplied = encoded * alpha;
            let light = to_light_premultiplied(premultiplied, alpha);
            let round_tripped = to_bytes_premultiplied(light, alpha);
            assert!(
                (round_tripped - premultiplied).abs() < 0.002,
                "premultiplied {premultiplied} at alpha {alpha} came back as {round_tripped}"
            );
        }
    }

    // Zero coverage carries no colour to decode, and the division that would
    // recover one is the division this must not perform.
    assert_eq!(to_light_premultiplied(0.0, 0.0), 0.0);
    assert_eq!(to_bytes_premultiplied(0.0, 0.0), 0.0);
    assert!(to_light_premultiplied(0.5, 0.0).is_finite());
    assert!(to_bytes_premultiplied(0.5, 0.0).is_finite());
}

/// Catmull-Rom rings below zero on a hard edge, and the square root of a
/// negative is not a number. A NaN fragment reaches the screen as a hole rather
/// than as a dark pixel, so the clamp has to happen before the encode and not
/// after it.
#[test]
fn reconstruction_ringing_encodes_to_black_rather_than_to_not_a_number() {
    for ringing in [-0.5f32, -0.05, -0.0001] {
        let encoded = to_bytes_premultiplied(ringing, 1.0);
        assert!(
            encoded.is_finite() && encoded >= 0.0,
            "ringing {ringing} encoded to {encoded}"
        );
    }

    let clamp_position = RECONSTRUCTION_FRAGMENT
        .find("float alpha = clamp(mixed.a, 0.0, 1.0);")
        .expect("the mixed alpha is clamped before it is used to re-encode");
    let encode_position = RECONSTRUCTION_FRAGMENT
        .find("clamp(to_bytes(mixed.rgb, alpha, source_is_opaque), 0.0, 1.0)")
        .expect("the encode is applied to the clamped value");
    assert!(
        clamp_position < encode_position,
        "the clamp must precede the encode, or sqrt sees a negative"
    );
    assert!(
        RECONSTRUCTION_FRAGMENT.contains("vec3 safe = max(light, vec3(0.0));"),
        "the encode guards its own input as well, since it is reachable directly"
    );
}

/// The filter and the texture sampler are one decision.
///
/// A reconstructed draw gathers its own 4x4 footprint at texel centres, so it
/// needs the texels as stored. Leaving `GL_LINEAR` on would blend them in
/// gamma-encoded space before the shader ever ran, undoing the correction
/// invisibly -- the evidence would still say `sharp_downscale status=active`.
#[test]
fn reconstructed_draws_sample_unfiltered_texels() {
    let renderer = include_str!("../src/gl.rs");
    let program = include_str!("../src/gl/sampling_program.rs");

    // The filter is no longer derived at the call site from the requested
    // sampling alone; both call sites ask the same resolution the program comes
    // from.
    assert_eq!(
        renderer
            .matches("self.draw_plan(sampling).texture_filter")
            .count(),
        2,
        "both the CPU and texture layer paths take the filter from the draw plan"
    );
    assert!(
        !renderer.contains("fn texture_filter("),
        "the standalone filter policy is gone, so it cannot drift from the program choice"
    );
    assert!(
        program.contains("texture_filter: glow::NEAREST"),
        "reconstruction and exact draws both sample unfiltered"
    );
    // LINEAR survives only where no shader is running.
    assert_eq!(
        program.matches("texture_filter: glow::LINEAR").count(),
        2,
        "only the two fallback arms keep hardware filtering"
    );
}

#[test]
fn opaque_xrgb_ignores_the_unused_alpha_byte_before_applying_opacity() {
    assert_eq!(
        finish_sample([1.0, 0.5, 0.25, 0.0], AlphaMode::Opaque, 1.0),
        [1.0, 0.5, 0.25, 1.0],
    );
    assert_eq!(
        finish_sample([1.0, 0.5, 0.25, 0.0], AlphaMode::Opaque, 0.5),
        [0.5, 0.25, 0.125, 0.5],
    );
}

#[test]
fn premultiplied_argb_clamps_reconstruction_ringing_to_alpha() {
    assert_eq!(
        finish_sample([0.8, 0.4, 0.2, 0.5], AlphaMode::Premultiplied, 0.5,),
        [0.25, 0.2, 0.1, 0.25],
    );
    assert_eq!(
        finish_sample([0.7, 0.2, 0.1, 0.0], AlphaMode::Premultiplied, 1.0,),
        [0.0, 0.0, 0.0, 0.0],
    );
}

/// The four mixed-case glyphs the reconstruction fixtures are built from.
fn glyph_bitmap(width: usize, height: usize) -> Vec<f32> {
    let mut source = vec![0.0f32; width * height];
    for (glyph, byte) in [b'A', b'a', b'Z', b'z'].into_iter().enumerate() {
        for (row, bits) in x_fixed_glyph_rows(byte).into_iter().enumerate() {
            for column in 0..6 {
                if bits & (1 << (5 - column)) != 0 {
                    source[row * width + glyph * 6 + column] = 1.0;
                }
            }
        }
    }
    source
}
