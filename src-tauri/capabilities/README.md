# Tauri capabilities

`default.json` grants the minimum set for the MVP window:

- `core:default` — core APIs including **invoke** only to handlers registered in Rust (`generate_handler![...]`). There is no generic command execution from the webview.
- `core:event:default` — listen/emit for import progress, chat streaming, etc.
- `dialog:default` — native folder picker for vault selection.

Adding filesystem or HTTP permissions would require an explicit product decision and ADR update.
