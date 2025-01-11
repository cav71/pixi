# A small trampoline binary that allow to run executables installed by pixi global install.


This is the configuration used by trampoline to set the env variables, and run the executable.

```js
{
    // Full path to the executable
    "exe": "/Users/wolfv/.pixi/envs/conda-smithy/bin/conda-smithy",
    // One or more path segments to prepend to the PATH environment variable
    "path": "/Users/wolfv/.pixi/envs/conda-smithy/bin",
    // One or more environment variables to set
    "env": {
        "CONDA_PREFIX": "/Users/wolfv/.pixi/envs/conda-smithy"
    }
}
```

# How to build it?
You can use `trampoline.yaml` workflow to build the binary for all the platforms and architectures supported by pixi.
In case of building it manually, you can use the following command, after executing the `cargo build --release`, you need to compress it using `zstd`.
If running it manually or triggered by changes in `crates/pixi_trampoline` from the main repo, they will be automatically committed to the branch.


# Manual build & integrate

Build for the current target using cargo:

```sh
python3 trampoline/build-trampoline.py
```

Build for another target using cross

```sh
python3 trampoline/build-trampoline.py --target armv7-unknown-linux-gnueabihf --cargo cross
```

Add the newly create trampoline (under trampoline/binaries/*.zst) in src/global/trampoline.rs:

```rust
use super::ExposedName;
 
#[cfg(target_arch = "arm")]
#[cfg(target_os = "linux")]
#[cfg(target_abi = "eabihf")]
const TRAMPOLINE_BIN: &[u8] =
    include_bytes!("../../trampoline/binaries/pixi-trampoline-armv7-unknown-linux-gnueabihf.zst");
```
