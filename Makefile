CARGO_FILES = Cargo.toml image/Cargo.toml kernel/Cargo.toml search/Cargo.toml Cargo.lock
BUILD_FILES = $(CARGO_FILES) kernel/src/*.rs image/build.rs search/src/*.rs
CPU_MODEL ?= host
FEATURES ?= msr

# Build cargo feature flags
ifeq ($(FEATURES),)
  CARGO_FEATURES = --no-default-features
else
  CARGO_FEATURES = --no-default-features --features $(FEATURES)
endif

.PHONY: all
all: test bios.img

.PHONY: test
test:
	cargo test --workspace --exclude kernel

# Note: run 'make clean' when changing FEATURES
.PHONY: bios.img
bios.img:
	cargo build -p image --release $(CARGO_FEATURES) && \
    cp $$(ls -t target/release/build/image-*/out/bios.img | head -1) $@

.PHONY: run
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

.PHONY: clean
clean:
	cargo clean
