# Third-party notices

herdr-scm is MIT-licensed (see [LICENSE](LICENSE)). It is built from the Rust crates listed
below, each under its own license. This file is derived from [`Cargo.lock`](Cargo.lock) —
every crate in the resolved dependency graph is here, including the ones that only build on
platforms herdr-scm does not ship for yet. Licenses are the SPDX expressions the crates
themselves declare; the full license texts live in each project's repository, linked per row.

Regenerate this file whenever the dependency graph changes.

## Licenses in the graph

| License | Crates |
|---|---:|
| `MIT OR Apache-2.0` | 104 |
| `MIT` | 53 |
| `MIT/Apache-2.0` | 12 |
| `Apache-2.0 OR MIT` | 7 |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 5 |
| `Unlicense OR MIT` | 5 |
| `Apache-2.0/MIT` | 2 |
| `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | 2 |
| `Unlicense/MIT` | 2 |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | 1 |
| `(MIT OR Apache-2.0) AND Unicode-DFS-2016` | 1 |
| `Apache-2.0` | 1 |
| `Apache-2.0 / MIT` | 1 |
| `Apache-2.0 OR BSL-1.0` | 1 |
| `MIT AND Unicode-DFS-2016` | 1 |
| `MIT OR MPL-2.0` | 1 |
| `WTFPL` | 1 |
| `Zlib` | 1 |
| `Zlib OR Apache-2.0 OR MIT` | 1 |

## Direct dependencies

The crates herdr-scm names in its own `Cargo.toml`. `tempfile` is a dev-dependency: it is used
by the test suite only and is not part of the shipped binary.

| Crate | Version | License | Source |
|---|---|---|---|
| `ansi-to-tui` | 8.0.1 | MIT | https://github.com/ratatui/ansi-to-tui |
| `crossterm` | 0.29.0 | MIT | https://github.com/crossterm-rs/crossterm |
| `ignore` | 0.4.33 | Unlicense OR MIT | https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore |
| `ratatui` | 0.30.2 | MIT | https://github.com/ratatui/ratatui |
| `serde` | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| `serde_json` | 1.0.151 | MIT OR Apache-2.0 | https://github.com/serde-rs/json |
| `tempfile` | 3.27.0 | MIT OR Apache-2.0 | https://github.com/Stebalien/tempfile |
| `toml` | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |

## Full dependency graph (202 crates)

| Crate | Version | License |
|---|---|---|
| [aho-corasick](https://github.com/BurntSushi/aho-corasick) | 1.1.5 | Unlicense OR MIT |
| [allocator-api2](https://github.com/zakarumych/allocator-api2) | 0.2.21 | MIT OR Apache-2.0 |
| [ansi-to-tui](https://github.com/ratatui/ansi-to-tui) | 8.0.1 | MIT |
| [anyhow](https://github.com/dtolnay/anyhow) | 1.0.104 | MIT OR Apache-2.0 |
| [approx](https://github.com/brendanzab/approx) | 0.5.1 | Apache-2.0 |
| [atomic](https://github.com/Amanieu/atomic-rs) | 0.6.1 | Apache-2.0/MIT |
| [autocfg](https://github.com/cuviper/autocfg) | 1.5.1 | Apache-2.0 OR MIT |
| [base64](https://github.com/marshallpierce/rust-base64) | 0.22.1 | MIT OR Apache-2.0 |
| [bit-set](https://github.com/contain-rs/bit-set) | 0.5.3 | MIT/Apache-2.0 |
| [bit-vec](https://github.com/contain-rs/bit-vec) | 0.6.3 | MIT/Apache-2.0 |
| [bitflags](https://github.com/bitflags/bitflags) | 1.3.2 | MIT/Apache-2.0 |
| [bitflags](https://github.com/bitflags/bitflags) | 2.13.1 | MIT OR Apache-2.0 |
| [block-buffer](https://github.com/RustCrypto/utils) | 0.10.4 | MIT OR Apache-2.0 |
| [bstr](https://github.com/BurntSushi/bstr) | 1.13.1 | MIT OR Apache-2.0 |
| [bumpalo](https://github.com/fitzgen/bumpalo) | 3.20.3 | MIT OR Apache-2.0 |
| [by_address](https://github.com/mbrubeck/by_address) | 1.2.1 | MIT OR Apache-2.0 |
| [bytemuck](https://github.com/Lokathor/bytemuck) | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| [castaway](https://github.com/sagebind/castaway) | 0.2.4 | MIT |
| [cfg-if](https://github.com/rust-lang/cfg-if) | 1.0.4 | MIT OR Apache-2.0 |
| [cfg_aliases](https://github.com/katharostech/cfg_aliases) | 0.2.2 | MIT |
| [compact_str](https://github.com/ParkMyCar/compact_str) | 0.9.1 | MIT |
| [convert_case](https://github.com/rutrum/convert-case) | 0.10.0 | MIT |
| [cpufeatures](https://github.com/RustCrypto/utils) | 0.2.17 | MIT OR Apache-2.0 |
| [critical-section](https://github.com/rust-embedded/critical-section) | 1.2.0 | MIT OR Apache-2.0 |
| [crossbeam-deque](https://github.com/crossbeam-rs/crossbeam) | 0.8.8 | MIT OR Apache-2.0 |
| [crossbeam-epoch](https://github.com/crossbeam-rs/crossbeam) | 0.9.21 | MIT OR Apache-2.0 |
| [crossbeam-utils](https://github.com/crossbeam-rs/crossbeam) | 0.8.23 | MIT OR Apache-2.0 |
| [crossterm](https://github.com/crossterm-rs/crossterm) | 0.29.0 | MIT |
| [crossterm_winapi](https://github.com/crossterm-rs/crossterm-winapi) | 0.9.1 | MIT |
| [crypto-common](https://github.com/RustCrypto/traits) | 0.1.7 | MIT OR Apache-2.0 |
| [csscolorparser](https://github.com/mazznoer/csscolorparser-rs) | 0.6.2 | MIT OR Apache-2.0 |
| [darling](https://github.com/TedDriggs/darling) | 0.24.1 | MIT |
| [darling_core](https://github.com/TedDriggs/darling) | 0.24.1 | MIT |
| [darling_macro](https://github.com/TedDriggs/darling) | 0.24.1 | MIT |
| [deltae](https://gitlab.com/ryanobeirne/deltae.git) | 0.3.2 | MIT |
| [deranged](https://github.com/jhpratt/deranged) | 0.5.8 | MIT OR Apache-2.0 |
| [derive_more](https://github.com/JelteF/derive_more) | 2.1.1 | MIT |
| [derive_more-impl](https://github.com/JelteF/derive_more) | 2.1.1 | MIT |
| [digest](https://github.com/RustCrypto/traits) | 0.10.7 | MIT OR Apache-2.0 |
| [document-features](https://github.com/slint-ui/document-features) | 0.2.12 | MIT OR Apache-2.0 |
| [either](https://github.com/rayon-rs/either) | 1.18.0 | MIT OR Apache-2.0 |
| [equivalent](https://github.com/indexmap-rs/equivalent) | 1.0.2 | Apache-2.0 OR MIT |
| [errno](https://github.com/lambda-fairy/rust-errno) | 0.3.14 | MIT OR Apache-2.0 |
| [euclid](https://github.com/servo/euclid) | 0.22.14 | MIT OR Apache-2.0 |
| [fancy-regex](https://github.com/fancy-regex/fancy-regex) | 0.11.0 | MIT |
| [fastrand](https://github.com/smol-rs/fastrand) | 2.5.0 | Apache-2.0 OR MIT |
| [filedescriptor](https://github.com/wezterm/wezterm) | 0.8.3 | MIT |
| [finl_unicode](https://github.com/dahosek/finl_unicode) | 1.4.0 | (MIT OR Apache-2.0) AND Unicode-DFS-2016 |
| [fixedbitset](https://github.com/petgraph/fixedbitset) | 0.4.2 | MIT/Apache-2.0 |
| [fnv](https://github.com/servo/rust-fnv) | 1.0.7 | Apache-2.0 / MIT |
| [foldhash](https://github.com/orlp/foldhash) | 0.2.0 | Zlib |
| [futures-core](https://github.com/rust-lang/futures-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [futures-task](https://github.com/rust-lang/futures-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [futures-util](https://github.com/rust-lang/futures-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [generic-array](https://github.com/fizyk20/generic-array.git) | 0.14.7 | MIT |
| [getrandom](https://github.com/rust-random/getrandom) | 0.3.4 | MIT OR Apache-2.0 |
| [getrandom](https://github.com/rust-random/getrandom) | 0.4.3 | MIT OR Apache-2.0 |
| [globset](https://github.com/BurntSushi/ripgrep/tree/master/crates/globset) | 0.4.20 | Unlicense OR MIT |
| [hashbrown](https://github.com/rust-lang/hashbrown) | 0.16.1 | MIT OR Apache-2.0 |
| [hashbrown](https://github.com/rust-lang/hashbrown) | 0.17.1 | MIT OR Apache-2.0 |
| [heck](https://github.com/withoutboats/heck) | 0.5.0 | MIT OR Apache-2.0 |
| [hex](https://github.com/KokaKiwi/rust-hex) | 0.4.3 | MIT OR Apache-2.0 |
| [ident_case](https://github.com/TedDriggs/ident_case) | 1.0.1 | MIT/Apache-2.0 |
| [ignore](https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore) | 0.4.33 | Unlicense OR MIT |
| [indoc](https://github.com/dtolnay/indoc) | 2.0.7 | MIT OR Apache-2.0 |
| [instability](https://github.com/ratatui/instability) | 0.3.13 | MIT |
| [itertools](https://github.com/rust-itertools/itertools) | 0.14.0 | MIT OR Apache-2.0 |
| [itoa](https://github.com/dtolnay/itoa) | 1.0.18 | MIT OR Apache-2.0 |
| [js-sys](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys) | 0.3.105 | MIT OR Apache-2.0 |
| [kasuari](https://github.com/ratatui/kasuari) | 0.4.12 | MIT OR Apache-2.0 |
| [lab](https://github.com/TooManyBees/lab) | 0.11.0 | MIT |
| [lazy_static](https://github.com/rust-lang-nursery/lazy-static.rs) | 1.5.0 | MIT OR Apache-2.0 |
| [libc](https://github.com/rust-lang/libc) | 0.2.189 | MIT OR Apache-2.0 |
| [libm](https://github.com/rust-lang/compiler-builtins) | 0.2.16 | MIT |
| [line-clipping](https://github.com/ratatui/line-clipping) | 0.3.8 | MIT OR Apache-2.0 |
| [linux-raw-sys](https://github.com/sunfishcode/linux-raw-sys) | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [litrs](https://github.com/LukasKalbertodt/litrs) | 1.0.0 | MIT OR Apache-2.0 |
| [lock_api](https://github.com/Amanieu/parking_lot) | 0.4.14 | MIT OR Apache-2.0 |
| [log](https://github.com/rust-lang/log) | 0.4.34 | MIT OR Apache-2.0 |
| [lru](https://github.com/jeromefroe/lru-rs.git) | 0.18.4 | MIT |
| [mac_address](https://github.com/rep-nop/mac_address) | 1.1.8 | MIT OR Apache-2.0 |
| [memchr](https://github.com/BurntSushi/memchr) | 2.8.3 | Unlicense OR MIT |
| [memmem](http://github.com/jneem/memmem) | 0.1.1 | MIT/Apache-2.0 |
| [memoffset](https://github.com/Gilnaa/memoffset) | 0.9.1 | MIT |
| [minimal-lexical](https://github.com/Alexhuszagh/minimal-lexical) | 0.2.1 | MIT/Apache-2.0 |
| [mio](https://github.com/tokio-rs/mio) | 1.2.3 | MIT |
| [nix](https://github.com/nix-rust/nix) | 0.29.0 | MIT |
| [nom](https://github.com/Geal/nom) | 7.1.3 | MIT |
| [nom](https://github.com/rust-bakery/nom) | 8.0.0 | MIT |
| [num-conv](https://github.com/jhpratt/num-conv) | 0.2.2 | MIT OR Apache-2.0 |
| [num-derive](https://github.com/rust-num/num-derive) | 0.4.2 | MIT OR Apache-2.0 |
| [num-traits](https://github.com/rust-num/num-traits) | 0.2.19 | MIT OR Apache-2.0 |
| [num_threads](https://github.com/jhpratt/num_threads) | 0.1.7 | MIT OR Apache-2.0 |
| [once_cell](https://github.com/matklad/once_cell) | 1.21.4 | MIT OR Apache-2.0 |
| [ordered-float](https://github.com/reem/rust-ordered-float) | 4.6.0 | MIT |
| [palette](https://github.com/Ogeon/palette) | 0.7.7 | MIT OR Apache-2.0 |
| [palette_derive](https://github.com/Ogeon/palette) | 0.7.7 | MIT OR Apache-2.0 |
| [palette_math](https://github.com/Ogeon/palette) | 0.7.7 | MIT OR Apache-2.0 |
| [parking_lot](https://github.com/Amanieu/parking_lot) | 0.12.5 | MIT OR Apache-2.0 |
| [parking_lot_core](https://github.com/Amanieu/parking_lot) | 0.9.12 | MIT OR Apache-2.0 |
| [pest](https://github.com/pest-parser/pest) | 2.9.1 | MIT OR Apache-2.0 |
| [pest_derive](https://github.com/pest-parser/pest) | 2.9.1 | MIT OR Apache-2.0 |
| [pest_generator](https://github.com/pest-parser/pest) | 2.9.1 | MIT OR Apache-2.0 |
| [pest_meta](https://github.com/pest-parser/pest) | 2.9.1 | MIT OR Apache-2.0 |
| [phf](https://github.com/rust-phf/rust-phf) | 0.11.3 | MIT |
| [phf_codegen](https://github.com/rust-phf/rust-phf) | 0.11.3 | MIT |
| [phf_generator](https://github.com/rust-phf/rust-phf) | 0.11.3 | MIT |
| [phf_macros](https://github.com/rust-phf/rust-phf) | 0.11.3 | MIT |
| [phf_shared](https://github.com/rust-phf/rust-phf) | 0.11.3 | MIT |
| [pin-project-lite](https://github.com/taiki-e/pin-project-lite) | 0.2.17 | Apache-2.0 OR MIT |
| [portable-atomic](https://github.com/taiki-e/portable-atomic) | 1.15.0 | Apache-2.0 OR MIT |
| [powerfmt](https://github.com/jhpratt/powerfmt) | 0.2.0 | MIT OR Apache-2.0 |
| [proc-macro2](https://github.com/dtolnay/proc-macro2) | 1.0.107 | MIT OR Apache-2.0 |
| [quote](https://github.com/dtolnay/quote) | 1.0.47 | MIT OR Apache-2.0 |
| [r-efi](https://github.com/r-efi/r-efi) | 5.3.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| [r-efi](https://github.com/r-efi/r-efi) | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| [rand](https://github.com/rust-random/rand) | 0.8.8 | MIT OR Apache-2.0 |
| [rand_core](https://github.com/rust-random/rand) | 0.6.4 | MIT OR Apache-2.0 |
| [ratatui](https://github.com/ratatui/ratatui) | 0.30.2 | MIT |
| [ratatui-core](https://github.com/ratatui/ratatui) | 0.1.2 | MIT |
| [ratatui-crossterm](https://github.com/ratatui/ratatui) | 0.1.2 | MIT |
| [ratatui-macros](https://github.com/ratatui/ratatui) | 0.7.2 | MIT |
| [ratatui-termina](https://github.com/ratatui/ratatui) | 0.1.0 | MIT |
| [ratatui-termwiz](https://github.com/ratatui/ratatui) | 0.1.2 | MIT |
| [ratatui-widgets](https://github.com/ratatui/ratatui) | 0.3.2 | MIT |
| [redox_syscall](https://gitlab.redox-os.org/redox-os/syscall) | 0.5.18 | MIT |
| [regex](https://github.com/rust-lang/regex) | 1.13.1 | MIT OR Apache-2.0 |
| [regex-automata](https://github.com/rust-lang/regex) | 0.4.18 | MIT OR Apache-2.0 |
| [regex-syntax](https://github.com/rust-lang/regex) | 0.8.11 | MIT OR Apache-2.0 |
| [rustc_version](https://github.com/djc/rustc-version-rs) | 0.4.1 | MIT OR Apache-2.0 |
| [rustix](https://github.com/bytecodealliance/rustix) | 1.1.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [rustversion](https://github.com/dtolnay/rustversion) | 1.0.23 | MIT OR Apache-2.0 |
| [ryu](https://github.com/dtolnay/ryu) | 1.0.23 | Apache-2.0 OR BSL-1.0 |
| [same-file](https://github.com/BurntSushi/same-file) | 1.0.6 | Unlicense/MIT |
| [scopeguard](https://github.com/bluss/scopeguard) | 1.2.0 | MIT OR Apache-2.0 |
| [semver](https://github.com/dtolnay/semver) | 1.0.28 | MIT OR Apache-2.0 |
| [serde](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_core](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_derive](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_json](https://github.com/serde-rs/json) | 1.0.151 | MIT OR Apache-2.0 |
| [serde_spanned](https://github.com/toml-rs/toml) | 1.1.1 | MIT OR Apache-2.0 |
| [sha2](https://github.com/RustCrypto/hashes) | 0.10.9 | MIT OR Apache-2.0 |
| [signal-hook](https://github.com/vorner/signal-hook) | 0.3.18 | Apache-2.0/MIT |
| [signal-hook-mio](https://github.com/vorner/signal-hook) | 0.2.5 | MIT OR Apache-2.0 |
| [signal-hook-registry](https://github.com/vorner/signal-hook) | 1.4.8 | MIT OR Apache-2.0 |
| [simdutf8](https://github.com/rusticstuff/simdutf8) | 0.1.5 | MIT OR Apache-2.0 |
| [siphasher](https://github.com/jedisct1/rust-siphash) | 1.0.3 | MIT/Apache-2.0 |
| [slab](https://github.com/tokio-rs/slab) | 0.4.12 | MIT |
| [smallvec](https://github.com/servo/rust-smallvec) | 1.16.0 | MIT OR Apache-2.0 |
| [static_assertions](https://github.com/nvzqz/static-assertions-rs) | 1.1.0 | MIT OR Apache-2.0 |
| [strsim](https://github.com/rapidfuzz/strsim-rs) | 0.11.1 | MIT |
| [strum](https://github.com/Peternator7/strum) | 0.28.0 | MIT |
| [strum_macros](https://github.com/Peternator7/strum) | 0.28.0 | MIT |
| [syn](https://github.com/dtolnay/syn) | 1.0.109 | MIT OR Apache-2.0 |
| [syn](https://github.com/dtolnay/syn) | 2.0.119 | MIT OR Apache-2.0 |
| [syn](https://github.com/dtolnay/syn) | 3.0.5 | MIT OR Apache-2.0 |
| [tempfile](https://github.com/Stebalien/tempfile) | 3.27.0 | MIT OR Apache-2.0 |
| [termina](https://github.com/helix-editor/termina) | 0.3.3 | MIT OR MPL-2.0 |
| [terminfo](https://github.com/meh/rust-terminfo) | 0.9.0 | WTFPL |
| [termios](https://github.com/dcuddeback/termios-rs) | 0.3.3 | MIT |
| [termwiz](https://github.com/wezterm/wezterm) | 0.23.3 | MIT |
| [thiserror](https://github.com/dtolnay/thiserror) | 1.0.69 | MIT OR Apache-2.0 |
| [thiserror](https://github.com/dtolnay/thiserror) | 2.0.20 | MIT OR Apache-2.0 |
| [thiserror-impl](https://github.com/dtolnay/thiserror) | 1.0.69 | MIT OR Apache-2.0 |
| [thiserror-impl](https://github.com/dtolnay/thiserror) | 2.0.20 | MIT OR Apache-2.0 |
| [time](https://github.com/time-rs/time) | 0.3.55 | MIT OR Apache-2.0 |
| [time-core](https://github.com/time-rs/time) | 0.1.9 | MIT OR Apache-2.0 |
| [toml](https://github.com/toml-rs/toml) | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_datetime](https://github.com/toml-rs/toml) | 1.1.1+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_parser](https://github.com/toml-rs/toml) | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 |
| [typenum](https://github.com/paholg/typenum) | 1.20.1 | MIT OR Apache-2.0 |
| [ucd-trie](https://github.com/BurntSushi/ucd-generate) | 0.1.7 | MIT OR Apache-2.0 |
| [unicode-ident](https://github.com/dtolnay/unicode-ident) | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| [unicode-segmentation](https://github.com/unicode-rs/unicode-segmentation) | 1.13.3 | MIT OR Apache-2.0 |
| [unicode-truncate](https://github.com/Aetf/unicode-truncate) | 2.0.1 | MIT OR Apache-2.0 |
| [unicode-width](https://github.com/unicode-rs/unicode-width) | 0.2.2 | MIT OR Apache-2.0 |
| [utf8parse](https://github.com/alacritty/vte) | 0.2.2 | Apache-2.0 OR MIT |
| [uuid](https://github.com/uuid-rs/uuid) | 1.26.0 | Apache-2.0 OR MIT |
| [version_check](https://github.com/SergioBenitez/version_check) | 0.9.5 | MIT/Apache-2.0 |
| [vtparse](https://github.com/wez/wezterm) | 0.6.2 | MIT |
| [walkdir](https://github.com/BurntSushi/walkdir) | 2.5.0 | Unlicense/MIT |
| [wasi](https://github.com/bytecodealliance/wasi) | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [wasip2](https://github.com/bytecodealliance/wasi-rs) | 1.0.4+wasi-0.2.12 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [wasm-bindgen](https://github.com/wasm-bindgen/wasm-bindgen) | 0.2.128 | MIT OR Apache-2.0 |
| [wasm-bindgen-macro](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro) | 0.2.128 | MIT OR Apache-2.0 |
| [wasm-bindgen-macro-support](https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support) | 0.2.128 | MIT OR Apache-2.0 |
| [wasm-bindgen-shared](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared) | 0.2.128 | MIT OR Apache-2.0 |
| [wezterm-bidi](https://github.com/wez/wezterm) | 0.2.3 | MIT AND Unicode-DFS-2016 |
| [wezterm-blob-leases](https://github.com/wezterm/wezterm) | 0.1.1 | MIT |
| [wezterm-color-types](https://github.com/wez/wezterm) | 0.3.0 | MIT |
| [wezterm-dynamic](https://github.com/wezterm/wezterm) | 0.2.1 | MIT |
| [wezterm-dynamic-derive](https://github.com/wezterm/wezterm) | 0.1.1 | MIT |
| [wezterm-input-types](https://github.com/wez/wezterm) | 0.1.0 | MIT |
| [winapi](https://github.com/retep998/winapi-rs) | 0.3.9 | MIT/Apache-2.0 |
| [winapi-i686-pc-windows-gnu](https://github.com/retep998/winapi-rs) | 0.4.0 | MIT/Apache-2.0 |
| [winapi-util](https://github.com/BurntSushi/winapi-util) | 0.1.11 | Unlicense OR MIT |
| [winapi-x86_64-pc-windows-gnu](https://github.com/retep998/winapi-rs) | 0.4.0 | MIT/Apache-2.0 |
| [windows-link](https://github.com/microsoft/windows-rs) | 0.2.1 | MIT OR Apache-2.0 |
| [windows-sys](https://github.com/microsoft/windows-rs) | 0.61.2 | MIT OR Apache-2.0 |
| [winnow](https://github.com/winnow-rs/winnow) | 1.0.4 | MIT |
| [wit-bindgen](https://github.com/bytecodealliance/wit-bindgen) | 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [zmij](https://github.com/dtolnay/zmij) | 1.0.23 | MIT |

## External programs

These are not bundled or redistributed with herdr-scm — it runs whichever copy is already on
your machine. They are listed for attribution and so you know what to install.

| Program | Required? | Project | License |
|---|---|---|---|
| `herdr` | yes — the host this plugin runs inside | <https://herdr.dev> ([herdrdev/herdr](https://github.com/herdrdev/herdr)) | Apache-2.0 |
| `git` | yes — every status and diff comes from it | <https://git-scm.com> | GPL-2.0-only |
| `cargo` / `rustc` | yes, at install time — the plugin builds from source | <https://rustup.rs> | MIT OR Apache-2.0 |
| `delta` | optional — the default `diff_tool`; unset it for plain text | <https://github.com/dandavison/delta> | MIT |
| `bash` | yes — the two launcher scripts | <https://www.gnu.org/software/bash/> | GPL-3.0-or-later |
