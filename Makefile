CARGO_FILES = Cargo.toml image/Cargo.toml kernel/Cargo.toml Cargo.lock
BUILD_FILES = $(CARGO_FILES) kernel/src/*.rs image/build.rs
CPU_MODEL ?= host

.PHONY:
all: bios.img

.PHONY:
bios.img: $(BUILD_FILES)
	cargo build -p image --release && \
    cp $$(ls -t target/release/build/image-*/out/bios.img | head -1) $@

.PHONY:
run: bios.img
	qemu-system-x86_64 \
		-cpu $(CPU_MODEL) \
		-nographic \
		-no-reboot \
		-drive format=raw,file=$(CURDIR)/bios.img \
		-accel kvm \
		-smp cpus=1 \
		-m 128M \
		-device isa-debug-exit,iobase=0xf4,iosize=0x04
