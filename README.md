# ibp-matrix-alerts-bot
Subscribe to Alerts published by ibp-monitor and delivery over matrix

## 🚧 Work In Progress

- [&check;] matrix authentication, load and process commands from public and private rooms
- [&check;] implement http server with shared state (cache and matrix)
- [&check;] review matrix commands:
    - [&check;] !subscribe alerts MEMBER SEVERITY
    - [&check;] !unsubscribe alerts MEMBER SEVERITY
    - [&check;] !help
- [ ] implement /alerts webhook
- [ ] allow configuration of mute time interval
- [ ] define alert message template