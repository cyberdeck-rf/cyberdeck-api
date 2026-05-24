# cyberdeck-api

Host-side library (Rust, edition 2024) for the WraithRF / RF Cyberdeck firmware.

Two crates :

- **`cyberdeck-api`** (`crates/core`) — protocol codec, USB transport, plugins per RF band.
- **`cyberdeck-cli`** (`crates/cli`) — `clap`-based CLI (`status`, `scan`, `inject`, …).

See [CLAUDE.md](./CLAUDE.md) for full context, architecture and conventions.

## Quickstart

```bash
cargo run -p cyberdeck-cli -- status
```

The CLI auto-detects the first serial port advertising the STM32 VID (0x0483). Pass `--port /dev/cu.usbmodemXXXX` to override.

## License

Dual-licensed under MIT or Apache-2.0.
