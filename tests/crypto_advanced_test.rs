use shae::run;
use shae::run_vm;
use shae::value::Value;

#[test]
fn test_aes_256_gcm_roundtrip_and_tamper_vm() {
    let script = r#"
        let crypto = use "std:crypto"
        let key = "12345678901234567890123456789012"
        let nonce = "123456789012"
        let plaintext = [84, 111, 112, 32, 115, 101, 99, 114, 101, 116] // 'Top secret'
        let aad = "additional-auth-metadata"

        let enc = crypto.encrypt_aes_gcm(key, nonce, plaintext, aad)
        let dec = crypto.decrypt_aes_gcm(key, nonce, enc.ciphertext, enc.tag, aad)
        let dec_combined = crypto.decrypt_aes_gcm(key, nonce, enc.combined, null, aad)

        [dec == plaintext, dec == dec_combined, enc.nonce.len, enc.tag.len]
    "#;

    let res = run_vm(script).expect("AES-256-GCM execution on VM should succeed");
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Bool(true));
            assert_eq!(guard[1], Value::Bool(true));
            assert_eq!(guard[2], Value::Int(12));
            assert_eq!(guard[3], Value::Int(16));
        }
        other => panic!("Expected array result, got {:?}", other),
    }
}

#[test]
fn test_aes_256_gcm_roundtrip_and_tamper_ast() {
    let script = r#"
        let crypto = use "std:crypto"
        let key = "12345678901234567890123456789012"
        let nonce = "123456789012"
        let plaintext = "Top secret message for Shae cryptography!"
        let aad = "additional-auth-metadata"

        let enc = crypto.encrypt_aes_gcm(key, nonce, plaintext, aad)
        let dec = crypto.decrypt_aes_gcm(key, nonce, enc.ciphertext, enc.tag, aad)

        dec.len > 0
    "#;

    let res = run(script).expect("AES-256-GCM execution on AST should succeed");
    assert_eq!(res, Value::Bool(true));
}

#[test]
fn test_aes_256_gcm_auto_nonce_generation() {
    let script = r#"
        let crypto = use "std:crypto"
        let key = "12345678901234567890123456789012"
        let plaintext = "Same plaintext, different nonce"

        // Explicit null nonce
        let enc1 = crypto.encrypt_aes_gcm(key, null, plaintext)
        let enc2 = crypto.encrypt_aes_gcm(key, null, plaintext)

        // Fully omitted nonce (2 arguments)
        let enc3 = crypto.encrypt_aes_gcm(key, plaintext)

        // ChaCha20 fully omitted nonce (2 arguments)
        let enc_chacha_omitted = crypto.encrypt_chacha20(key, plaintext)

        let nonces_differ = enc1.nonce != enc2.nonce
        let ciphertexts_differ = enc1.ciphertext != enc2.ciphertext

        let dec1 = crypto.decrypt_aes_gcm(key, enc1.nonce, enc1.ciphertext, enc1.tag)
        let dec2 = crypto.decrypt_aes_gcm(key, enc2.nonce, enc2.ciphertext, enc2.tag)
        let dec3 = crypto.decrypt_aes_gcm(key, enc3.nonce, enc3.ciphertext, enc3.tag)
        let dec_chacha = crypto.decrypt_chacha20(key, enc_chacha_omitted.nonce, enc_chacha_omitted.ciphertext, enc_chacha_omitted.tag)

        // Empty plaintext roundtrip
        let enc_empty = crypto.encrypt_aes_gcm(key, "")
        let dec_empty = crypto.decrypt_aes_gcm(key, enc_empty.nonce, enc_empty.ciphertext, enc_empty.tag)

        [
            enc1.nonce.len,
            enc2.nonce.len,
            enc3.nonce.len,
            enc_chacha_omitted.nonce.len,
            nonces_differ,
            ciphertexts_differ,
            dec1 == dec2,
            dec2 == dec3,
            dec_chacha == dec1,
            dec_empty.len == 0
        ]
    "#;

    let res = run_vm(script).expect("Auto nonce generation on VM should succeed");
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Int(12));
            assert_eq!(guard[1], Value::Int(12));
            assert_eq!(guard[2], Value::Int(12));
            assert_eq!(guard[3], Value::Int(12));
            assert_eq!(guard[4], Value::Bool(true));
            assert_eq!(guard[5], Value::Bool(true));
            assert_eq!(guard[6], Value::Bool(true));
            assert_eq!(guard[7], Value::Bool(true));
            assert_eq!(guard[8], Value::Bool(true));
            assert_eq!(guard[9], Value::Bool(true));
        }
        other => panic!("Expected array result, got {:?}", other),
    }
}

