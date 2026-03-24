# GET Requests

The system handles basic GET requests and reflects request information.

## GET /get

- Returns 200 OK
- Response body is JSON
- Response includes `url` field matching the request URL
- Response includes `headers` object with the request headers sent
- Content-Type response header is `application/json`

## GET /get with query parameters

- GET /get?foo=bar returns 200
- Response `args` object includes `{"foo": "bar"}`
- Multiple query params are all present in `args`
- Empty query param values are preserved
