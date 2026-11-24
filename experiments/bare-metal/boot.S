/* Multiboot2 x86-64 smoke harness; QEMU only, not a general bootloader. */
.section .multiboot,"a",@progbits
.balign 8
header:
    .long 0xe85250d6, 0, header_end - header
    .long -(0xe85250d6 + (header_end - header))
    .short 0, 0
    .long 8
header_end:

.section .text.boot,"ax",@progbits
.code32
.global _start
.extern scarp_checked_integer_function
_start:
    cli
    cld
    cmpl $0x36d76289, %eax
    jne boot_failure
    /* GRUB starts us in 32-bit protected mode with paging disabled. */
    movl $stack_top, %esp
    lgdt gdt_pointer
    movl %cr4, %eax
    orl $0x20, %eax             /* PAE */
    movl %eax, %cr4
    movl $pml4, %eax
    movl %eax, %cr3
    movl $0xc0000080, %ecx      /* EFER */
    rdmsr
    orl $0x100, %eax            /* LME */
    wrmsr
    movl %cr0, %eax
    orl $0x80000000, %eax       /* paging */
    movl %eax, %cr0
    ljmp $0x08, $long_mode
boot_failure:
    movl $0x11, %eax
    movw $0xf4, %dx
    outl %eax, %dx
1:  hlt
    jmp 1b

.code64
long_mode:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    leaq stack_top(%rip), %rsp  /* aligned to 16 bytes before each call */
    xorl %ebp, %ebp
    movl $7, %edi
    movl $16, %esi
    call scarp_checked_integer_function
    cmpq $112, %rax
    jne failed
    testq %rdx, %rdx            /* packed status and span_id */
    jne failed
    movq $-1, %rdi
    movl $16, %esi
    call scarp_checked_integer_function
    cmpq $-1, %rax
    jne failed
    testq %rdx, %rdx
    jne failed
    movabsq $0x7fffffffffffffff, %rdi
    movl $2, %esi
    call scarp_checked_integer_function
    testq %rax, %rax
    jne failed
    cmpl $1, %edx               /* checked integer overflow status */
    jne failed
    movl $0x10, %eax            /* QEMU exit status 33 */
    jmp finish
failed:
    movl $0x11, %eax            /* QEMU exit status 35 */
finish:
    movw $0xf4, %dx
    outl %eax, %dx
2:  hlt
    jmp 2b

.section .rodata,"a",@progbits
.balign 8
gdt:
    .quad 0, 0x00af9a000000ffff, 0x00cf92000000ffff
gdt_end:
gdt_pointer:
    .word gdt_end - gdt - 1
    .long gdt

.section .data,"aw",@progbits
.balign 4096
pml4:
    .quad pdpt + 3
    .zero 4096 - 8
pdpt:
    .quad pd + 3
    .zero 4096 - 8
pd:
    .set page, 0
    .rept 512
    .quad (page * 0x200000) + 0x83
    .set page, page + 1
    .endr

.section .bss,"aw",@nobits
.balign 16
stack_bottom:
    .skip 65536
stack_top:
.section .note.GNU-stack,"",@progbits
