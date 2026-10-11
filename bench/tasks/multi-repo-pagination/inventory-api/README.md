# inventory-api

In-memory inventory service. `api.handle(request)` maps a request
`{"method", "path", "query"}` to a response `{"status", "body"}`;
`python3 api_cli.py < request.json` serves one request from the shell.

## Endpoints

- `GET /items` → `{"items": [...]}`: every item, ordered by `id`.
- `GET /items/<id>` → the item, 404 when unknown, 400 when `id` is not a number.

Run the tests with `python3 -m unittest`.
