#
# Copyright (c) 2022 Shi Lei
#
# Use of this source code is governed by a MIT-style license
# that can be found in the LICENSE file or
# at https://opensource.org/licenses/MIT
#

.PHONY: all clean run build asm qemu FORCE

BUILDDIR := target/riscv64gc-unknown-none-elf/release/
KERNEL := $(BUILDDIR)/kernel
TARGET := $(BUILDDIR)/kernel.bin

CROSS_COMPILE := riscv64-linux-gnu-

OBJCOPY = $(CROSS_COMPILE)objcopy
OBJDUMP = $(CROSS_COMPILE)objdump
READELF = $(CROSS_COMPILE)readelf
NM = $(CROSS_COMPILE)nm

all: build

build: $(TARGET)
	@ctags -R .

$(TARGET): $(KERNEL)
	@$(OBJCOPY) $^ -S -R .eh_frame -R .note -R .comment -O binary $@

$(KERNEL): FORCE
	@cargo build --target riscv64gc-unknown-none-elf --release
	#-Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem
	@$(NM) -n $@ | grep -v '\( [aNUw] \)\|\(__crc_\)\|\( \$[adt]\)\|\( \.L\)' > System.map

clean:
	@-rm -f $(TARGET) $(KERNEL)
	@-cargo clean

run: build qemu

asm:
	$(OBJDUMP) -d $(KERNEL) | less

elf:
	$(READELF) -WS $(KERNEL)

qemu:
	@ cd ../../qemu && ./sstart.sh && cd -

PHONY += FORCE
FORCE:
