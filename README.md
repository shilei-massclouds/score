# score
A simple OS in rust

Entry
=====

1. Disable to link to std
    #![no_std]

2. Disable _main_
    #![no_main]

3. Disable to rename function name (now for _main_)
    #[no_mangle]

4. Provide panic handler
    #[panic_handler]

5. Disable *stack unwinding*
Add profile into Cargo.toml

    [profile.dev]
    panic = "abort"
    [profile.release]
    panic = "abort"

6. Discover and install build-target for RiscV64
    rustup target list | grep riscv64
    rustup target add riscv64gc-unknown-none-elf

    arch: riscv64;
    compiler: gcc;
    vendor: unknown
    OS: none (means on baremetal)
    abi: elf

7. Build target (cross compile)
    cargo run --target riscv64gc-unknown-none-elf
