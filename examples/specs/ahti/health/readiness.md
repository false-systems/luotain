# Health Check

## GET /health

- Returns 200 OK
- Response is JSON with `status` field
- Response includes `uptime` field (numeric, seconds)
- Response includes `components` object

## GET /readyz

- Returns 200 when the system is ready to accept traffic
- Returns 503 when not ready
