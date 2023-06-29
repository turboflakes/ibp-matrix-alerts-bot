# ibp-matrix-alerts-bot
Subscribe to Alerts published by ibp-monitor and delivery over matrix

## 🚧 Work In Progress

- [&check;] matrix authentication, load and process commands from public and private rooms
- [&check;] implement http server with shared state (cache and matrix)
- [&check;] load members from json config file
- [&check;] review matrix commands:
    - [&check;] !subscribe alerts MEMBER SEVERITY [MUTE_INTERVAL]
    - [&check;] !unsubscribe alerts MEMBER SEVERITY
    - [&check;] !maintenance MEMBER MODE
    - [&check;] !alerts
    - [&check;] !help
    - [ ] !stats alerts
    - [ ] !test alert
- [&check;] allow configuration of mute time interval
- [&check;] implement /alerts webhook
- [&check;] implement alert stats counters
- [&check;] define alert message template
- [&check;] protect endpoint with API-Key

## Development / Build from Source

If you'd like to build from source, first install Rust.

```bash
curl https://sh.rustup.rs -sSf | sh
```

If Rust is already installed run

```bash
rustup update
```

Verify Rust installation by running

```bash
rustc --version
```

Once done, finish installing the support software

```bash
sudo apt install build-essential git clang libclang-dev pkg-config libssl-dev
```

Build `abot` by cloning this repository

```bash
#!/bin/bash
git clone http://github.com/ibp-monitor/ibp-matrix-alerts-bot
```

Compile `abot` package with Cargo

```bash
#!/bin/bash
cargo build
```

And then run it

```bash
#!/bin/bash
./target/debug/abot
```

Otherwise, recompile the code on changes and run the binary

```bash
#!/bin/bash
cargo watch -x 'run --bin abot'
```