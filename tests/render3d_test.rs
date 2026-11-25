use shae::run_vm;

#[test]
fn test_render3d_procedural_primitives_and_obj_in_shae() {
    let script = r#"
        use { cube, sphere, plane, cylinder, parse_obj } from "std:gui3d";

        let my_cube = cube(2.0);
        assert(my_cube.vertex_count == 24, "Cube should have 24 vertices");
        assert(my_cube.triangle_count == 12, "Cube should have 12 triangles");

        let my_sphere = sphere(1.0, 8, 16);
        assert(my_sphere.triangle_count == 256, "Sphere should have 256 triangles");

        let my_plane = plane(4.0, 4.0);
        assert(my_plane.vertex_count == 4, "Plane should have 4 vertices");
        assert(my_plane.triangle_count == 2, "Plane should have 2 triangles");

        let my_cyl = cylinder(1.0, 2.0, 10);
        assert(my_cyl.triangle_count == 20, "Cylinder should have 20 triangles");

        let obj_data = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
        let parsed = parse_obj(obj_data);
        assert(parsed.vertex_count == 3, "Parsed OBJ should have 3 vertices");
        assert(parsed.triangle_count == 1, "Parsed OBJ should have 1 triangle");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "render3d primitives script failed: {:?}", res.err());
}

#[test]
fn test_render3d_depth_buffering_and_occlusion_in_shae() {
    let script = r#"
        use { rasterizer, clear, set_pixel, get_pixel, get_depth } from "std:gui3d";

        let r = rasterizer(64, 64);
        clear(r.id, 0, 1000.0);

        // Near red pixel at (30, 30) with depth 10.0
        set_pixel(r.id, 30, 30, 16711680, 10.0);
        assert(get_pixel(r.id, 30, 30) == 16711680, "Pixel should be red");
        assert(get_depth(r.id, 30, 30) == 10.0, "Depth should be 10.0");

        // Occluded green pixel at (30, 30) with depth 50.0 (farther away)
        set_pixel(r.id, 30, 30, 65280, 50.0);
        assert(get_pixel(r.id, 30, 30) == 16711680, "Pixel should stay red due to depth test");
        assert(get_depth(r.id, 30, 30) == 10.0, "Depth should stay 10.0");

        // Closer blue pixel at (30, 30) with depth 2.0 (closer)
        set_pixel(r.id, 30, 30, 255, 2.0);
        assert(get_pixel(r.id, 30, 30) == 255, "Pixel should update to blue");
        assert(get_depth(r.id, 30, 30) == 2.0, "Depth should update to 2.0");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "render3d depth buffer script failed: {:?}", res.err());
}

#[test]
fn test_render3d_full_pipeline_projection_and_mesh_raster() {
    let script = r#"
        use { rasterizer, clear, cube, draw_mesh, get_pixel } from "std:gui3d";
        use { vec3, mat4_look_at, mat4_perspective, mat4_mul, mat4_translation } from "std:math3d";

        let r = rasterizer(80, 80);
        clear(r.id, 0, 1000.0);

        let my_cube = cube(1.0);
        let model = mat4_translation(0.0, 0.0, -3.0);
        let view = mat4_look_at(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, -1.0), vec3(0.0, 1.0, 0.0));
        let proj = mat4_perspective(1.04719, 1.0, 0.1, 100.0);

        let vp = mat4_mul(proj, view);
        let mvp = mat4_mul(vp, model);

        let light = vec3(0.0, 1.0, -1.0);
        draw_mesh(r.id, my_cube, mvp, model, light, 0.2, 16777215);

        // Center pixel (40, 40) should have been rasterized and is no longer black (0)
        let center_color = get_pixel(r.id, 40, 40);
        assert(center_color != 0, "Center pixel of rasterized cube should not be background color");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "render3d full pipeline script failed: {:?}", res.err());
}
