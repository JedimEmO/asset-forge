# Relay Run in the browser

The browser build uses WebGPU, retaining Hanabi compute particles and the same
combat simulation as the native demo. It requires a desktop browser with WebGPU,
hardware acceleration, keyboard and mouse. Click Launch game, wait for assets to load,
then click the game or press Enter. Escape releases the mouse and pauses. Best
score uses browser localStorage; blocked storage does not stop play.
Press F3 to show actual game FPS, frame-time p95, render resolution and the
browser-reported graphics adapter. These are live measurements, not an FPS target.

**[Play Relay Run](https://jedimemo.github.io/asset-forge/)** on GitHub Pages.
The public repository deploys through GitHub Actions.

From a fresh checkout, use the repository's pinned Rust toolchain and Python 3.11
or later. Install the CLI version from the lockfile so its bindings match the game:

```sh
rustup target add wasm32-unknown-unknown
relay_bindgen_version=$(python3 -c 'import tomllib; d=tomllib.load(open("demos/relay-runner/Cargo.lock","rb")); print(next(p["version"] for p in d["package"] if p["name"] == "wasm-bindgen"))')
cargo install wasm-bindgen-cli --version "$relay_bindgen_version" --locked
python3 demos/relay-runner/web/package.py --check
CARGO_TARGET_DIR=target cargo build --locked --release --target wasm32-unknown-unknown --manifest-path demos/relay-runner/Cargo.toml
wasm-bindgen --target web --out-name relay_run --out-dir out/relay-wasm target/wasm32-unknown-unknown/release/relay-runner.wasm
python3 demos/relay-runner/web/package.py --wasm-dir out/relay-wasm --output out/relay-pages
python3 -m http.server --directory out/relay-pages 8000
```

Open [the local game](http://localhost:8000). The package output directory must be new.
WebGPU requires localhost or HTTPS. The package verifies every committed runtime asset against
its receipt and checks embedded metadata against hosted JSON before copying.
No generator, private installation, model weights or local configuration is
needed to build. Generated asset bytes are preserved from accepted v16; see
[NOTICES.md](NOTICES.md) for provenance and non-commercial texture restrictions.

The [Relay Run Pages workflow](../../../.github/workflows/relay-pages.yml) builds
on manual dispatch, or on relevant main pushes when `RELAY_PAGES_ENABLED` is
`true`. It does not compile the game on every PR. Deployment additionally
requires main; the variable alone does not make the repository eligible for Pages.
The repository's Pages source must be GitHub Actions. Keep deployment disabled
when publishing the verified artifact through a separate demo repository. The hosted
payload contains the game, its selected assets and notices; full local audition
and review archives remain outside it.

Native play uses these same assets; see [the demo README](../README.md).
Browser verification covers rendering, mouse lock, rifle/plasma input,
pause/resume and a running audio context. Audio activation is not a new listening
approval. See [verification](../VERIFICATION.md) for tested hardware and limits.