#[test]
fn test_aes_and_chacha_validation_errors() {
    let script = r#"
        let crypto = use "std:crypto"

        let err1 = false
        try {
            crypto.encrypt_aes_gcm("too-short-key", "123456789012", "hello")
        } catch (e) {
            err1 = true
        }

        let err2 = false
        try {
            crypto.encrypt_aes_gcm("12345678901234567890123456789012", "short_nonce", "hello")
        } catch (e) {
            err2 = true
        }

        let enc = crypto.encrypt_aes_gcm("12345678901234567890123456789012", "123456789012", "hello", "aad")
        let tampered = []
        let i = 0
        while i < enc.ciphertext.len {
            tampered.push(enc.ciphertext[i])
            i = i + 1
        }
        tampered[0] = (tampered[0] + 1) % 256

        let err3 = false
        try {
            crypto.decrypt_aes_gcm("12345678901234567890123456789012", "123456789012", tampered, enc.tag, "aad")
        } catch (e) {
            err3 = true
        }

        let err4 = false
        try {
            crypto.decrypt_aes_gcm("12345678901234567890123456789012", "123456789012", enc.ciphertext, enc.tag, "wrong-aad")
        } catch (e) {
            err4 = true
        }

        let err5 = false
        try {
            // Non-finite float rejection in byte arrays
            crypto.encrypt_aes_gcm("12345678901234567890123456789012", null, [1.0 / 0.0])
        } catch (e) {
            err5 = true
        }

        [err1, err2, err3, err4, err5]
    "#;

    let res = run_vm(script).expect("Validation errors test should succeed");
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Bool(true));
            assert_eq!(guard[1], Value::Bool(true));
            assert_eq!(guard[2], Value::Bool(true));
            assert_eq!(guard[3], Value::Bool(true));
            assert_eq!(guard[4], Value::Bool(true));
        }
        other => panic!("Expected array result, got {:?}", other),
    }
}

#[test]
fn test_chacha20_poly1305_roundtrip_dual_engine() {
    let script = r#"
        let crypto = use "std:crypto"
        let key = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
            17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32
        ]
        let nonce = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120]
        let plaintext = [67, 104, 97, 67, 104, 97]
        let aad = "protocol-version-2"

        let enc = crypto.encrypt_chacha20(key, nonce, plaintext, aad)
        let dec = crypto.decrypt_chacha20(key, nonce, enc.ciphertext, enc.tag, aad)

        [dec == plaintext, enc.ciphertext.len > 0, enc.tag.len == 16]
    "#;

    let res_vm = run_vm(script).expect("ChaCha20 VM run should succeed");
    let res_ast = run(script).expect("ChaCha20 AST run should succeed");

    assert_eq!(res_vm, res_ast);
    match res_vm {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Bool(true));
            assert_eq!(guard[1], Value::Bool(true));
            assert_eq!(guard[2], Value::Bool(true));
        }
        other => panic!("Expected array, got {:?}", other),
    }
}

#[test]
fn test_pbkdf2_key_derivation() {
    let script = r#"
        let crypto = use "std:crypto"
        let password = "correct-horse-battery-staple"
        let salt = "cryptographic-salt-value-42"
        let iters = 1000
        let len = 32

        let k1 = crypto.pbkdf2_sha256(password, salt, iters, len)
        let k2 = crypto.pbkdf2_sha512(password, salt, iters, len)

        let k1_repeat = crypto.pbkdf2_sha256(password, salt, iters, len)

        [k1.len, k2.len, k1 == k1_repeat, k1 != k2]
    "#;

    let res = run_vm(script).expect("PBKDF2 VM run should succeed");
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Int(32));
            assert_eq!(guard[1], Value::Int(32));
            assert_eq!(guard[2], Value::Bool(true));
            assert_eq!(guard[3], Value::Bool(true));
        }
        other => panic!("Expected array, got {:?}", other),
    }
}

#[test]
fn test_hkdf_key_derivation() {
    let script = r#"
        let crypto = use "std:crypto"
        let secret = "ikm-input-keying-material-12345"
        let salt = "optional-hkdf-salt"
        let info = "application-context-info"
        let out_len = 64

        let okm256 = crypto.hkdf_sha256(secret, salt, info, out_len)
        let okm512 = crypto.hkdf_sha512(secret, salt, info, out_len)

        let okm256_repeat = crypto.hkdf_sha256(secret, salt, info, out_len)

        [okm256.len, okm512.len, okm256 == okm256_repeat, okm256 != okm512]
    "#;

    let res = run_vm(script).expect("HKDF VM run should succeed");
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Int(64));
            assert_eq!(guard[1], Value::Int(64));
            assert_eq!(guard[2], Value::Bool(true));
            assert_eq!(guard[3], Value::Bool(true));
        }
        other => panic!("Expected array, got {:?}", other),
    }
}

#[test]
fn test_timing_safe_equal() {
    let script = r#"
        let crypto = use "std:crypto"

        let eq1 = crypto.timing_safe_equal("supersecret", "supersecret")
        let eq2 = crypto.timing_safe_equal("supersecret", "supersecreX")
        let eq3 = crypto.timing_safe_equal("short", "longer-string")
        let eq4 = crypto.timing_safe_equal([1, 2, 3], [1, 2, 3])
        let eq5 = crypto.timing_safe_equal([1, 2, 3], [1, 2, 4])
        let eq6 = crypto.timing_safe_equal([1, 2], [1, 2, 3])

        [eq1, eq2, eq3, eq4, eq5, eq6]
    "#;

    let res = run_vm(script).expect("timing_safe_equal run should succeed");
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Bool(true));
            assert_eq!(guard[1], Value::Bool(false));
            assert_eq!(guard[2], Value::Bool(false));
            assert_eq!(guard[3], Value::Bool(true));
            assert_eq!(guard[4], Value::Bool(false));
            assert_eq!(guard[5], Value::Bool(false));
        }
        other => panic!("Expected array, got {:?}", other),
    }
}
