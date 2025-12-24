.PHONY:
all: bios.img

.PHONY:
bios.img: kernel/src/*.rs
	cargo build --release && \
    cp $$(ls -t target/release/build/basic-os-*/out/bios.img | head -1) $@

.PHONY:
run: bios.img
	qemu-system-x86_64 \
		-cpu qemu64 \
		-nographic \
		-no-reboot \
		-drive format=raw,file=$(CURDIR)/bios.img \
		-accel kvm \
		-smp cpus=1 \
		-m 128M \
		-device isa-debug-exit,iobase=0xf4,iosize=0x04
