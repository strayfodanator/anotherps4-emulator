//! PS4 NID-based symbol resolution.
//!
//! PS4 uses a Name ID (NID) system for symbols. Each symbol is identified by
//! an 11-character base64-encoded hash of its name, combined with the library
//! and module IDs. This allows Sony to obfuscate symbol names while still
//! supporting dynamic linking.

use rustc_hash::FxHashMap;
use std::fmt;

/// Type of a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolType {
    /// Unknown type.
    Unknown,
    /// Function symbol.
    Function,
    /// Object/data symbol.
    Object,
    /// TLS variable.
    Tls,
    /// No type.
    NoType,
}

/// A resolved symbol record.
#[derive(Debug, Clone)]
pub struct SymbolRecord {
    /// The symbol name or NID.
    pub name: String,
    /// The virtual address of the symbol.
    pub virtual_address: u64,
    /// The symbol type.
    pub sym_type: SymbolType,
    /// The library name this symbol belongs to.
    pub library: String,
    /// The module name this symbol belongs to.
    pub module: String,
}

/// Resolves PS4 symbols by NID or name.
///
/// Maintains a registry of exported symbols that can be looked up during
/// dynamic linking and relocation.
#[derive(Debug, Default)]
pub struct SymbolResolver {
    /// Symbols indexed by their encoded NID string.
    symbols: FxHashMap<String, SymbolRecord>,
}

impl SymbolResolver {
    /// Create a new empty resolver.
    pub fn new() -> Self {
        Self {
            symbols: FxHashMap::default(),
        }
    }

    /// Register a symbol.
    pub fn add_symbol(&mut self, nid: String, record: SymbolRecord) {
        tracing::trace!(nid = %nid, name = %record.name, addr = format!("0x{:X}", record.virtual_address), "Symbol registered");
        self.symbols.insert(nid, record);
    }

    /// Look up a symbol by NID.
    pub fn find_symbol(&self, nid: &str) -> Option<&SymbolRecord> {
        self.symbols.get(nid)
    }

    /// Resolve a symbol by NID or name, returning its virtual address.
    pub fn resolve(&self, name: &str) -> Option<u64> {
        // Try direct NID lookup first
        if let Some(rec) = self.symbols.get(name) {
            return Some(rec.virtual_address);
        }
        // Try matching by record name (human-readable name)
        for (_nid, rec) in &self.symbols {
            if rec.name == name {
                return Some(rec.virtual_address);
            }
        }
        None
    }

    /// Number of registered symbols.
    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    /// Whether the resolver is empty.
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }

    /// Iterate over all symbols.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &SymbolRecord)> {
        self.symbols.iter()
    }

    /// Merge another resolver into this one (for combining exports from multiple modules).
    pub fn merge(&mut self, other: &SymbolResolver) {
        for (nid, record) in &other.symbols {
            self.symbols.insert(nid.clone(), record.clone());
        }
    }
}

impl fmt::Display for SymbolResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "SymbolResolver ({} symbols):", self.symbols.len())?;
        let mut entries: Vec<_> = self.symbols.iter().collect();
        entries.sort_by_key(|(nid, _)| nid.clone());
        for (nid, record) in entries.iter().take(20) {
            writeln!(
                f,
                "  {nid} -> {} @ 0x{:016X} ({:?})",
                record.name, record.virtual_address, record.sym_type
            )?;
        }
        if entries.len() > 20 {
            writeln!(f, "  ... and {} more", entries.len() - 20)?;
        }
        Ok(())
    }
}

/// Encode a symbol name + library + module into a NID string.
///
/// The PS4 NID is SHA-1(name) truncated to 8 bytes, then base64-encoded
/// to produce an 11-character string. For now, this is a placeholder
/// that returns the raw name for HLE symbols.
pub fn encode_nid(name: &str, _library: &str, _module: &str) -> String {
    // TODO: Implement proper SHA-1 based NID encoding
    // For HLE symbols, we use the raw name as the NID
    name.to_string()
}

