# POST Requests

The system accepts POST requests with various body types and echoes them back.

## POST /post with JSON body

- Accepts Content-Type: application/json
- Returns 200 OK
- Response `json` field contains the parsed request body
- Response `data` field contains the raw request body as string

## POST /post with form data

- Accepts Content-Type: application/x-www-form-urlencoded
- Returns 200 OK
- Response `form` field contains the parsed form key-value pairs

## POST /post without body

- Returns 200 OK
- Response `data` field is empty string
- Response `json` field is null
