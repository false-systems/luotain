# 150-Request Load Test

Send 150 HTTP requests through the RAUTA proxy and verify the system handles them correctly.

## Prerequisites

- RAUTA is deployed on Kubernetes with a working Gateway and HTTPRoute
- A demo backend (echo service) is routed via `Host: echo.local`
- Admin API is accessible on port 9091

## Baseline status

- Query GET /api/v1/status on the admin endpoint (port 9091)
- Response should return 200 with JSON
- `route_count` should be >= 1
- `open_circuits` should be 0

## Send 100 GET requests to /

- Send 100 GET requests to the proxy (port 8080) with `Host: echo.local` and path `/`
- Every request should return 200 OK
- Response bodies should contain backend echo output (JSON with `status`, `version`, `backend` fields)
- No requests should timeout (5s threshold)

## Send 50 GET requests to /api

- Send 50 GET requests to the proxy with `Host: echo.local` and path `/api`
- Every request should return 200 OK
- Response bodies should contain backend echo output
- The `/api` route has weighted backends (90% stable, 10% canary)
- With Maglev consistent hashing, a single source IP may not see canary traffic — this is expected behavior

## Verify admin status after load

- Query GET /api/v1/status on admin endpoint again
- `status` should still be `ok`
- `open_circuits` should still be 0 (no circuit breakers tripped)
- `exhausted_rate_limiters` should be 0
- `route_count` should be unchanged

## Verify route listing

- Query GET /api/v1/routes on admin endpoint
- Should return JSON array with 2 routes: `/` and `/api`
- Each route should have `backends` with healthy pod IPs
- No backends should be draining
