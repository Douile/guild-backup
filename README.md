Download all text messages sent in a discord guild using the HTTP API

## Usage
```
BOT_TOKEN="API_TOKEN" GUILD_ID="GUILD_ID" ./guild-backup
```

## TODO
- Add better output format (JSON not the way to go, maybe sqlite)
- Extend cache support
  - Check for messages older than or newer the ones currently downloaded if
  file already exists
- CLI args (clap)
- Better progress logging
- Backup other metadata
  - Roles/permissions
  - Emojis
  - Stickers
  - Audit log
  - Attachments (maybe discord CDN should keep them forever)
- Restore backup?
- Better async
  - Download multiple channels at same time
