//! PS4 dynamic linker.
//!
//! Loads ELF modules into memory, resolves symbols via NID matching,
//! applies relocations, and manages TLS (Thread Local Storage).

pub mod module;
pub mod relocation;
pub mod tls;

use crate::loader::symbols::SymbolResolver;
use crate::memory::MemoryManager;
use module::Module;
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;

/// The dynamic linker, responsible for loading and linking all PS4 modules.
pub struct Linker {
    /// All loaded modules (index 0 is the main executable).
    modules: Vec<Module>,
    /// HLE symbol resolver (reimplemented system functions).
    hle_symbols: SymbolResolver,
    /// Reference to the memory manager.
    memory: Arc<Mutex<MemoryManager>>,
    /// TLS generation counter.
    dtv_generation_counter: u32,
    /// Total static TLS size across all modules.
    static_tls_size: usize,
    /// Maximum TLS index.
    max_tls_index: u32,
}

impl Linker {
    /// Create a new linker.
    pub fn new(memory: Arc<Mutex<MemoryManager>>) -> Self {
        Linker {
            modules: Vec::new(),
            hle_symbols: SymbolResolver::new(),
            memory,
            dtv_generation_counter: 1,
            static_tls_size: 0,
            max_tls_index: 0,
        }
    }

    /// Get a mutable reference to the HLE symbol resolver.
    pub fn hle_symbols_mut(&mut self) -> &mut SymbolResolver {
        &mut self.hle_symbols
    }

    /// Get the HLE symbol resolver.
    pub fn hle_symbols(&self) -> &SymbolResolver {
        &self.hle_symbols
    }

    /// Load a module from disk, and recursively load its dependencies.
    pub fn load_module(&mut self, path: &Path) -> anyhow::Result<u32> {
        let module_name = path.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".to_string());

        if let Some(idx) = self.modules.iter().position(|m| m.name == module_name) {
            tracing::info!(module = %module_name, index = idx, "Module already loaded");
            return Ok(idx as u32);
        }

        tracing::info!(path = %path.display(), "Loading module");

        let module = {
            let mut mem = self.memory.lock();
            Module::load(path, &mut mem, &mut self.max_tls_index)?
        };

        let index = self.modules.len() as u32;
        let mut needed = module.elf.needed_modules();
        let name = module.name.clone();

        tracing::info!(
            index,
            name = %name,
            base = format!("0x{:X}", module.base_virtual_addr),
            "Module loaded"
        );
        let base_virtual_addr = module.base_virtual_addr;
        self.modules.push(module);

        // --- EXPERIMENTAL: Game-Specific Patching ---
        if name == "eboot.bin" {
            let rip_offset = 0x20CD6A;
            unsafe {
                let target_addr = base_virtual_addr as *mut u8;
                let inst_ptr = target_addr.add(rip_offset);
                if *inst_ptr == 0xCD && *inst_ptr.add(1) == 0x41 {
                    tracing::warn!("(HACK) Patching dataFormatEncoder 'int 0x41' assertion at offset 0x{:X}", rip_offset);
                    // Provide memory protection override if needed. VirtualMemory should be R/W/E at this layer.
                    *inst_ptr = 0x90; // NOP
                    *inst_ptr.add(1) = 0x90; // NOP
                }
            }
        }

        let parent_dir = path.parent().unwrap_or(Path::new(""));
        let sce_module_dir = parent_dir.join("sce_module");

        // Load dependencies recursively
        for dep in needed {
            // First try same directory, then sce_module directory
            let dep_path = parent_dir.join(&dep);
            let dep_sce = sce_module_dir.join(&dep);

            if dep_path.exists() {
                if let Err(e) = self.load_module(&dep_path) {
                    tracing::warn!(dep = %dep, error = %e, "Failed to load dependency");
                }
            } else if dep_sce.exists() {
                if let Err(e) = self.load_module(&dep_sce) {
                    tracing::warn!(dep = %dep, error = %e, "Failed to load dependency from sce_module");
                }
            } else {
                tracing::debug!(dep = %dep, "Dependency not found on disk, will use HLE stubs");
            }
        }

        Ok(index)
    }

    /// Get a loaded module by index.
    pub fn get_module(&self, index: u32) -> Option<&Module> {
        self.modules.get(index as usize)
    }

