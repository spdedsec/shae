# Real-World Microservice Showcase (SQLite & CSV)

This showcase demonstrates a production-grade analytics microservice written in Shae, featuring:

1. **Persistent SQLite Database (`std:sqlite`)**:
   - Zero-dependency dynamic linking to native system SQLite (`libsqlite3.so`, `winsqlite3.dll`, `libsqlite3.dylib`).
   - Transactional DDL schema migration and parameterized CRUD operations.
   - Real-time aggregations (`COUNT`, `AVG(latency_ms)`).

2. **RFC 4180 CSV Engine (`std:csv`)**:
   - High-throughput CSV data export endpoint (`GET /api/export.csv`).
   - Bulk CSV batch ingestion endpoint (`POST /api/import.csv`).
   - Full RFC 4180 compliance with quoted multiline strings, escaped quotes, and type coercion.

3. **HTTP Server (`serve`)**:
   - Built-in multi-threaded asynchronous server handling REST endpoints.

## Running the Service

```bash
shae run examples/realworld_service/main.shae
```

## Running the Automated Test Client

In a separate terminal or background job:
```bash
shae run examples/realworld_service/test_service.shae
```

## API Endpoints

| Method | Path | Description |
|---|---|---|
| `GET` | `/health` | Service health status and live SQLite database statistics |
| `GET` | `/api/metrics` | Retrieve the latest 50 telemetry events as JSON |
| `POST` | `/api/metrics` | Record a single telemetry event into SQLite |
| `GET` | `/api/export.csv` | Export all telemetry records formatted as RFC 4180 CSV |
| `POST` | `/api/import.csv` | Batch ingest CSV rows directly into the database |
