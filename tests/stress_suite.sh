#!/usr/bin/env bash
set -euo pipefail

echo "=========================================================="
echo "         SHAE MULTI-MODULE END-TO-END STRESS SUITE        "
echo "=========================================================="

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
WORK_DIR=$(mktemp -d -t shae-stress-XXXXXX)

cleanup() {
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
elif [ -d "$HOME/.cargo/bin" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi

echo "--> Building Shae release binary..."
cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml"
BIN="$REPO_ROOT/target/release/shae"

echo "--> Generating multi-module test application in $WORK_DIR..."

# 1. models.shae
cat << 'EOF' > "$WORK_DIR/models.shae"
struct User {
    id,
    name,
    role,
    score
}

struct ApiResponse {
    status,
    data
}

fn create_user(id, name, role, score) {
    User {
        id: id,
        name: name,
        role: role,
        score: score
    }
}
EOF

# 2. crypto_service.shae
cat << 'EOF' > "$WORK_DIR/crypto_service.shae"
use { sha256, hmac_sha256 } from "std:crypto"
use { base64_encode, base64_decode, hex_encode, hex_decode, url_encode, url_decode } from "std:codec"

fn hash_password(password, salt) {
    let combined = password + ":" + salt
    sha256(combined)
}

fn sign_token(payload, secret) {
    let b64 = base64_encode(payload)
    let sig = hmac_sha256(secret, b64)
    b64 + "." + sig
}

fn verify_token(token, secret) {
    let parts = token.split(".")
    if len(parts) != 2 {
        return false
    }
    let expected_sig = hmac_sha256(secret, parts[0])
    parts[1] == expected_sig
}
EOF

# 3. worker_pool.shae
cat << 'EOF' > "$WORK_DIR/worker_pool.shae"
fn run_concurrency_stress(num_tasks) {
    let ch = channel(num_tasks)
    let tasks = []

    let i = 0
    while i < num_tasks {
        let task_id = i
        let t = spawn(fn() {
            let res = task_id * task_id + 1
            send(ch, { "task_id": task_id, "result": res })
        })
        tasks.push(t)
        i = i + 1
    }

    let results = []
    let j = 0
    while j < num_tasks {
        let msg = recv(ch)
        results.push(msg)
        j = j + 1
    }

    for task in tasks {
        join(task)
    }

    results
}
EOF

# 4. main.shae
cat << 'EOF' > "$WORK_DIR/main.shae"
use { create_user } from "./models.shae"
use { hash_password, sign_token, verify_token } from "./crypto_service.shae"
use { run_concurrency_stress } from "./worker_pool.shae"
use { is_match, replace_all } from "std:regex"
use { now, sleep } from "std:time"
use { sha256 } from "std:crypto"
use { base64_encode, base64_decode, url_encode, url_decode } from "std:codec"

print("[STRESS] 1. Testing Structs & Methods...")
let u1 = create_user(101, "Alice", "Admin", 95)
let u2 = create_user(102, "Bob", "Developer", 82)
assert_eq(u1.name, "Alice")
assert_eq(u2.role, "Developer")

print("[STRESS] 2. Testing Pattern Matching & Guards...")
fn categorize(u) {
    match u.score {
        90..101 if u.role == "Admin" => "SuperAdmin",
        90..101 => "TopPerformer",
        80..90 => "SolidPerformer",
        _ => "NeedsImprovement"
    }
}
assert_eq(categorize(u1), "SuperAdmin")
assert_eq(categorize(u2), "SolidPerformer")

print("[STRESS] 3. Testing Crypto & Codecs...")
let pwd_hash = hash_password("secret123", "random_salt")
assert(len(pwd_hash) == 64)

let token = sign_token("user:101:session", "super_secret_key")
assert(verify_token(token, "super_secret_key"))
assert(verify_token(token, "wrong_key") == false)

let raw_url = "https://shae-lang.org/search?q=hello world & safe = 100%"
let enc_url = url_encode(raw_url)
let dec_url = url_decode(enc_url)
assert_eq(dec_url, raw_url)

print("[STRESS] 4. Testing Regex...")
assert(is_match(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$", "alice.dev@shae.internal"))
assert(is_match(r"^\d{4}-\d{2}-\d{2}$", "2026-10-11"))
let cleaned = replace_all(r"\s+", " ", "multiple   spaces   collapsed")
assert_eq(cleaned, "multiple spaces collapsed")

print("[STRESS] 5. Testing Higher-Order Array & Map Methods...")
let nums = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
let doubled_evens = nums
    .filter(fn(x) { x % 2 == 0 })
    .map(fn(x) { x * 2 })
assert_eq(doubled_evens, [4, 8, 12, 16, 20])

let sum = nums.reduce(fn(acc, x) { acc + x }, 0)
assert_eq(sum, 55)

print("[STRESS] 6. Testing Try/Catch Error Recovery...")
let caught = false
try {
    let invalid = 10 / 0
} catch (e) {
    caught = true
}
assert(caught)

print("[STRESS] 7. Testing Concurrency (Spawn, Channels, Join)...")
let worker_results = run_concurrency_stress(20)
assert_eq(len(worker_results), 20)

print("[STRESS] 8. Testing Garbage Collection under Churn...")
let k = 0
while k < 500 {
    let dummy = [k, { "iter": k, "data": "churn_test" }]
    k = k + 1
}
gc()

print("ALL STRESS TESTS PASSED SUCCESSFULLY!")
EOF

echo "--> Mode 1: Running on Default Bytecode VM..."
"$BIN" "$WORK_DIR/main.shae"

echo "--> Mode 2: Running on AST Engine (--engine=ast)..."
"$BIN" "$WORK_DIR/main.shae" --engine=ast

echo "--> Mode 3: Precompiling to .shaec Bytecode & Executing..."
"$BIN" compile "$WORK_DIR/main.shae" -o "$WORK_DIR/main.shaec"
"$BIN" "$WORK_DIR/main.shaec"

echo "--> Mode 4: Standalone Bundle (Default: Bytecode + Fallback)..."
"$BIN" bundle "$WORK_DIR/main.shae" -o "$WORK_DIR/app_default.bin"
"$WORK_DIR/app_default.bin"

echo "--> Mode 5: Standalone Bundle (Stripped: Bytecode Only)..."
"$BIN" bundle "$WORK_DIR/main.shae" -o "$WORK_DIR/app_stripped.bin" --strip
"$WORK_DIR/app_stripped.bin"

echo "--> Mode 6: Standalone Bundle (AST Source Only)..."
"$BIN" bundle "$WORK_DIR/main.shae" -o "$WORK_DIR/app_nobc.bin" --no-bytecode
"$WORK_DIR/app_nobc.bin"

echo "=========================================================="
echo "     ALL 6 STRESS MODES COMPLETED AND PASSED 100%!        "
echo "=========================================================="
