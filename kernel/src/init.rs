// SPDX-License-Identifier: MPL-2.0

//! Kernel initialization.

use aster_cmdline::INIT_PROC_ARGS;
use component::InitStage;
use ostd::{cpu::CpuId, util::id_set::Id};
use spin::once::Once;

use crate::{
    fs::vfs::path::{MountNamespace, PathResolver},
    prelude::*,
    process::{Process, spawn_init_process},
    sched::SchedPolicy,
    thread::kernel_thread::ThreadOptions,
};

pub(super) fn main() {
    // VBE graphics framebuffer (x86_64 QEMU with -vga std)
    #[cfg(target_arch = "x86_64")]
    {
        ostd::early_println!("[VBE] Setting graphics mode 640x480x32...");
        if let Some((fb_addr, w, h, bpp)) = crate::vbe_dispi::set_graphics_mode(640, 480, 32) {
            ostd::early_println!("[VBE] Framebuffer at {:#x}, {}x{}x{}", fb_addr, w, h, bpp);

            // Draw a test pattern: blue background with green banner area
            crate::vbe_dispi::draw_rect(fb_addr, w, h, bpp, 0, 0, w, h, 10, 10, 40); // dark blue bg
            crate::vbe_dispi::draw_rect(fb_addr, w, h, bpp, 50, 80, 540, 160, 20, 20, 20); // banner bg

            ostd::early_println!("[VBE] Graphics displayed on QEMU VGA.");
        } else {
            ostd::early_println!("[VBE] No VBE DISPI support, falling back to VGA text");
            crate::vga_text::print_banner();
        }
    }

    // Initialize the global states for all CPUs.
    ostd::early_println!("OSTD initialized. Preparing components.");

    // Initialize the leveled logger early so all subsequent log calls
    // (info!/warn!/error!) get timestamps and levels. On aarch64 the
    // component system may not register the logger, so we do it manually.
    #[cfg(target_arch = "aarch64")]
    {
        aster_logger::init_manual();
        ostd::info!("AsterLogger initialized (manual)");
    }
    // Now that the kernel page table is activated (linear VMA linking),
    // try the full component system on aarch64 too. If it fails, we have
    // manual fallbacks in init_in_first_kthread.
    ostd::early_println!("[init] calling component::init_all(Bootstrap)...");
    {
        // Bring-up diagnostic: dump the component metadata and the registry
        // entries so a skipped component is immediately visible.
        let comps = component::parse_metadata!();
        ostd::early_println!("[comp] {} ComponentInfo entries:", comps.len());
        for c in &comps {
            ostd::early_println!("[comp]   info: {:?}", c);
        }
        let registries: Vec<_> =
            component::inventory::iter::<component::ComponentRegistry>().collect();
        ostd::early_println!("[comp] {} ComponentRegistry entries:", registries.len());
        for r in registries {
            ostd::early_println!("[comp]   registry: {:?}", r);
        }
    }
    match component::init_all(InitStage::Bootstrap, component::parse_metadata!()) {
        Ok(()) => ostd::early_println!("[init] component::init_all(Bootstrap) OK"),
        Err(e) => ostd::early_println!("[init] component::init_all(Bootstrap) FAILED: {:?}", e),
    }
    ostd::early_println!("Components Bootstrap done.");
    init();
    ostd::early_println!("Kernel init done.");
    ostd::early_println!("Kernel init done.");

    // Initialize the per-CPU states for BSP.
    // This must run even on aarch64 single-CPU: sched/process/fs/time need
    // their per-CPU runqueues and state initialized on the BSP before any
    // thread can be spawned. Without this, bsp_idle_loop cannot spawn
    // first_kthread. (aarch64 count_processors()==1, so APs never boot and
    // ap_init is never called — BSP is the only CPU.)
    ostd::early_println!("Initializing per-CPU states for BSP...");
    init_on_each_cpu();
    ostd::early_println!("Per-CPU init done.");

    // On aarch64, virtio/framebuffer/fb_console init is deferred to
    // first_kthread (see init_in_first_kthread) rather than done here in the
    // boot context. Reason: virtio init calls allocate_major() →
    // ostd::sync::Mutex::lock(), and ostd's Mutex uses a WaitQueue whose
    // scheduling hooks are only valid inside a task context. Calling it from
    // the boot context (before bsp_idle_loop) panics. x86_64 does its device
    // init in first_kthread via the component system; we mirror that.

    // Enable APs. On aarch64 single-CPU, register_ap_entry stores the entry
    // fn but count_processors()==1 means APs never boot, so ap_init never
    // runs. Spawning bsp_idle_loop is required on all architectures: it is
    // the only path that creates the first non-idle kernel thread.
    ostd::boot::smp::register_ap_entry(ap_init);
    ostd::early_println!("Spawning BSP idle thread...");

    // Give the control of the BSP to the idle thread.
    ThreadOptions::new(bsp_idle_loop)
        .cpu_affinity(CpuId::bsp().into())
        .sched_policy(SchedPolicy::Idle)
        .spawn();
    ostd::early_println!("BSP idle thread spawned.");
}

