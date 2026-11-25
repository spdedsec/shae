use shae::run_vm;

#[test]
fn test_tensor_creation_and_algebra_in_shae() {
    let script = r#"
        use { zeros, ones, arange, add, mul, sum, mean, max } from "std:tensor";

        let z = zeros([3, 3]);
        assert(z.size == 9, "zeros size should be 9");
        assert(sum(z) == 0.0, "sum of zeros should be 0.0");

        let o = ones([2, 4]);
        assert(o.size == 8, "ones size should be 8");
        assert(sum(o) == 8.0, "sum of ones should be 8.0");
        assert(mean(o) == 1.0, "mean of ones should be 1.0");

        let a = arange(0.0, 5.0, 1.0);
        assert(a.size == 5, "arange size should be 5");
        assert(max(a) == 4.0, "max of arange should be 4.0");

        let two_a = add(a, a);
        let squared = mul(a, a);
        assert(sum(two_a) == 20.0, "2*sum(0..4) should be 20.0");
        assert(sum(squared) == 30.0, "sum of squares 0+1+4+9+16 should be 30.0");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "tensor algebra script failed: {:?}", res.err());
}

#[test]
fn test_tensor_matmul_and_dense_layer_in_shae() {
    let script = r#"
        use { tensor, matmul, add, relu, softmax, sum } from "std:tensor";
        use { abs } from "std:math";

        // X: Batch of 2 samples with 3 features: [2, 3]
        let x = tensor([2, 3], [
            1.0, 2.0, 3.0,
            4.0, 5.0, 6.0
        ]);

        // W: Weights projecting 3 features to 2 outputs: [3, 2]
        let w = tensor([3, 2], [
            0.5, -0.5,
            1.0,  0.0,
           -0.5,  0.5
        ]);

        // B: Bias for 2 outputs: [2]
        let b = tensor([2], [0.1, -0.1]);

        // Forward pass: Linear -> Add Bias -> ReLU -> Softmax
        let xw = matmul(x, w);
        assert(xw.shape[0] == 2, "XW rows should be 2");
        assert(xw.shape[1] == 2, "XW cols should be 2");

        let z = add(xw, b);
        let a = relu(z);
        let p = softmax(a, 1);

        // Softmax output shape: [2, 2]
        assert(p.shape[0] == 2, "P rows should be 2");
        assert(p.shape[1] == 2, "P cols should be 2");

        // Row probabilities must each sum to 1.0
        let p_row0_sum = p.data[0] + p.data[1];
        let p_row1_sum = p.data[2] + p.data[3];

        assert(abs(p_row0_sum - 1.0) < 0.0001, "Row 0 probabilities must sum to 1.0");
        assert(abs(p_row1_sum - 1.0) < 0.0001, "Row 1 probabilities must sum to 1.0");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "neural network forward pass script failed: {:?}", res.err());
}

#[test]
fn test_gpu_compute_buffers_and_dispatch_in_shae() {
    let script = r#"
        use { create_buffer, write_buffer, read_buffer, dispatch } from "std:gpu";

        let x_buf = create_buffer(4, [1.0, 2.0, 3.0, 4.0]);
        let y_buf = create_buffer(4, [10.0, 20.0, 30.0, 40.0]);

        // Dispatch SAXPY kernel: Y = 2 * X + Y
        dispatch("saxpy", [4, 1, 1], [x_buf, y_buf]);

        let result = read_buffer(y_buf, 0, 4);
        assert(result[0] == 12.0, "10 + 2*1 = 12");
        assert(result[1] == 24.0, "20 + 2*2 = 24");
        assert(result[2] == 36.0, "30 + 2*3 = 36");
        assert(result[3] == 48.0, "40 + 2*4 = 48");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "gpu buffer dispatch script failed: {:?}", res.err());
}
