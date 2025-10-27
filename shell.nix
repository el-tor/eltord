{ pkgs ? import (fetchTarball "https://github.com/NixOS/nixpkgs/archive/24.11.tar.gz") {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    # Rust toolchain  
    rustc
    cargo
    rustfmt
    clippy
    
    # Build dependencies
    pkg-config
    openssl
    sqlite
    gnumake
    wget
    git
    unzip
    gh
  ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
    # macOS specific - conditionally added only on Darwin
    darwin.apple_sdk.frameworks.Security
    darwin.apple_sdk.frameworks.SystemConfiguration
    libiconv
  ];
  
  # Environment variables
  OPENSSL_DIR = "${pkgs.openssl.dev}";
  OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
  OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
  PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig:${pkgs.sqlite.dev}/lib/pkgconfig";
  SQLITE3_LIB_DIR = "${pkgs.sqlite.out}/lib";
  SQLITE3_INCLUDE_DIR = "${pkgs.sqlite.dev}/include";
  
  # Rust target
  CARGO_BUILD_TARGET = "aarch64-apple-darwin";

  shellHook = ''
    if [ -f .secrets ]; then
      set -a
      source .secrets
      set +a
    fi
  '';
}
