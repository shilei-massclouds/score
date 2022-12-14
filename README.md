# score
A simple OS in rust

### Entry

1. Disable to link to std

    #![no_std]

2. Disable _main_

    #![no_main]

3. Disable to rename function name (now for _main_)

    #[no_mangle]

4. Provide panic handler

    #[panic_handler]

5. Disable **stack unwinding**

    Add profile into Cargo.toml
    ```sh
    [profile.dev]
    panic = "abort"

    [profile.release]
    panic = "abort"
    ```

6. Discover and install build-target for RiscV64
    ```sh
    rustup target list | grep riscv64  
    rustup target add riscv64gc-unknown-none-elf
    ```

    Explanation for riscv64gc-unknown-none-elf:  
    arch: riscv64;  
    compiler: gcc;  
    vendor: unknown  
    OS: none (means on baremetal)  
    abi: elf

7. Build target (cross compile)
    ```sh
    cargo run --target riscv64gc-unknown-none-elf
    ```

### LDScript (kernel.ld)

1. Use self-defining ldscript

    Add .cargo/config to override the default ld script.
    ```asm
    [target.riscv64gc-unknown-none-elf]
    rustflags = [
        "-C", "link-arg=-Tkernel.ld",
    ]
    ```

2. Base Link Address
    ```asm
    /* Beginning of entry code segment */
    . = KERNEL_BASE;
    _start = .;
    
    ```

    KERNEL_BASE is the virtual space address for kernel image itself.  
    After turning on mmu, map kernel image into this address.  

3. Sections

    There are five major sections:
    - .text.entry

        startup code which must be the headmost.

    - .text:

        code.

    - .rodata

        read-only data and srodata.

    - .data

        read-write data and sdata.

    - .bss

        _bss_ and _sbss_.  
        At the beginning of _bss_, we alloc two areas for boot stack and  
        boot heap.
        ```asm
        .bss : AT(ADDR(.bss) - KERNEL_BASE) {
            _boot_stack = .;
            . += CONFIG_STACK_SIZE;
            _boot_stack_top = .;
            _boot_heap = .;
            . += CONFIG_BOOT_HEAP_SIZE;
            _boot_heap_end = .;
        }
        ```

### StdOut with Mutex

1. StdOut is simply based on sbi console_put_char.

2. As a global variable, StdOut should be protected by Mutex.  
   Now there is no concepts of thread and block, so just  
   introduce a spinlock called spin::Mutex.

    ```RUST
    pub static STDOUT: Mutex<StdOut> = Mutex::new(StdOut);
   
    /* use it by a guard */
    STDOUT.lock().puts("Hello\n");
    ```

> __NOTICE__:  
> Now we still cannot implement println!, since MMU hasn't been enabled.  
> Those print stuff must parse Arguments by crate alloc::format;  
> and this crate is backed by compiler.  
> Surprisingly, compiler generates code is based on LinkAddress-Relative  
> rather than PC-Relative.  
>
> E.g. If we NOW call _println!_ or even _format!_ directly, we will  
> run up against a fault.  
>
> Trace Qemu: "riscv\_cpu\_\do\_interrupt[target/riscv/cpu\_helper.c]".  
> Check csrs:  
>   _scause_: RISCV\_EXCP_\INST\_ACCESS\_FAULT;  
>   _tval_(badaddr): like 0xffffffff0000XXXX;  
>
> Obviously, code in alloc::format accesses virtual aspace address!

### Enable mmu

1. Setup swapper_pgd

   Ignore trampoline, just setup swapping.

2. Detect Paging Mode

   Choose SVxx according to KERNEL_ASPACE_BITS.

3. Relocate to virtual address space

   Use stvec to implement relocate.

### Println! & Panic!

1. Copy macro println! into this project
2. Implement _print based on sbi putchar
3. Call println! in Panic! to display information
4. Panic! ends with power_off which is also based on sbi extention SRST
