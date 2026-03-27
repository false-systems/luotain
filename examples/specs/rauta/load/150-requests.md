# 150-Request Load Test with Metrics Verification

Send 150 HTTP requests through the RAUTA proxy and verify that Prometheus metrics accurately reflect the traffic.

## Prerequisites

- RAUTA is deployed on Kubernetes with a working Gateway and HTTPRoute
- A demo backend (echo service) is routed via `Host: echo.local`
- Prometheus metrics are exposed on port 9090

## Baseline metrics

- Query GET /metrics on the metrics endpoint (port 9090)
- Record the current value of `http_requests_total` (sum across all labels)
- If no prior traffic, the counter may not exist yet — that's fine

## Send 150 GET requests to the proxy

- Send 150 sequential GET requests to the proxy (port 8080) with `Host: echo.local`
- Use path `/` for the first 100 requests
- Use path `/api` for the remaining 50 requests
- Every request should return 200 OK
- Response bodies should contain echo output from the backend
- No requests should timeout (5s threshold)

## Verify Prometheus metrics after load

- Query GET /metrics on the metrics endpoint again
- `http_requests_total` should have increased by at least 150 compared to baseline
- `http_request_duration_seconds_count` should also reflect the 150 new requests
- `http_request_duration_seconds_bucket` histograms should show most requests under 1 second
- Metrics should include labels for method (GET), status (200), and path

## Verify status endpoint reflects traffic

- Query GET /api/v1/status on the admin endpoint (port 9091)
- Response should return 200 with JSON
- `route_count` should be >= 1 (the echo route exists)
