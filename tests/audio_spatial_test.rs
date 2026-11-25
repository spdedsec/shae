use shae::run_vm;

#[test]
fn test_spatial_audio_panning_in_shae() {
    let script = r#"
        use { listener, source, compute_params } from "std:audio:spatial";
        use { vec3 } from "std:math3d";

        let l = listener(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, -1.0), vec3(0.0, 1.0, 0.0));

        let s_right = source(vec3(10.0, 0.0, 0.0), 1.0, 100.0, 1.0);
        let p_right = compute_params(l, s_right);
        assert(p_right.right_gain > p_right.left_gain, "Right source must have higher right gain");

        let s_left = source(vec3(-10.0, 0.0, 0.0), 1.0, 100.0, 1.0);
        let p_left = compute_params(l, s_left);
        assert(p_left.left_gain > p_left.right_gain, "Left source must have higher left gain");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "spatial audio panning script failed: {:?}", res.err());
}

#[test]
fn test_spatial_audio_distance_attenuation_in_shae() {
    let script = r#"
        use { listener, source, compute_params } from "std:audio:spatial";
        use { vec3 } from "std:math3d";

        let l = listener(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, -1.0), vec3(0.0, 1.0, 0.0));

        let s_near = source(vec3(0.0, 0.0, -2.0), 1.0, 100.0, 1.0);
        let s_far = source(vec3(0.0, 0.0, -50.0), 1.0, 100.0, 1.0);

        let p_near = compute_params(l, s_near);
        let p_far = compute_params(l, s_far);

        assert(p_near.attenuation > p_far.attenuation, "Near sound must be louder than far sound");
        assert(p_near.distance < p_far.distance, "Near distance must be smaller than far distance");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "spatial audio distance attenuation script failed: {:?}", res.err());
}

#[test]
fn test_spatial_audio_stereo_tone_synthesis_in_shae() {
    let script = r#"
        use { listener, source, spatialize_tone } from "std:audio:spatial";
        use { vec3 } from "std:math3d";

        let l = listener(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, -1.0), vec3(0.0, 1.0, 0.0));
        let s = source(vec3(15.0, 0.0, -2.0), 1.0, 100.0, 1.0);

        // Synthesize 50ms 440Hz tone in stereo
        let samples = spatialize_tone("sine", 440.0, 50, l, s);
        assert(samples.len > 100, "Should generate interleaved stereo samples");

        // First stereo pair: index 0 is Left, index 1 is Right
        // For a source far to the right, Right channel amplitude must exceed Left channel
        let sum_l = 0.0;
        let sum_r = 0.0;
        let i = 0;
        while i < 100 {
            let left_val = samples[i];
            let right_val = samples[i + 1];
            sum_l = sum_l + left_val * left_val;
            sum_r = sum_r + right_val * right_val;
            i = i + 2;
        }

        assert(sum_r > sum_l, "Right channel power must be higher for right-positioned source");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "spatial tone synthesis script failed: {:?}", res.err());
}
