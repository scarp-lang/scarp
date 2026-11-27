#!/usr/bin/env bash
# Host dependencies: Cargo, GNU binutils, GRUB BIOS tools, xorriso, mtools and QEMU.
set -euo pipefail
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd -- "$here/../.." && pwd)"
for tool in cargo as ld objcopy grub-file grub-mkrescue xorriso mformat qemu-system-x86_64 timeout; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf 'Missing dependency: %s\n' "$tool" >&2
        exit 2
    fi
done
grub_platform="${SCARP_GRUB_PLATFORM:-/usr/lib/grub/i386-pc}"
if [[ ! -f "$grub_platform/modinfo.sh" ]]; then
    printf 'Missing GRUB BIOS platform: %s\n' "$grub_platform" >&2
    exit 2
fi
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
mkdir -p "$work/iso/boot/grub"
cd "$root"
cargo run --locked --example checked_elf64_function -- "$work/scarp.o"
cargo run --locked --example bare_metal_bounded -- "$work/bounded.o"
cargo run --locked --bin scarp -- native-object --json x86_64-none \
    bounded-entry evaluate 1000 512 examples/native_bounded_entry.srp -o "$work/direct-calls.o"
cargo run --locked --bin scarp -- native-object --json x86_64-none \
    bounded-entry entry 1000 512 examples/native_aggregate_locals.srp -o "$work/aggregates.o"
objcopy --redefine-sym scarp_bounded_integer_entry=scarp_bounded_aggregate_entry "$work/aggregates.o"
as --64 -o "$work/boot.o" "$here/boot.S"
ld -m elf_x86_64 -z max-page-size=0x1000 --fatal-warnings \
    -T "$here/kernel.ld" -o "$work/iso/boot/kernel.elf" \
    "$work/boot.o" "$work/scarp.o" "$work/bounded.o" "$work/direct-calls.o" "$work/aggregates.o"
grub-file --is-x86-multiboot2 "$work/iso/boot/kernel.elf"
cat > "$work/iso/boot/grub/grub.cfg" <<'GRUB'
serial --unit=0 --speed=115200
terminal_input serial
terminal_output serial
echo 'Loading Scarp Multiboot2 image'
if multiboot2 /boot/kernel.elf; then
    echo 'Handing control to Scarp bootstrap'
    boot
fi
echo 'GRUB could not start the Scarp image'
halt
GRUB
grub-mkrescue -d "$grub_platform" -o "$work/scarp.iso" "$work/iso"
status=0
timeout 45s qemu-system-x86_64 -accel tcg -cpu qemu64 -m 128M \
    -display none -serial stdio -monitor none -no-reboot \
    -debugcon "file:$work/boot-markers.log" \
    -d int,cpu_reset,guest_errors -D "$work/qemu.log" \
    -device isa-debug-exit,iobase=0xf4,iosize=4 \
    -boot order=d,strict=on -cdrom "$work/scarp.iso" || status=$?
markers="$(cat "$work/boot-markers.log" 2>/dev/null || true)"
printf 'Guest boot markers: %s\n' "${markers:-<none>}"
if [[ "$status" != 33 || "$markers" != BGPEVLS123456789A ]]; then
    echo 'Last QEMU exception/reset log entries:' >&2
    if [[ -f "$work/qemu.log" ]]; then tail -n 80 "$work/qemu.log" >&2; fi
fi
case "$status" in
    33)
        if [[ "$markers" != BGPEVLS123456789A ]]; then
            echo 'FAIL: guest exit code did not match complete boot markers.' >&2
            exit 1
        fi
        echo 'PASS: Scarp-generated checked functions, bounded loops, direct calls and aggregate locals ran in bare-metal x86-64 mode.'
        ;;
    35) echo 'FAIL: boot magic or Scarp ABI/result check failed.' >&2; exit 1 ;;
    124) echo 'FAIL: QEMU timed out before reporting a result.' >&2; exit 1 ;;
    *) printf 'FAIL: unexpected QEMU status %s\n' "$status" >&2; exit 1 ;;
esac

