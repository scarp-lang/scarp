#!/usr/bin/env bash
# Host dependencies: Cargo, GNU binutils, GRUB BIOS tools, xorriso and QEMU.
set -euo pipefail
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd -- "$here/../.." && pwd)"
for tool in cargo as ld grub-file grub-mkrescue xorriso qemu-system-x86_64 timeout; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf 'Missing dependency: %s\n' "$tool" >&2
        exit 2
    fi
done
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
mkdir -p "$work/iso/boot/grub"
cd "$root"
cargo run --locked --example checked_elf64_function -- "$work/scarp.o"
as --64 -o "$work/boot.o" "$here/boot.S"
ld -m elf_x86_64 -z max-page-size=0x1000 --fatal-warnings \
    -T "$here/kernel.ld" -o "$work/iso/boot/kernel.elf" \
    "$work/boot.o" "$work/scarp.o"
grub-file --is-x86-multiboot2 "$work/iso/boot/kernel.elf"
cat > "$work/iso/boot/grub/grub.cfg" <<'GRUB'
set timeout=0
set default=0
menuentry "Scarp bare-metal smoke" {
    multiboot2 /boot/kernel.elf
    boot
}
GRUB
grub-mkrescue -o "$work/scarp.iso" "$work/iso"
status=0
timeout 45s qemu-system-x86_64 -accel tcg -cpu qemu64 -m 128M \
    -display none -serial none -monitor none -no-reboot -no-shutdown \
    -device isa-debug-exit,iobase=0xf4,iosize=4 \
    -boot d -cdrom "$work/scarp.iso" || status=$?
case "$status" in
    33) echo 'PASS: Scarp-generated function ran in bare-metal x86-64 mode.' ;;
    35) echo 'FAIL: boot magic or Scarp ABI/result check failed.' >&2; exit 1 ;;
    124) echo 'FAIL: QEMU timed out before reporting a result.' >&2; exit 1 ;;
    *) printf 'FAIL: unexpected QEMU status %s\n' "$status" >&2; exit 1 ;;
esac