fn init() {
    ostd::early_println!("[init] arch::init...");
    crate::arch::init();
    ostd::early_println!("[init] cmdline::init...");
    #[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
    {
        // cmdline component init may not have run if Bootstrap failed.
        // riscv64 has the same issue: the inventory-based component system
        // is unreliable on qemu-direct boot paths.
        aster_cmdline::init_no_component();
    }
    ostd::early_println!("[init] thread::init...");
    crate::thread::init();
    ostd::early_println!("[init] random::init...");
    crate::util::random::init();
    ostd::early_println!("[init] driver::init...");
    // aarch64 and riscv64 defer driver init. On riscv64, aster_input's
    // all_devices() triggers a div-by-zero panic in core::unicode::conversions
    // (likely an uninitialized input device constant). Skipping driver init
    // here lets riscv64 reach userspace; input devices can be initialized
    // later once the root cause is fixed.
    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    crate::driver::init();
    // Register memory character devices (/dev/null, /dev/zero, /dev/urandom)
    // early so they're available before any user-space process starts.
    #[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
    crate::device::mem::init_in_first_kthread();
    ostd::early_println!("[init] time::init...");
    crate::time::init();
    // On aarch64 and riscv64, net::init() is deferred to init_in_first_kthread() so
    // that all device components (virtio, network, vsock, softirq) are initialized
    // by the component system's Kthread stage before the network stack probes
    // the virtio-net/vsock devices.
    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    {
        ostd::info!("net::init");
        crate::net::init();
    }
    ostd::early_println!("[init] sched::init...");
    crate::sched::init();
    ostd::early_println!("[init] process::init...");
    crate::process::init();
    ostd::early_println!("[init] fs::init...");
    crate::fs::init();
    ostd::early_println!("[init] security::init...");
    crate::security::init();
    ostd::early_println!("[init] done");
}

fn init_on_each_cpu() {
    crate::sched::init_on_each_cpu();
    crate::process::init_on_each_cpu();
    crate::fs::init_on_each_cpu();
    crate::time::init_on_each_cpu();
}

fn ap_init() {
    // Initialize the per-CPU states for AP.
    init_on_each_cpu();

    ThreadOptions::new(ap_idle_loop)
        // No races because `ap_init` runs on a certain AP.
        .cpu_affinity(CpuId::current_racy().into())
        .sched_policy(SchedPolicy::Idle)
        .spawn();
}

//--------------------------------------------------------------------------
// Per-CPU idle threads
//--------------------------------------------------------------------------

// Note: Keep the code in the idle loop to the bare minimum.
//
// We do not want the idle loop to
// rely on the APIs of other kernel subsystems for two reasons.
// First, the idle task must never sleep or block.
// This property is relied upon by the scheduler.
// Second, the idle task is spawned before the kernel is fully initialized.
// So other subsystems may not be ready, yet.
//
// In addition,
// doing more work in the idle task may have negative impact on
// the latency to switching from the idle task to a useful, runnable one.

