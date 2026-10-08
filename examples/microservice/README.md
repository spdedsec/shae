# 🚀 Shae Showcase Microservice

A production-style Authentication & User Management microservice written in **Shae**, demonstrating all 10 roadmap capabilities functioning together in harmony:

- 📦 **Project System**: Configured with [`shae.toml`](shae.toml) (`shae pkg`).
- 🌐 **Native HTTP Engine**: Runs with `serve(8080, handle_request)` and parameterized route matching (`route_match("/api/users/:id", path)`).
- 🧩 **Regex Validation**: Uses `std:regex` (`is_match`) for email and username constraints.
- 🔒 **Cryptographic Security**: Uses `std:crypto` (`sha256`, `hmac_sha256`, `uuid_v4`) for password hashing and HMAC auth token generation.
- 🔤 **URL-Safe Encoding**: Uses `std:codec` (`b64_url_encode`, `b64_url_decode`) for JWT-style payload encoding.
- ⚡ **Asynchronous Concurrency**: Backed by thread-safe message channels (`channel(50)`) and background workers (`spawn()`) for audit logging.
- 📦 **Single-Binary Distribution**: Ready to bundle into a zero-dependency executable via `shae bundle`.

---

## 📁 Architecture

```
examples/microservice/
├── shae.toml            # Project manifest
├── src/
│   ├── models.shae      # User struct & Regex validation rules
│   ├── auth.shae        # Password hashing & HMAC token signing
│   ├── worker.shae      # Channel-based async background audit logger
│   └── main.shae        # HTTP router, endpoints & server entrypoint
├── test_service.shae    # Automated self-testing verification suite
└── README.md            # Service documentation
```

---

## 🏃 Running the Service

### 1. Run in Development Mode
```bash
shae run src/main.shae
```

### 2. Run Automated Self-Tests
```bash
shae run test_service.shae
```

### 3. Compile to a Zero-Dependency Standalone Binary
```bash
shae bundle src/main.shae -o auth_service
./auth_service
```

---

## 📡 API Endpoints

### 1. Health Check
* **Endpoint**: `GET /api/health`
* **Response**:
```json
{
  "status": "healthy",
  "engine": "Shae Language Runtime",
  "os": "linux",
  "arch": "x86_64",
  "timestamp": 1728400000000
}
```

### 2. Register New User
* **Endpoint**: `POST /api/register`
* **Body**:
```json
{
  "username": "alice_dev",
  "email": "alice@shae.dev",
  "password": "SecretPassword123!"
}
```
* **Response (201 Created)**:
```json
{
  "message": "User registered successfully",
  "user_id": "1",
  "token": "<BASE64_URL_PAYLOAD>.<HMAC_SHA256_SIGNATURE>"
}
```

### 3. Get User Profile (Protected)
* **Endpoint**: `GET /api/users/:id`
* **Headers**: `Authorization: <TOKEN>`
* **Response (200 OK)**:
```json
{
  "id": "1",
  "username": "alice_dev",
  "email": "alice@shae.dev"
}
```
