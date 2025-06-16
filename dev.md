Dev Setup
=========

This dev setup assumes you are working in ~/code. This setup is for linux (but should work on mac too with some tweaks i.e brew instead of apt)
```sh
# if you don't have a code folder in your home directory
mkdir ~/code
cd code
# make sure you have rust up to date or install it 
# curl https://sh.rustup.rs -sSf | sh
rustup update
```

1. eltor
```sh
cd ~/code
git clone https://github.com/el-tor/eltor.git
cd eltor
# install deps
sudo apt-get install automake libevent-dev zlib1g zlib1g-dev build-essential libssl-dev
./autogen.sh
./configure
make
```

2. libeltor-sys
```sh
cd ~/code
git clone https://github.com/el-tor/libeltor-sys.git
cd libeltor-sys
scripts/copy.sh
mkdir -p ~/code/libeltor-sys/libtor-src/patches
sudo apt install pkg-config zlib1g-dev
scripts/build.sh
```

3. libeltor
```sh
cd ~/code
git clone https://github.com/el-tor/libeltor.git
cd libeltor
cargo build --features=vendored-openssl
```

4. lni
```sh
cd ~/code
git clone https://github.com/lightning-node-interface/lni.git
cd lni
nano crates/lni/Cargo.toml
# *comment out `crate-type = ["staticlib", "lib"]` when building for napi_rs
cd bindings/lni_nodejs && cargo clean && cargo build --release && yarn && yarn build && cd ../../
```

5. eltord
```sh
cd ~/code
git clone https://github.com/el-tor/eltord.git
cd eltord
cargo run
```

6. eltor-app
```sh
cd ~/code
git clone https://github.com/el-tor/eltor-app.git
cd frontend
pnpm i
pnpm run dev:tauri
cd ..
# if you want to run as web app
cd backend
./run.sh
cd ../frontend
pnpm run dev:web
```
