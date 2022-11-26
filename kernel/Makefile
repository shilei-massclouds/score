#
# Copyright (c) 2022 Shi Lei
#
# Use of this source code is governed by a MIT-style license
# that can be found in the LICENSE file or
# at https://opensource.org/licenses/MIT
#

.PHONY: all clean run build asm qemu TARGET FORCE

KERNEL = target/riscv64/release/kernel
TARGET = target/riscv64/release/kernel.bin

OBJCOPY = riscv64-linux-gnu-objcopy
OBJDUMP = riscv64-linux-gnu-objdump
READELF = riscv64-linux-gnu-readelf

all: build

build: $(TARGET)

$(TARGET): $(KERNEL)
	@$(OBJCOPY) $^ -S -O binary $@

$(KERNEL): FORCE
	@cargo xbuild --target ../scripts/riscv64.json --release

clean:
	@-rm $(TARGET) $(KERNEL)
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
