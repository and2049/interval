# Infrastructure Notes

The app currently ships as a native GPUI desktop binary (`interval-desktop`) that embeds the backend in-process, so there is no server deployment for the MVP. The pieces are:

- Rust backend service exposing the API (embedded in the desktop app on a loopback port; runnable standalone for development).
- Native GPUI desktop client that consumes the backend over HTTP + SSE.
- SQLite database for cached historical OpenF1 data and normalized replay snapshots.

If a hosted web deployment is revisited later, the same backend can serve a browser client over the same HTTP contract.

Live-mode infrastructure, paid upstream credentials, and separate streaming transport are intentionally out of scope for the MVP scaffold.
