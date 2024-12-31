cargo clean
cargo build -vv --features=vendored-openssl
cargo run --bin eltor -vv --features=vendored-openssl
