# ibp-matrix-alerts-bot
Subscribe to Alerts published by ibp-monitor and delivery over matrix

## 🚧 Work In Progress

- [&check;] matrix authentication, load and process commands from public and private rooms
- [&check;] implement http server with shared state (cache and matrix)
- [ ] implement /alerts webhook
- [ ] allow configuration of mute time interval
- [ ] define alert message template
- [ ] review matrix commands:
    - !subscribe alerts MEMBER SEVERITY
    - !unsubscribe alerts MEMBER SEVERITY
    - !help