/// Lookup a known base64 NID to a human-readable library function name.
/// This allows us to map obfuscated game imports to implementable names.
/// Generated from shadPS4's aerolib.inl database.
pub fn lookup_known_nid(nid: &str) -> Option<&'static str> {
    // Extract just the base64 part, stripping things like `#S#T`
    let base_nid = nid.split('#').next().unwrap_or(nid);
    match base_nid {
        // === C Runtime / LibcInternal ===
        "bzQExy189ZI" => Some("_init_env"),
        "8G2LB+A3rzg" => Some("atexit"),
        "tsvEmnenz48" => Some("__cxa_atexit"),
        "3GPpjQdAMTw" => Some("__cxa_guard_acquire"),
        "9rAeANT2tyE" => Some("__cxa_guard_release"),
        "fJnpuVVBbKk" => Some("_Znwm"),       // operator new(unsigned long)
        "z+P+xCnWLBk" => Some("_ZdlPv"),       // operator delete(void*)
        "gQX+4GDQjpM" => Some("malloc"),
        "tIhsqj0qsFE" => Some("free"),
        "XKRegsFpEpk" => Some("catchReturnFromMain"),
        "uMei1W9uyNo" => Some("exit"),

        // === String / Memory (libc) ===
        "Q3VBxCXhUHs" => Some("memcpy"),
        "8zTFvBIAIN8" => Some("memset"),
        "kiZSXIWd9vg" => Some("strcpy"),
        "j4ViWNHEgww" => Some("strlen"),
        "Ls4tzzhimqQ" => Some("strcat"),
        "Ovb2dSJOAuE" => Some("strcmp"),
        "DfivPArhucg" => Some("memcmp"),

        // === Math ===
        "cpCOXWMgha0" => Some("rand"),
        "ZtjspkJQ+vw" => Some("sinf"),
        "ZE6RNL+eLbk" => Some("tanf"),
        "GZWjF-YIFFk" => Some("asinf"),
        "QI-x0SL8jhw" => Some("acosf"),
        "EH-x713A99c" => Some("atan2f"),

        // === I/O (libc) ===
        "hcuQgD53UxM" => Some("printf"),
        "YQ0navp+YIc" => Some("puts"),
        "xeYO4u7uyJ0" => Some("fopen"),
        "lbB+UlZqVG0" => Some("fread"),
        "uodLYyUip20" => Some("fclose"),
        "rQFVBXp-Cxg" => Some("fseek"),
        "Qazy8LmXTvw" => Some("ftell"),
        "MpxhMh8QFro" => Some("fwrite"),
        "KdP-nULpuGw" => Some("fgets"),
        "Q2V+iqvjgC0" => Some("vsnprintf"),

        // === Kernel Memory ===
        "B+vc2AO2Zrc" => Some("sceKernelAllocateMainDirectMemory"),
        "pO96TwzOm5E" => Some("sceKernelGetDirectMemorySize"),
        "WslcK1FQcGI" => Some("sceKernelIsNeoMode"),
        "rTXw65xmLIA" => Some("sceKernelAllocateDirectMemory"),
        "L-Q3LEjIbgA" => Some("sceKernelMapDirectMemory"),
        "cQke9UuBQOk" => Some("sceKernelMunmap"),
        "MBuItvba6z8" => Some("sceKernelReleaseDirectMemory"),

        // === Kernel Threading / Sync ===
        "2Of0f+3mhhE" => Some("scePthreadMutexDestroy"),
        "cmo1RIYva9o" => Some("scePthreadMutexInit"),
        "9UK1vLZQft4" => Some("scePthreadMutexLock"),
        "tn3VlD0hG60" => Some("scePthreadMutexUnlock"),
        "6UgtwV+0zb4" => Some("scePthreadCreate"),
        "D0OdFMjp46I" => Some("sceKernelCreateEqueue"),
        "jpFjmgAC5AE" => Some("sceKernelDeleteEqueue"),

        // === Video Out ===
        "Up36PTk687E" => Some("sceVideoOutOpen"),
        "CBiu4mCE1DA" => Some("sceVideoOutSetFlipRate"),
        "uquVH4-Du78" => Some("sceVideoOutClose"),
        "6kPnj51T62Y" => Some("sceVideoOutGetResolutionStatus"),
        "i6-sR91Wt-4" => Some("sceVideoOutSetBufferAttribute"),
        "w3BY+tAEiQY" => Some("sceVideoOutRegisterBuffers"),

        // === GNM (GPU) ===
        "b0xyllnVY-I" => Some("sceGnmAddEqEvent"),
        "PVT+fuoS9gU" => Some("sceGnmDeleteEqEvent"),
        "yb2cRhagD1I" => Some("sceGnmDrawInitDefaultHardwareState350"),
        "1qXLHIpROPE" => Some("sceGnmInsertWaitFlipDone"),

        // === Pad (Controller) ===
        "hv1luiJrqQM" => Some("scePadInit"),
        "xk0AcarP3V4" => Some("scePadOpen"),
        "6ncge5+l5Qs" => Some("scePadClose"),
        "YndgXqQVV7c" => Some("scePadReadState"),

        // === User Service ===
        "j3YMu1MVNNo" => Some("sceUserServiceInitialize"),
        "bwFjS+bX9mA" => Some("sceUserServiceTerminate"),
        "CdWp0oHWGr0" => Some("sceUserServiceGetInitialUser"),
        "yH17Q6NWtVg" => Some("sceUserServiceGetEvent"),
        "fPhymKNvK-A" => Some("sceUserServiceGetLoginUserIdList"),

        // === System / Services ===
        "Vo5V8KAwCmk" => Some("sceSystemServiceHideSplashScreen"),
        "fZo48un7LK4" => Some("sceSystemServiceParamGetInt"),
        "656LMQSrg6U" => Some("sceSystemServiceReceiveEvent"),
        "rPo6tV8D9bM" => Some("sceSystemServiceGetStatus"),
        "g8cM39EUZ6o" => Some("sceSysmoduleLoadModule"),
        "uoUpLGNkygk" => Some("sceCommonDialogInitialize"),

        // === Save Data ===
        "TywrFKCoLGY" => Some("sceSaveDataInitialize3"),
        "yKDy8S5yLA0" => Some("sceSaveDataTerminate"),
        "KK3Bdg1RWK0" => Some("sceSaveDataDialogUpdateStatus"),
        "yEiJ-qqr6Cg" => Some("sceSaveDataDialogGetResult"),

        // === NP / Trophy ===
        "A2CQ3kgSopQ" => Some("sceNpSetContentRestriction"),
        "Ec63y59l9tw" => Some("sceNpSetNpTitleId"),
        "GWnWQNXZH5M" => Some("sceNpScoreCreateNpTitleCtxA"),
        "q7U6tEAQf7c" => Some("sceNpTrophyCreateHandle"),
        "TJCAxto9SEU" => Some("sceNpTrophyRegisterContext"),
        "XbkjbobZlCY" => Some("sceNpTrophyCreateContext"),

        // === Screenshot ===
        "73WQ4Jj0nJI" => Some("sceScreenShotSetOverlayImageWithOrigin"),

        // === Audio Out ===
        "JfEPXVxhFqA" => Some("sceAudioOutInit"),
        "ekNvsT22rsY" => Some("sceAudioOutOpen"),

        _ => None,
    }
}