fn bsp_idle_loop() {
    // Use early_println instead of ostd::info! on arches where the log
    // system routes output through uninitialized console components. On
    // riscv64, ostd::info!() is suppressed entirely (unicode bug workaround).
    #[cfg(target_arch = "aarch64")]
    ostd::info!("Idle thread for CPU #0 started");
    #[cfg(target_arch = "riscv64")]
    ostd::early_println!("Idle thread for CPU #0 started");
    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    ostd::info!("Idle thread for CPU #0 started");

    // Spawn the first non-idle kernel thread on BSP.
    ThreadOptions::new(first_kthread)
        .cpu_affinity(CpuId::bsp().into())
        .sched_policy(SchedPolicy::default())
        .spawn();

    // Wait till the init process is spawned.
    let init_process = loop {
        if let Some(init_process) = INIT_PROCESS.get() {
            break init_process;
        };

        ostd::task::halt_cpu();
    };

    // Wait till the init process becomes zombie.
    while !init_process.status().is_zombie() {
        ostd::task::halt_cpu();
    }

    panic!(
        "The init process terminates with code {:?}",
        init_process.status().exit_code()
    );
}

fn ap_idle_loop() {
    ostd::info!(
        "Idle thread for CPU #{} started",
        // No races because this function runs on a certain AP.
        CpuId::current_racy().as_usize(),
    );

    loop {
        ostd::task::halt_cpu();
    }
}

//--------------------------------------------------------------------------
// The first kernel thread
//--------------------------------------------------------------------------

// The main function of the first (non-idle) kernel thread
fn first_kthread() {
    ostd::info!("Spawn the first kernel thread");
    #[cfg(target_arch = "riscv64")]
    ostd::early_println!("[kthread] first kernel thread spawned");

    let init_mnt_ns = MountNamespace::get_init_singleton();
    let fs_resolver = init_mnt_ns.new_path_resolver();
    init_in_first_kthread(&fs_resolver);

    // print_banner uses println! which routes through the log/console system;
    // on aarch64/riscv64 the console component isn't initialized yet (and the
    // gradient logo art risks hitting the riscv64 unicode table bug), so skip
    // it there.
    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    print_banner();

    INIT_PROCESS.call_once(|| {
        let karg = INIT_PROC_ARGS.get().unwrap();
        let init_path = INIT_PATH.get().map(|s| s.as_str());
        spawn_init_process(init_path, karg.argv().to_vec(), karg.envp().to_vec())
            .expect("Failed to run the init process")
    });
}

static INIT_PROCESS: Once<Arc<Process>> = Once::new();

