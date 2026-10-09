use serde_json::json;
use shae::run;

#[test]
fn test_std_crypto_hashes_and_hmac() {
    let script = r#"
use { sha256, sha512, hmac_sha256, hmac_sha512 } from "std:crypto"

let s256 = sha256("hello world")
let s512 = sha512("hello world")
let h256 = hmac_sha256("secret", "hello world")
let h512 = hmac_sha512("secret", "hello world")

{
    "s256": s256,
    "s512": s512,
    "h256": h256,
    "h512_len": h512.len
}
"#;
    let res = run(script).expect("crypto hashes execution failed");
    let map = res.to_json();
    assert_eq!(
        map["s256"],
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
    assert_eq!(
        map["s512"],
        "309ecc489c12d6eb4cc40f50c902f2b4d0ed77ee511a7c7a9bcd3ca86d4cd86f989dd35bc5ff499670da34255b45b0cfd830e81f605dcf7dc5542e93ae9cd76f"
    );
    assert_eq!(
        map["h256"],
        "734cc62f32841568f45715aeb9f4d7891324e6d948e4c6c60c0621cdac48623a"
    );
    assert_eq!(map["h512_len"], 128);
}

#[test]
fn test_std_crypto_random_and_uuid() {
    let script = r#"
let crypto = use "std:crypto"

let bytes = crypto.random_bytes(16)
let hex = crypto.random_hex(16)
let u = crypto.uuid()

{
    "bytes_len": bytes.len,
    "first_is_num": bytes[0] >= 0 and bytes[0] <= 255,
    "hex_len": hex.len,
    "uuid_len": u.len,
    "uuid_version": u.slice(14, 15)
}
"#;
    let res = run(script).expect("crypto random execution failed");
    assert_eq!(
        res.to_json(),
        json!({
            "bytes_len": 16,
            "first_is_num": true,
            "hex_len": 32,
            "uuid_len": 36,
            "uuid_version": "4"
        })
    );
}

#[test]
fn test_std_codec_base64_and_url() {
    let script = r#"
use { base64_encode, base64_decode, base64_url_encode, base64_url_decode, url_encode, url_decode, hex_encode, hex_decode } from "std:codec"

let original = "Hello World! Shae 2026."
let b64 = base64_encode(original)
let decoded_b64 = base64_decode(b64)

let url_orig = "user=alice & admin=true/100%"
let u_enc = url_encode(url_orig)
let u_dec = url_decode(u_enc)

let h_enc = hex_encode("Shae")
let h_dec = hex_decode(h_enc)

{
    "b64": b64,
    "b64_ok": decoded_b64 == original,
    "url_enc": u_enc,
    "url_ok": u_dec == url_orig,
    "hex_enc": h_enc,
    "hex_ok": h_dec == "Shae"
}
"#;
    let res = run(script).expect("codec execution failed");
    let map = res.to_json();
    assert_eq!(map["b64"], "SGVsbG8gV29ybGQhIFNoYWUgMjAyNi4=");
    assert_eq!(map["b64_ok"], true);
    assert_eq!(map["url_ok"], true);
    assert_eq!(map["hex_enc"], "53686165");
    assert_eq!(map["hex_ok"], true);
}

#[test]
fn test_std_regex_matching_and_finding() {
    let script = r#"
use { is_match, test, find, find_all } from "std:regex"

let m1 = is_match(r"^\d{3}-\d{4}$", "123-4567")
let m2 = test(r"^\d{3}-\d{4}$", "invalid-phone")

let f1 = find(r"\d+", "Price: 199 USD")
let f2 = find(r"\d+", "No numbers here")

let all = find_all(r"[A-Za-z]+", "Shae 4.0 is blazingly fast!")
let words = []
for item in all {
    words.push(item["match"])
}

{
    "m1": m1,
    "m2": m2,
    "f1_match": f1["match"],
    "f1_start": f1["start"],
    "f1_end": f1["end"],
    "f2_null": f2 == null,
    "words": words
}
"#;
    let res = run(script).expect("regex matching execution failed");
    assert_eq!(
        res.to_json(),
        json!({
            "m1": true,
            "m2": false,
            "f1_match": "199",
            "f1_start": 7,
            "f1_end": 10,
            "f2_null": true,
            "words": ["Shae", "is", "blazingly", "fast"]
        })
    );
}

#[test]
fn test_std_regex_replace_split_captures() {
    let script = r#"
use { replace, replace_all, split, captures } from "std:regex"

let r1 = replace(r"\d+", "NUM", "id: 10, count: 20")
let r2 = replace_all(r"\d+", "NUM", "id: 10, count: 20")

let parts = split(r"[,;\s]+", "apple, banana; orange grape")

let caps = captures(r"(\w+)@(\w+\.\w+)", "Contact: admin@shaelang.org today")

{
    "replace_one": r1,
    "replace_all": r2,
    "parts": parts,
    "full_match": caps[0],
    "user": caps[1],
    "domain": caps[2]
}
"#;
    let res = run(script).expect("regex replace execution failed");
    assert_eq!(
        res.to_json(),
        json!({
            "replace_one": "id: NUM, count: 20",
            "replace_all": "id: NUM, count: NUM",
            "parts": ["apple", "banana", "orange", "grape"],
            "full_match": "admin@shaelang.org",
            "user": "admin",
            "domain": "shaelang.org"
        })
    );
}
