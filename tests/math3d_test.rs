use shae::run_vm;

#[test]
fn test_math3d_vector_operations_in_shae() {
    let script = r#"
use { vec3, vec3_add, vec3_sub, vec3_dot, vec3_cross, vec3_length, vec3_normalize, vec3_distance, vec3_lerp } from "std:math3d"

let v1 = vec3(1.0, 2.0, 3.0)
let v2 = vec3(4.0, 5.0, 6.0)

let v_add = vec3_add(v1, v2)
assert(v_add.x == 5.0 and v_add.y == 7.0 and v_add.z == 9.0, "vec3_add failed")

let v_sub = vec3_sub(v2, v1)
assert(v_sub.x == 3.0 and v_sub.y == 3.0 and v_sub.z == 3.0, "vec3_sub failed")

let dot = vec3_dot(v1, v2)
assert(dot == 32.0, "vec3_dot failed")

let cross = vec3_cross(v1, v2)
assert(cross.x == -3.0 and cross.y == 6.0 and cross.z == -3.0, "vec3_cross failed")

let len = vec3_length(vec3(3.0, 4.0, 0.0))
assert(len == 5.0, "vec3_length failed")

let norm = vec3_normalize(vec3(0.0, 5.0, 0.0))
assert(norm.x == 0.0 and norm.y == 1.0 and norm.z == 0.0, "vec3_normalize failed")

let dist = vec3_distance(vec3(0.0, 0.0, 0.0), vec3(0.0, 10.0, 0.0))
assert(dist == 10.0, "vec3_distance failed")

let lerped = vec3_lerp(vec3(0.0, 0.0, 0.0), vec3(10.0, 20.0, 30.0), 0.5)
assert(lerped.x == 5.0 and lerped.y == 10.0 and lerped.z == 15.0, "vec3_lerp failed")
"#;

    let res = run_vm(script);
    assert!(res.is_ok(), "math3d vector script failed: {:?}", res.err());
}

#[test]
fn test_math3d_matrix_transforms_in_shae() {
    let script = r#"
use {
    vec3, mat4_identity, mat4_translation, mat4_scaling, mat4_rotation_y,
    mat4_perspective, mat4_look_at, mat4_mul, mat4_transform_point, mat4_inverse
} from "std:math3d"
use { abs } from "std:math"

let p = vec3(1.0, 2.0, 3.0)
let trans = mat4_translation(10.0, 20.0, 30.0)
let p_trans = mat4_transform_point(trans, p)

assert(p_trans.x == 11.0 and p_trans.y == 22.0 and p_trans.z == 33.0, "mat4_transform_point failed")

let inv_trans = mat4_inverse(trans)
let p_restored = mat4_transform_point(inv_trans, p_trans)
let dx = abs(p_restored.x - p.x)
let dy = abs(p_restored.y - p.y)
let dz = abs(p_restored.z - p.z)
assert(dx < 0.0001 and dy < 0.0001 and dz < 0.0001, "mat4_inverse failed")

// View and Projection matrices
let eye = vec3(0.0, 5.0, 10.0)
let target = vec3(0.0, 0.0, 0.0)
let up = vec3(0.0, 1.0, 0.0)
let view = mat4_look_at(eye, target, up)
let proj = mat4_perspective(1.047197, 1.7777, 0.1, 100.0)
let view_proj = mat4_mul(proj, view)
assert(view_proj.len == 16, "mat4_mul should return 16-element array")
"#;

    let res = run_vm(script);
    assert!(res.is_ok(), "math3d matrix script failed: {:?}", res.err());
}

#[test]
fn test_math3d_quaternions_and_rays_in_shae() {
    let script = r#"
use {
    vec3, quat_from_axis_angle, quat_slerp, quat_to_mat4,
    ray, ray_intersect_triangle, ray_intersect_aabb, mat4_transform_point
} from "std:math3d"
use { abs } from "std:math"

// 1. Quaternions
let y_axis = vec3(0.0, 1.0, 0.0)
let q = quat_from_axis_angle(y_axis, 1.57079632679) // 90 deg around Y
let rot_mat = quat_to_mat4(q)
let fwd = vec3(0.0, 0.0, -1.0)
let rotated = mat4_transform_point(rot_mat, fwd)

assert(abs(rotated.x - (-1.0)) < 0.0001, "quat rotation X failed")
assert(abs(rotated.y) < 0.0001, "quat rotation Y failed")
assert(abs(rotated.z) < 0.0001, "quat rotation Z failed")

// 2. Ray-Triangle Intersection
let r = ray(vec3(0.0, 0.0, 5.0), vec3(0.0, 0.0, -1.0))
let v0 = vec3(-2.0, -2.0, 0.0)
let v1 = vec3(2.0, -2.0, 0.0)
let v2 = vec3(0.0, 2.0, 0.0)

let hit = ray_intersect_triangle(r, v0, v1, v2)
assert(hit.hit == true, "Ray should hit triangle")
assert(abs(hit.distance - 5.0) < 0.0001, "Hit distance should be 5.0")
assert(abs(hit.point.z) < 0.0001, "Hit point Z should be 0.0")

// 3. Ray-AABB Intersection
let min = vec3(-1.0, -1.0, -1.0)
let max = vec3(1.0, 1.0, 1.0)
let aabb_hit = ray_intersect_aabb(r, min, max)
assert(aabb_hit.hit == true, "Ray should hit AABB")
assert(abs(aabb_hit.distance - 4.0) < 0.0001, "AABB hit distance should be 4.0")
"#;

    let res = run_vm(script);
    assert!(res.is_ok(), "math3d quaternion and ray script failed: {:?}", res.err());
}
