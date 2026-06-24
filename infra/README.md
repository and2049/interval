# Infrastructure Notes

Initial demo deployment should be a simple web deployment:

- Rust backend service serving the API.
- Static Solid frontend built with Vite.
- SQLite database for cached historical OpenF1 data and normalized replay snapshots.

Live-mode infrastructure, paid upstream credentials, and separate streaming transport are intentionally out of scope for the MVP scaffold.
