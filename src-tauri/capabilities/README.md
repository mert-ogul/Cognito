# Tauri capabilities

`default.json` grants the minimum set for the main window:

- `core:default`: core APIs including invoke only to handlers registered in Rust (`generate_handler![...]`). There is no generic command execution from the webview.
- `core:event:default`: listen and emit for import progress, chat streaming, and related events.
- `dialog:default`: native folder picker for choosing a library folder.

Adding filesystem or HTTP permissions would need an explicit product decision and an ADR update.
