//! HLE library registry.
//!
//! Manages the registration and lookup of all reimplemented PS4 system
//! libraries. Each library registers its exported symbols with the linker.

use crate::loader::symbols::{SymbolRecord, SymbolResolver, SymbolType};

/// Callback type for initializing an HLE library.
pub type HleInitFn = fn(&mut SymbolResolver);

/// Information about an HLE library.
pub struct HleLibrary {
    /// The PS4 module name (e.g. "libkernel", "libSceGnmDriver").
    pub name: &'static str,
    /// Initialization callback that registers symbols.
    pub init: HleInitFn,
}

/// All available HLE libraries.
static HLE_LIBRARIES: &[HleLibrary] = &[
    HleLibrary {
        name: "libkernel",
        init: super::libkernel::register,
    },
];

/// Register all HLE libraries with the given symbol resolver.
pub fn register_all_libraries(resolver: &mut SymbolResolver) {
    tracing::info!("Registering {} HLE libraries", HLE_LIBRARIES.len());

    for lib in HLE_LIBRARIES {
        tracing::debug!(name = lib.name, "Registering HLE library");
        (lib.init)(resolver);
    }

    tracing::info!(
        total_symbols = resolver.len(),
        "All HLE libraries registered"
    );
}

/// Register a single HLE function stub.
///
/// This creates a small trampoline that returns 0 (xor eax, eax; ret).
/// Real implementations will replace these as we implement each function.
pub fn register_stub(resolver: &mut SymbolResolver, nid: &str, name: &str, library: &str) {
    let addr = super::stubs::allocate_stub(name.to_string());
    resolver.add_symbol(
        nid.to_string(),
        SymbolRecord {
            name: name.to_string(),
            virtual_address: addr,
            sym_type: SymbolType::Function,
            library: library.to_string(),
            module: String::new(),
        },
    );
}
