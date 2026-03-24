# Bearer Token Authentication

The system validates bearer token authentication on protected endpoints.

## GET /bearer without token

- Returns 401 Unauthorized
- Response body indicates authentication is required

## GET /bearer with valid token

- Send header: Authorization: Bearer <any-token>
- Returns 200 OK
- Response body includes the token value that was sent

## GET /bearer with malformed auth header

- Send header: Authorization: Basic abc123
- Returns 401 Unauthorized (bearer tokens only, not basic auth)
