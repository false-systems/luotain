# Event Retrieval

## GET /api/v1/events/{id} with valid event ID

- Returns 200 with event JSON
- Response includes `source`, `type`, `severity` fields
- Response includes `timestamp` field in ISO 8601 format

## GET /api/v1/events/{id} with nonexistent ID

- Returns 404

## GET /api/v1/events/{id} with garbage ID

- Returns 400 (not 500)
