# SQL Query API

## POST /api/v1/query with valid SQL

- Accepts JSON body with `sql` field
- Returns 200 with `rows` array
- SELECT COUNT(*) returns a single row with numeric value

## POST /api/v1/query with invalid SQL

- Returns 400 with error message
- Does not crash the server (subsequent queries still work)

## POST /api/v1/query with SQL injection attempt

- Input: `sql: "'; DROP TABLE events; --"`
- Returns 400 error
- Does not execute destructive statements