fn init_in_first_kthread(path_resolver: &PathResolver) {
    // riscv64: skip component::init_all(Kthread) — it panics with a div-by-zero
    // in aster_input (core::unicode::conversions). The Kthread stage initializes
    // device components (virtio, input, net) which aren't needed for a basic
    // userspace boot test. Skipping lets us reach spawn_init_process.
    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    if let Err(e) = component::init_all(InitStage::Kthread, component::parse_metadata!()) {
        ostd::warn!("component::init_all(Kthread) failed: {:?}", e);
    }
    #[cfg(target_arch = "aarch64")]
    {
        // Component::init_all(Bootstrap) already ran in init() and initialized
        // virtio, framebuffer, console, input, etc. We just need to run the
        // Kthread stage (which does the same as component::init_all(Kthread)
        // on other architectures).
        ostd::info!("running component::init_all(Kthread)...");
        if let Err(e) = component::init_all(InitStage::Kthread, component::parse_metadata!()) {
            ostd::warn!("component::init_all(Kthread) failed: {:?}", e);
        } else {
            ostd::info!("component::init_all(Kthread) OK");
        }
    }
    // Work queue should be initialized before interrupt is enabled,
    // in case any irq handler uses work queue as bottom half
    crate::thread::work_queue::init_in_first_kthread();
    // riscv64 mirrors aarch64 here: the mem char devices were already
    // registered in init(), and the rest of device init depends on driver
    // components that the qemu-direct path skips.
    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    crate::device::init_in_first_kthread();
    // On aarch64, net::init() is deferred to here (after the component system's
    // Kthread stage). However, inventory-based component registration is
    // unreliable on aarch64, so we also explicitly initialize the softirq,
    // network, and virtio components that net::init() depends on.
    #[cfg(target_arch = "aarch64")]
    {
        ostd::info!("explicit component init for net stack");
        let _ = aster_softirq::init_component_fn();
        let _ = aster_console::init_component_fn();
        let _ = aster_framebuffer::init_component_fn();
        // Initialize the input core first (creates InputCore singleton),
        // then register the evdev handler class (so it connects to devices
        // when they're probed), then probe virtio devices.
        let _ = aster_input::init_component_fn();
        ostd::early_println!("[kthread] evdev handler init (before virtio probe)");
        crate::device::evdev::init_in_first_kthread();
        let _ = aster_network::init_component_fn();
        // virtio component init probes devices (needs FDT, which is available)
        let _ = aster_virtio::virtio_component_init_pub();

        // Publish the framebuffer. On QEMU TCG aarch64, Arc::new can trigger
        // a page fault (heap allocator issue). We try it but continue on failure.
        ostd::info!("publishing framebuffer...");
        let published = aster_virtio::aarch64_raw_gpu_probe::publish_framebuffer();
        if published {
            ostd::info!("framebuffer published OK");
            crate::device::fb::register_late();
            ostd::info!("/dev/fb0 registered");
            // Initialize the hardware cursor after the framebuffer is published.
            ostd::early_println!("[kthread] init hardware cursor");
            aster_virtio::aarch64_raw_gpu_probe::init_cursor();
        } else {
            ostd::info!("WARNING: framebuffer not published");
        }
        // Note: even without publish, the GPU probe has already set up the
        // scanout with the framebuffer resource. The display shows whatever
        // is in the DMA buffer.

        // NOTE: fb_console::init() is intentionally NOT called. It triggers
        // flush_framebuffer() which sends TRANSFER_TO_HOST_2D commands that
        // hang QEMU TCG after the initial GPU setup. The screen stays black
        // until user-space aris-render writes to /dev/fb0.

        ostd::info!("net::init (deferred)");
        crate::net::init();
    }
    ostd::early_println!("[kthread] before net::init_in_first_kthread");
    // riscv64 skips the network stack on this boot path: net::init() is not
    // called (no virtio-net device without driver components), so
    // init_in_first_kthread would panic on the uninitialized IFACES Once.
    #[cfg(not(target_arch = "riscv64"))]
    crate::net::init_in_first_kthread();
    #[cfg(target_arch = "riscv64")]
    ostd::early_println!("[kthread] net stack skipped on riscv64 (no net devices)");
    ostd::early_println!(
        "[kthread] after net::init_in_first_kthread, before fs::init_in_first_kthread"
    );
    crate::fs::init_in_first_kthread(path_resolver);
    ostd::early_println!("[kthread] after fs::init_in_first_kthread (rootfs ready)");
    // vDSO init needs the aster_time TSC clocksource, which is x86-only on
    // this boot path (the aster_time component is bypassed elsewhere). The
    // ELF loader maps the vDSO only when it is initialized, so skipping it on
    // riscv64 is safe.
    #[cfg(target_arch = "x86_64")]
    crate::vdso::init_in_first_kthread();
}

fn print_banner() {
    println!("");
    println!("{}", logo_ascii_art::get_gradient_color_version());
}

