# inventory-client

Client for `inventory-api`. Every function takes a `transport`,
`async (request) => response`, so callers choose HTTP, IPC or a fake.

- `listItems(transport)` → every item, ordered by `id`.
- `getItem(transport, id)` → one item; rejects on any non-200 response.
- `lowStock(items, threshold)` → SKUs with `qty` below the threshold.

Run the tests with `npm test`.
