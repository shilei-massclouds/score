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
    ```asm
    "linker": "rust-lld",
    "linker-flavor": "ld.lld",
    "pre-link-args": {
        "ld.lld": ["-Tkernel.ld"]
    },
    ```
    
    In scripts/riscv64.json, we specify our self-defining ldscript.
    
2. Base Link Address
    ```asm
    /* Beginning of entry code segment */
    . = KERNEL_BASE;
    _start = .;

    ```

    KERNEL_BASE is the virtual space address for kernel image itself.
    After turning on mmu, map kernel image into this address.
    But in fact, we compile kernel based on Model __medany__, so all
    kernel instructions are PC-relative and they never depend on this address.
    
3. Sections

    There are five major sections:
    - .text.entry
        startup code which must be the headmost;
    - .text:
        code
    - .rodata
        readonly data and srodata;
    - .data
        read-write data and sdata;
    - .bss
        bss and sbss;
