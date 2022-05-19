# Fathom

## Environment Variables

 Name               | Options    | Source   | Required | Description
--------------------|------------|----------|----------|------------
`RUST_LOG`          | `error`, `warn`, `info`, `debug`, `trace`, see docs for more | tracing-subscriber | No | Configures the global log level. Additional details in the [docs](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/index.html)
`ASSET_DIR`         | \<string\> | `Config` | Yes      | The directory within which all web app assets are located.
