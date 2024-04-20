# hikkaCLI
CLI tool to interact with hikka.io

## **Features**:
- [x] Translate characters from anime
- [ ] Bulk translate
- [x] Find words in description of chraracter via edits

## Requirements
- Rust `>=1.76.0` (for build only)
- geckodriver

## How to login
For now you need to get auth token from hikka. To do this open devtools, go to Storage tab (Firefox) or Application tab (Chrome). And just copy value of `auth`.

After this create file `.env` in folder where is app located, with this content:

```bash
AUTH_TOKEN=*your auth token*
```

## Build
1. Clone repo

```bash
git clone https://github.com/rosset-nocpes/hikka_cli
```

3. Build with cargo:

```bash
cargo build
```

Or just run:
```bash
cargo run
```
