//! anotherps4
//! ps4 emulator written in rust. 
//! boots some homebrew/games by natively running x86-64 code and hle-ing the os/gpu.

use anyhow::Result;
use clap::{Parser, Subcommand};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// anotherps4 - ps4 emu
#[derive(Parser)]
#[command(name = "anotherps4")]
#[command(version = "0.1.0")]
#[command(about = "wip ps4 emulator in rust")]
#[command(long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a PS4 executable (EBOOT.BIN or .elf)
    Run {
        /// Path to the PS4 executable
        path: PathBuf,

        /// Game directory (defaults to parent of executable)
        #[arg(short, long)]
        game_dir: Option<PathBuf>,

        /// Enable verbose logging
        #[arg(short, long)]
        verbose: bool,
    },

    /// Show information about a PS4 executable or game
    Info {
        /// Path to PARAM.SFO or EBOOT.BIN
        path: PathBuf,
    },
}

fn main() -> Result<()> {
    // Initialize logging
    anotherps4_common::logging::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            path,
            game_dir,
            verbose,
        } => {
            run_emulator(&path, game_dir.as_deref(), verbose)?;
        }
        Commands::Info { path } => {
            show_info(&path)?;
        }
    }

    Ok(())
}

// setup winit event loop and run the app
fn run_emulator(
    executable: &std::path::Path,
    game_dir: Option<&std::path::Path>,
    verbose: bool,
) -> Result<()> {
    // Determine game directory
    let game_directory = game_dir
        .map(|p| p.to_path_buf())
        .or_else(|| executable.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = AnotherPS4App {
        executable: executable.to_path_buf(),
        game_directory,
        verbose,
        window: None,
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}

struct AnotherPS4App {
    executable: PathBuf,
    game_directory: PathBuf,
    verbose: bool,
    window: Option<Arc<Window>>,
}

impl ApplicationHandler for AnotherPS4App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attributes = Window::default_attributes()
                .with_title("AnotherPS4 - Sonic Mania")
                .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
            
            let window = Arc::new(event_loop.create_window(attributes).expect("Failed to create window"));
            self.window = Some(window.clone());

            let executable = self.executable.clone();
            let game_directory = self.game_directory.clone();
            let verbose = self.verbose;
            let window_clone = window.clone();

            std::thread::spawn(move || {
                if let Err(e) = run_emulator_guest_thread(&executable, &game_directory, verbose, window_clone) {
                    tracing::error!("Emulator guest thread crashed: {}", e);
                }
            });
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::CloseRequested = event {
            event_loop.exit();
        }
    }
}