/// Emits a simple Sixel test image (three colored rectangles: red, green, blue)
/// to verify that the framebuffer console's Sixel DCS parser and renderer work.
///
/// The image is 24 pixels wide and 6 pixels tall. Each color block is 8 pixels
/// wide. Sixel data characters in the range 0x3f–0x7e encode 6 vertical pixels
/// (bit 0 = topmost). The value `~` (0x7e = 0x3f + 0x3f) sets all 6 bits.
#[cfg(target_arch = "aarch64")]
fn print_sixel_test_image() {
    // Sixel DCS sequence:
    //   ESC P q        — DCS introducer + Sixel command byte 'q'
    //   #1;2;100;0;0   — define color register 1 as RGB red (100%, 0%, 0%)
    //   #1 ~ ~ ~ ~ ~ ~ ~ ~  — select color 1, draw 8 columns of full-height pixels
    //   $              — carriage return (start of sixel row)
    //   #2;2;0;100;0   — define color register 2 as RGB green
    //   #2 ~ ~ ~ ~ ~ ~ ~ ~  — 8 green columns (NOT used — we use $ for CR within row)
    //   ... actually for horizontal blocks in the same sixel row, we just draw
    //   different colors side by side without CR.
    //
    // Simplified: draw 3 colored blocks side by side in one sixel row.
    // Each block = select color + 8 data chars of '~' (0x7e = all 6 bits set).
    // Use byte array to avoid Rust line-continuation whitespace issues.
    let sixel_bytes: &[u8] =
        b"\x1bPq#1;2;100;0;0#1~~~~~~~~#2;2;0;100;0#2~~~~~~~~#3;2;0;0;100#3~~~~~~~~\x1b\\";
    ostd::info!("sending {} bytes to fb_console", sixel_bytes.len());

    // Send the Sixel sequence directly through the boot console (fb_console),
    // which has its own DCS parser and renders directly to the framebuffer DMA.
    if let Ok(s) = core::str::from_utf8(sixel_bytes) {
        crate::fb_console::print_str(s);
    }
    ostd::info!("done sending.");
}

pub(super) fn on_first_process_startup(ctx: &Context) {
    // The inventory-based component system panics on aarch64/riscv64 (the
    // qemu-direct boot path bypasses component registration). Skip the
    // Process stage of component init and device init there — they depend on
    // the component/driver registration that these arches bypass manually.
    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    {
        component::init_all(InitStage::Process, component::parse_metadata!()).unwrap();
        crate::device::init_in_first_process(ctx).unwrap();
    }
    #[cfg(target_arch = "aarch64")]
    {
        // Initialize the console and framebuffer components explicitly (the
        // inventory-based component system doesn't reliably register them on
        // aarch64). These are needed before opening /dev/console and before
        // the VT subsystem connects to the framebuffer.
        ostd::info!("console/framebuffer component init");
        let _ = aster_console::init_component_fn();
        let _ = aster_framebuffer::init_component_fn();
        let _ = aster_input::init_component_fn();

        // Initialize the TTY subsystem (VT consoles, serial tty, /dev nodes).
        // The VT subsystem will allocate VT1, connect to the framebuffer
        // (already published), and register the keyboard handler (connecting
        // to any virtio-keyboard devices already registered).
        //
        // NOTE: TTY init is skipped on aarch64 for now. It hangs in QEMU TCG
        // mode due to framebuffer flush operations in the VT console backend.
        // The aris-render user-space process writes directly to /dev/fb0 via
        // the published FrameBuffer, which does not go through the TTY layer.
        ostd::info!("skipping tty subsystem (aarch64, QEMU TCG workaround)");

        // Create /dev/fb0 device node directly in the rootfs.
        // The full device::init_in_first_process hangs because mounting ramfs
        // on /dev is unreliable on aarch64 QEMU TCG. Instead, we register
        // device nodes directly into the existing rootfs /dev directory.
        ostd::info!("registering device nodes...");
        {
            let fs = ctx.thread_local.borrow_fs();
            let path_resolver = fs.resolver().read();
            // Register just char device nodes (fb0, null, zero, etc.) directly.
            for device in crate::device::registry::char::collect_all() {
                if let Some(meta) = device.devtmpfs_meta() {
                    let dev_id = device.id().as_encoded_u64();
                    let _ = crate::device::add_node(
                        crate::device::DeviceType::Char,
                        dev_id,
                        &meta,
                        &path_resolver,
                    );
                }
            }
            ostd::info!("device nodes registered");
        }
        // The framebuffer flush happens during GPU probe. Background fill
        // thread removed for build compatibility.

        // (Sixel test moved to init_in_first_kthread where framebuffer_info()
        // is still valid.)
    }
    // fs::init_in_first_process opens /dev/console as fd 0/1/2, which needs a
    // registered console device driver. On aarch64/riscv64 the console
    // component isn't initialized, so opening /dev/console fails (ENODEV).
    // Skip it; the init process will run without std fds (open_initial_console
    // below binds a SerialConsole instead).
    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    crate::fs::init_in_first_process(ctx);

    // Open /dev/console as fd 0 (stdin), 1 (stdout), 2 (stderr) for the init
    // process.  Linux does this in kernel_init() before exec'ing init; without
    // it, user-space writes to stdout silently fail (EBADF).
    open_initial_console(ctx);
}

