# Relay Run in the browser

The browser build uses WebGPU, retaining Hanabi compute particles and the same
combat simulation as the native demo. It requires a desktop browser with WebGPU,
hardware acceleration, keyboard and mouse. Click Launch, wait for assets to load,
then click the game or press Enter. Escape releases the mouse and pauses. Best
score uses browser localStorage; blocked storage does not stop play.

From the repository root:

```sh
rustup target add wasm32-unknown-unknown
CARGO_TARGET_DIR=target cargo build --locked --release --target wasm32-unknown-unknown --manifest-path demos/relay-runner/Cargo.toml
# Install the wasm-bindgen-cli version recorded in demos/relay-runner/Cargo.lock.
wasm-bindgen --target web --out-name relay_run --out-dir out/relay-wasm target/wasm32-unknown-unknown/release/relay-runner.wasm
python3 demos/relay-runner/web/package.py --wasm-dir out/relay-wasm --output out/relay-pages
python3 -m http.server --directory out/relay-pages 8000
```

Open http://localhost:8000. Output directories must be new. WebGPU requires
localhost or HTTPS. The package verifies every committed runtime asset against
its receipt and checks embedded metadata against hosted JSON before copying.
No generator, private installation, model weights or local configuration is
needed to build. Generated asset bytes are preserved from accepted v16; see
[NOTICES.md](NOTICES.md) for provenance and non-commercial texture restrictions.

The `Relay Run Pages` workflow builds pull requests and deploys main through
GitHub Pages. The repository's Pages source must be GitHub Actions. The hosted
payload contains the game, its selected assets and notices; full local audition
and review archives remain outside it.
