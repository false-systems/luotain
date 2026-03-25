# Gateway Health

## GET /api/v1/status

- Returns 200 OK
- Response is JSON with `uptime_seconds` field
- Response includes `route_count` (numeric, >= 0)
- Response includes `open_circuits` count
