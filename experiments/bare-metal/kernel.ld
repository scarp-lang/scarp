OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)
PHDRS {
    rx PT_LOAD FLAGS(5);
    rw PT_LOAD FLAGS(6);
    stack PT_GNU_STACK FLAGS(6);
}
SECTIONS {
    . = 1M;
    .multiboot : { KEEP(*(.multiboot)) } :rx
    .text ALIGN(16) : { *(.text.boot) *(.text .text.*) } :rx
    .rodata ALIGN(16) : { *(.rodata .rodata.*) } :rx
    .data ALIGN(4096) : { *(.data .data.*) } :rw
    .bss ALIGN(16) (NOLOAD) : { *(.bss .bss.*) *(COMMON) } :rw
    /DISCARD/ : { *(.note.GNU-stack) *(.comment) }
    ASSERT(. < 0x40000000, "kernel outside identity-mapped first GiB")
    ASSERT(SIZEOF(.multiboot) == 40, "unexpected Multiboot2 header size")
}
