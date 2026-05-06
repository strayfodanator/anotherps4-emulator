# anotherps4 emulator

A ps4 emulator written in rust, focused on native performance.

## features (incompleted)
* native performance execution
* vulkan based gpu emulation
* multi threaded core architecture
* cross platform support (Linux primarily, Windows in future)

##  requisites to build

to build and run anotherps4, you will need:
* [rust toolchain](https://rustup.rs/) (latest stable)
* vulkan sdk and drivers installed on your system

### linux dependencies to run
on debian based systems:
```bash
sudo apt install build-essential libvulkan-dev vulkan-tools
```

##  building from source

1. clone the repository:
```bash
git clone https://github.com/strayfodanator/anotherps4-emulator.git
cd anotherps4-emulator
```

2. build the project using Cargo:
```bash
cargo build --release
```

3. run the emulator:
```bash
cargo run --release -- [arguments]
```

##  architecture of project

the project is structured as a cargo workspace with several specialized crates:
* `anotherps4-core` - cpu execution and core system timing/events
* `anotherps4-gpu` - vulkan-based graphics rendering
* `anotherps4-audio` - sound emulation
* `anotherps4-input` - controller input mapping
* `anotherps4-formats` - PKG/PFS parsing and executable loading
* `anotherps4-bin` - The main executable and CLI frontend
* `anotherps4-common` - Shared utilities and types

## license

this project is licensed under the gplv2 or later. See the [LICENSE](LICENSE) file for details.

##  disclaimer!!! dont ignore this sony lawyers! (does anyone actually read this?)

this project is an independent piece of software developed for educational purposes. it is not affiliated with, nor authorized, endorsed or licensed in any way by Sony Interactive Entertainment Inc. all trademarks are the property of their respective owners.