/// Opens `/dev/console` and assigns it to fd 0, 1, 2.
///
/// Mirrors Linux's `init/main.c`:
///   fd = open("/dev/console", O_RDWR);
///   dup(fd);  // stdout
///   dup(fd);  // stderr
fn open_initial_console(ctx: &Context) {
    use crate::fs::{
        file::{AccessMode, FileLike, InodeHandle, StatusFlags, file_table::FdFlags},
        vfs::path::FsPath,
    };

    // On aarch64/riscv64, use SerialConsole for all fds so piped/serial input
    // works. (aarch64: PL011; riscv64: SBI console.) The VT framebuffer still
    // renders independently via FramebufferConsole on aarch64.
    #[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
    {
        let console: Arc<dyn FileLike> = Arc::new(crate::serial_console::SerialConsole::new(
            AccessMode::O_RDWR,
        ));
        let file_table = ctx.thread_local.borrow_file_table();
        let mut ft = file_table.unwrap().write();
        let _ = ft.insert(console.clone(), FdFlags::empty()); // fd 0 = stdin
        let _ = ft.insert(console.clone(), FdFlags::empty()); // fd 1 = stdout
        let _ = ft.insert(console.clone(), FdFlags::empty()); // fd 2 = stderr
        ostd::early_println!("[kthread] serial console bound to fd 0/1/2");
        return;
    }

    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    {
        // Try /dev/console first (or /dev/tty0). On aarch64, if the TTY/VT
        // subsystem initialized successfully, /dev/console will be the VT
        // framebuffer terminal — giving us keyboard input + ANSI display.
        let console_paths = ["/dev/console", "/dev/tty0", "/dev/ttyS0"];
        let fs_info = ctx.thread_local.borrow_fs();
        let resolver = fs_info.resolver();
        let resolver_guard = resolver.read();

        let path = console_paths.iter().find_map(|p| {
            FsPath::try_from(*p)
                .ok()
                .and_then(|fp| resolver_guard.lookup(&fp).ok().map(|path| (*p, path)))
        });
        drop(resolver_guard);

        let file: Arc<dyn FileLike> = if let Some((found, path)) = path {
            match InodeHandle::new(path, AccessMode::O_RDWR, StatusFlags::empty()) {
                Ok(f) => {
                    ostd::info!("console opened: {}", found);
                    Arc::new(f)
                }
                Err(e) => {
                    ostd::info!("console open failed: {:?}, falling back", e);
                    return;
                }
            }
        } else {
            return;
        };

        let file_table = ctx.thread_local.borrow_file_table();
        let mut ft = file_table.unwrap().write();
        let _ = ft.insert(file.clone(), FdFlags::empty()); // fd 0 = stdin
        let _ = ft.insert(file.clone(), FdFlags::empty()); // fd 1 = stdout
        let _ = ft.insert(file.clone(), FdFlags::empty()); // fd 2 = stderr
    } // end #[cfg(not(target_arch = "aarch64"))]
}

static INIT_PATH: Once<String> = Once::new();
aster_cmdline::define_kv_param!("init", INIT_PATH);