    /// Get the main module (index 0).
    pub fn main_module(&self) -> Option<&Module> {
        self.modules.first()
    }

    /// Relocate all modules.
    pub fn relocate_all(&mut self) {
        tracing::info!("Relocating {} modules", self.modules.len());

        for i in 0..self.modules.len() {
            let base = self.modules[i].base_virtual_addr;
            let reloc_count = self.modules[i].relocations.len();

            tracing::debug!(
                module = %self.modules[i].name,
                base = format!("0x{:X}", base),
                relocations = reloc_count,
                "Applying relocations"
            );

            for j in 0..reloc_count {
                let relocation = self.modules[i].relocations[j].clone();
                relocation::apply_relocation(
                    &relocation,
                    base,
                    &self.hle_symbols,
                    &self.modules,
                );
            }
        }
    }

    /// Get the entry point of the main module.
    pub fn entry_point(&self) -> Option<u64> {
        self.main_module()
            .map(|m| m.base_virtual_addr + m.elf.entry_point())
    }

    /// Get the number of loaded modules.
    pub fn module_count(&self) -> usize {
        self.modules.len()
    }

    /// Retrieve DT_INIT function pointers from loaded modules.
    /// These must be called FIRST — they populate the .init_array.
    pub fn get_dt_init_funcs(&self) -> Vec<u64> {
        let mut funcs = Vec::new();
        for module in &self.modules {
            if module.init_addr > 0x1000 {
                tracing::info!("Found DT_INIT function: 0x{:X}", module.init_addr);
                funcs.push(module.init_addr);
            }
        }
        tracing::info!("DT_INIT functions found: {}", funcs.len());
        funcs
    }

    /// Retrieve init_array function pointers from loaded modules.
    /// Must be called AFTER DT_INIT has run, since DT_INIT populates .init_array.
    pub fn get_init_array_funcs(&self) -> Vec<u64> {
        let mut funcs = Vec::new();
        for module in &self.modules {
            tracing::info!(
                module = %module.name,
                init_addr = format!("0x{:X}", module.init_array_addr),
                init_size = module.init_array_size,
                "Scanning init_array after DT_INIT"
            );
            if module.init_array_addr > 0x1000 {
                unsafe {
                    // Debug dump: show first 16 entries at init_array_addr
                    tracing::info!("Dumping first 16 entries at init_array 0x{:X}:", module.init_array_addr);
                    for dbg_i in 0u64..16 {
                        let dbg_addr = module.init_array_addr + (dbg_i * 8);
                        let dbg_val = std::ptr::read_unaligned(dbg_addr as *const u64);
                        tracing::info!("  init_array[{}] @ 0x{:X} = 0x{:X}", dbg_i, dbg_addr, dbg_val);
                    }
                    
                    let mut i = 0;
                    loop {
                        // If size is known, use it as an upper bound
                        if module.init_array_size > 0 && (i * 8) >= module.init_array_size {
                            break;
                        }
                        
                        let ptr_addr = module.init_array_addr + (i * 8);
                        let func_ptr = std::ptr::read_unaligned(ptr_addr as *const u64);
                        
                        // If size is 0 (null-terminated array), break on the first 0
                        if func_ptr == 0 {
                            if module.init_array_size == 0 {
                                break;
                            }
                        } else if func_ptr > 0x1000 {
                            tracing::info!("Found init_array function: 0x{:X}", func_ptr);
                            funcs.push(func_ptr);
                        }
                        
                        i += 1;
                        // Hard limit to avoid infinite loops on corrupted memory
                        if i > 10000 {
                            tracing::warn!("init_array iteration reached hard limit!");
                            break;
                        }
                    }
                }
            }
        }
        tracing::info!("Total init_array functions found: {}", funcs.len());
        funcs
    }

    /// Dump info about all loaded modules.
    pub fn dump_modules(&self) {
        tracing::info!("=== Loaded Modules ({}) ===", self.modules.len());
        for (i, module) in self.modules.iter().enumerate() {
            tracing::info!(
                "[{}] {} @ 0x{:X} (size={})",
                i,
                module.name,
                module.base_virtual_addr,
                anotherps4_common::format_size(module.aligned_base_size),
            );
        }
    }
}