/// Run the emulator's core components on a guest thread.
fn run_emulator_guest_thread(
    executable: &std::path::Path,
    game_directory: &std::path::Path,
    _verbose: bool,
    _window: Arc<Window>,
) -> Result<()> {
    tracing::info!("╔═══════════════════════════════════════════╗");
    tracing::info!("║     AnotherPS4 — PS4 Emulator v0.1.0     ║");
    tracing::info!("║         Written in Rust 🦀                ║");
    tracing::info!("╚═══════════════════════════════════════════╝");
    tracing::info!("");

    // Validate executable exists
    if !executable.exists() {
        anyhow::bail!("Executable not found: {}", executable.display());
    }

    tracing::info!(executable = %executable.display(), "Loading executable");
    tracing::info!(game_dir = %game_directory.display(), "Game directory");

    // ── Step 1: Try to load PARAM.SFO for game info ──
    let sfo_path = game_directory.join("sce_sys").join("param.sfo");
    if sfo_path.exists() {
        match anotherps4_formats::psf::Psf::open(&sfo_path) {
            Ok(psf) => {
                if let Some(title) = psf.title() {
                    tracing::info!(title, "Game title");
                }
                if let Some(title_id) = psf.title_id() {
                    tracing::info!(title_id, "Title ID");
                }
                if let Some(version) = psf.app_version() {
                    tracing::info!(version, "App version");
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to parse PARAM.SFO");
            }
        }
    }

    // ── Step 2: Initialize Memory Manager ──
    tracing::info!("Initializing memory manager...");
    let memory = Arc::new(Mutex::new(
        anotherps4_core::memory::MemoryManager::new()?,
    ));

    // ── Step 3: Initialize Kernel Subsystems ──
    tracing::info!("Initializing kernel subsystems...");
    anotherps4_core::kernel::time::init();

    let _filesystem = anotherps4_core::kernel::filesystem::FileSystem::new();
    let _thread_manager = anotherps4_core::kernel::threading::ThreadManager::new();
    let _equeue_manager = anotherps4_core::kernel::equeue::EventQueueManager::new();

    // ── Step 4: Initialize Linker & Register HLE Libraries ──
    tracing::info!("Initializing dynamic linker...");
    let mut linker = anotherps4_core::linker::Linker::new(memory.clone());

    tracing::info!("Initializing stub trampoline region...");
    anotherps4_core::hle::stubs::init()?;

    tracing::info!("Registering HLE libraries...");
    anotherps4_core::hle::libraries::register_all_libraries(linker.hle_symbols_mut());
    
    tracing::info!("Initializing HLE Exports Dispatcher...");
    anotherps4_core::hle::exports::initialize_exports(memory.clone());
    
    tracing::info!("Mounting /app0/ filesystem...");
    anotherps4_core::hle::exports::mount_filesystem("/app0/", &game_directory);

    // ── Step 5: Initialize GPU ──
    tracing::info!("Initializing GPU subsystems...");
    let _gpu = anotherps4_gpu::liverpool::command_processor::CommandProcessor::new();
    // Pass window to init_gpu
    anotherps4_gpu::init_gpu(Some(_window.clone()));
    let _shader = anotherps4_gpu::shader::ShaderRecompiler::new();

    // ── Step 6: Initialize Audio & Input (stubs) ──
    let _audio = anotherps4_audio::AudioOutput::new();
    let _input = anotherps4_input::ControllerManager::new();

    // ── Step 7: Load the Executable ──
    tracing::info!("Loading executable into memory...");
    let _module_index = linker.load_module(executable)?;

    // Show loaded module info
    linker.dump_modules();

    // ── Step 8: Apply Relocations ──
    tracing::info!("Applying relocations...");
    linker.relocate_all();

    // ── Step 9: Report Entry Point ──
    if let Some(entry) = linker.entry_point() {
        tracing::info!(entry = format!("0x{:016X}", entry), "Entry point resolved");
    }

    // ── Step 10: Dump Memory Map ──
    {
        let mem = memory.lock();
        mem.dump_vma_map();
    }

    // ── Phase 2: Native Execution Setup ──
    tracing::info!("═══════════════════════════════════════════");
    tracing::info!("  Phase 1 Complete. Initiating Phase 2!");
    tracing::info!("═══════════════════════════════════════════");

    // Initialize the Syscall Interceptor (SIGILL)
    tracing::info!("Initializing Syscall Interceptor...");
    anotherps4_core::hle::dispatcher::initialize_syscall_interceptor();

    // Patch executable segments (Replace SYSCALL with UD2)
    {
        let mem = memory.lock();
        let vmas = mem.get_vmas();
        let code_vma = vmas.iter().find(|vma| vma.name == "eboot.bin" && vma.vma_type == anotherps4_core::memory::VMAType::Code).unwrap();
        let base = code_vma.base as *mut u8;
        let size = code_vma.size as usize;
        tracing::info!(base = format!("0x{:X}", code_vma.base), size, "Patching syscalls in binary...");
        unsafe {
            let patched = anotherps4_core::hle::dispatcher::patch_syscalls(base, size);
            tracing::info!(patched, "Syscall instructions replaced with UD2");
        }
    }

    // Allocate TCB (Thread Control Block) / TLS
    tracing::info!("Allocating Thread Control Block (TCB)...");
    let tcb_size = 4096; // Basic size for now
    let tcb_base = {
        let mut mem = memory.lock();
        mem.map_memory(
            0,
            tcb_size,
            anotherps4_core::memory::MemoryProt::CPU_READ | anotherps4_core::memory::MemoryProt::CPU_WRITE,
            anotherps4_core::memory::MemoryMapFlags::ANONYMOUS | anotherps4_core::memory::MemoryMapFlags::PRIVATE,
            anotherps4_core::memory::VMAType::ThreadData,
            "Main Thread TCB",
        )?
    };

    // Allocate Stack for Main Thread
    let stack_size = 2 * 1024 * 1024; // 2MB typical PS4 stack
    let stack_base = {
        let mut mem = memory.lock();
        mem.map_memory(
            0,
            stack_size,
            anotherps4_core::memory::MemoryProt::CPU_READ | anotherps4_core::memory::MemoryProt::CPU_WRITE,
            anotherps4_core::memory::MemoryMapFlags::ANONYMOUS | anotherps4_core::memory::MemoryMapFlags::PRIVATE,
            anotherps4_core::memory::VMAType::ThreadData,
            "Main Thread Stack",
        )?
    };

    let stack_end = stack_base + stack_size;
    
    // Set up standard SystemV AMD64 Stack (argc, argv, envp)
    // We write downwards from stack_end
    let mut rsp = stack_end;
    unsafe {
        // Push 0 (Auxv terminator)
        rsp -= 8; std::ptr::write(rsp as *mut u64, 0);
        // Push 0 (Envp terminator)
        rsp -= 8; std::ptr::write(rsp as *mut u64, 0);
        // Push 0 (Argv terminator)
        rsp -= 8; std::ptr::write(rsp as *mut u64, 0);
        // Push fake arg pointer (would naturally be the string address, using 0 for safety)
        rsp -= 8; std::ptr::write(rsp as *mut u64, 0);
        // Push argc (0)
        rsp -= 8; std::ptr::write(rsp as *mut u64, 0);

        // Align RSP strictly to 16 bytes (required by ABI before function entry)
        rsp &= !0xF;
    }

    let entry_point = linker.entry_point().unwrap();
    let dt_init_funcs = linker.get_dt_init_funcs();
    
    // We must leak the Vec BEFORE we change FS_BASE,
    // because dropping a Vec calls `free`, which accesses Host TLS!
    let dt_init_ptr = dt_init_funcs.as_ptr();
    let dt_init_len = dt_init_funcs.len();
    std::mem::forget(dt_init_funcs);

    // CRITICAL: We change the Host's FS_BASE to the Guest's TCB.  
    // From this point onwards, NO RUST MALLOCS, NO HOST TLS OR PRINTING ALLOWED!
    unsafe {
        // Fetch current Host FS Base
        let mut host_fs_base: u64 = 0;
        let p_host_fs_base = &mut host_fs_base as *mut u64;
        // ARCH_GET_FS = 0x1003
        if libc::syscall(libc::SYS_arch_prctl, 0x1003, p_host_fs_base) == 0 {
            anotherps4_core::hle::dispatcher::register_thread_fs(host_fs_base);
        }

        // Write TCB self-pointer at FS:[0]
        let tcb_ptr = tcb_base as *mut u64;
        std::ptr::write(tcb_ptr, tcb_base);

        tracing::info!(
            tcb_base = format!("0x{:X}", tcb_base),
            entry = format!("0x{:016X}", entry_point),
            stack_top = format!("0x{:X}", rsp),
            "Executing native jump into guest code! (TLS is swapping)"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));

        // ARCH_SET_FS = 0x1002
        if libc::syscall(libc::SYS_arch_prctl, 0x1002, tcb_base) != 0 {
            // Panic via abort since tls is broken
            std::process::abort();
        }

        // NOTE: DT_INIT and init_array execution is currently DISABLED.
        // The DT_INIT function at 0x20 calls static constructors that reference
        // unresolved NID symbols from PRX system modules (libSceGnmDriver, etc.).
        // These stubs return 0, which the constructors dereference, causing SIGSEGV.
        // TODO: Implement PRX module loading to properly resolve these dependencies,
        //       then re-enable static initializer execution.
        //
        // for i in 0..dt_init_len {
        //     let func = *dt_init_ptr.add(i);
        //     std::arch::asm!(...);
        // }

        // IGNITION: Transfer control from Emulator Host Context to Game Guest Executable!
        // This function never returns.
        anotherps4_core::cpu::jump::jump_to_guest(entry_point, rsp);
    }

    // Unreachable, jump_to_guest never returns
    // Ok(())
}

/// Show information about a PS4 file.
fn show_info(path: &std::path::Path) -> Result<()> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "sfo" => {
            let psf = anotherps4_formats::psf::Psf::open(path)?;
            println!("{}", psf);
        }
        "bin" | "elf" | "self" => {
            let elf = anotherps4_core::loader::elf::Elf::open(path)?;
            println!("{}", elf);
        }
        _ => {
            // Try as ELF first, then SFO
            if let Ok(elf) = anotherps4_core::loader::elf::Elf::open(path) {
                println!("{}", elf);
            } else if let Ok(psf) = anotherps4_formats::psf::Psf::open(path) {
                println!("{}", psf);
            } else {
                anyhow::bail!("Unknown file format: {}", path.display());
            }
        }
    }

    Ok(())
}
