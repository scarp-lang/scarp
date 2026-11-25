/* Multiboot2 x86-64 smoke harness; QEMU only, not a general bootloader. */
.section .multiboot,"a",@progbits
.balign 8
header:
    .long 0xe85250d6, 0, header_end - header
    .long -(0xe85250d6 + (header_end - header))
    /* Explicit 32-bit physical entry address, even in an ELF64 image. */
    .short 3, 0
    .long 12
    .long _start
    .balign 8
    .short 0, 0
    .long 8
header_end:

.section .text.boot,"ax",@progbits
.code32
/* Debug port is QEMU-specific. The caller must not need AL afterward. */
.macro mark byte
    movb $\byte, %al
    outb %al, $0xe9
.endm
.global _start
.extern scarp_checked_integer_function
.extern scarp_bounded_integer_function
.extern scarp_bounded_integer_entry
_start:
    cli
    cld
    cmpl $0x36d76289, %eax
    jne boot_failure
    mark 'B'
    /* GRUB starts us in 32-bit protected mode with paging disabled. */
    movl $stack_top, %esp
    lgdt gdt_pointer
    mark 'G'
    movl %cr4, %eax
    orl $0x20, %eax             /* PAE */
    movl %eax, %cr4
    movl $pml4, %eax
    movl %eax, %cr3
    mark 'P'
    movl $0xc0000080, %ecx      /* EFER */
    rdmsr
    orl $0x100, %eax            /* LME */
    wrmsr
    mark 'E'
    movl %cr0, %eax
    orl $0x80000000, %eax       /* paging */
    movl %eax, %cr0
    mark 'V'
    ljmp $0x08, $long_mode
boot_failure:
    mark 'X'
    movl $0x11, %eax
    movw $0xf4, %dx
    outl %eax, %dx
1:  hlt
    jmp 1b

.code64
long_mode:
    mark 'L'
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    leaq stack_top(%rip), %rsp  /* aligned to 16 bytes before each call */
    xorl %ebp, %ebp
    mark 'S'
    movl $7, %edi
    movl $16, %esi
    call scarp_checked_integer_function
    cmpq $112, %rax
    jne failed
    testq %rdx, %rdx            /* packed status and span_id */
    jne failed
    mark '1'
    movq $-1, %rdi
    movl $16, %esi
    call scarp_checked_integer_function
    cmpq $-1, %rax
    jne failed
    testq %rdx, %rdx
    jne failed
    mark '2'
    movabsq $0x7fffffffffffffff, %rdi
    movl $2, %esi
    call scarp_checked_integer_function
    testq %rax, %rax
    jne failed
    cmpl $1, %edx               /* checked integer overflow status */
    jne failed
    mark '3'
    /* Exercise native loop control and a per-invocation work allowance. */
    movq %rsp, %r12
    movl $10, %edi
    call scarp_bounded_integer_function
    cmpq %r12, %rsp
    jne failed
    cmpq $55, %rax
    jne failed
    testq %rdx, %rdx
    jne failed
    mark '4'
    movl $1000000, %edi
    call scarp_bounded_integer_function
    cmpq %r12, %rsp
    jne failed
    testq %rax, %rax
    jne failed
    cmpl $4, %edx
    jne failed
    mark '5'
    /* A failed call must not consume the next invocation's allowance. */
    movl $3, %edi
    call scarp_bounded_integer_function
    cmpq %r12, %rsp
    jne failed
    cmpq $6, %rax
    jne failed
    testq %rdx, %rdx
    jne failed
    mark '6'
    /* Call through two private helpers, with stack arguments in the last call. */
    movl $20, %edi
    movl $2, %esi
    call scarp_bounded_integer_entry
    cmpq %r12, %rsp
    jne failed
    cmpq $30, %rax
    jne failed
    testq %rdx, %rdx
    jne failed
    mark '7'
    /* A division failure in the innermost helper must unwind to this caller. */
    movl $20, %edi
    xorl %esi, %esi
    call scarp_bounded_integer_entry
    cmpq %r12, %rsp
    jne failed
    testq %rax, %rax
    jne failed
    cmpl $2, %edx
    jne failed
    mark '8'
    movl $0x10, %eax            /* QEMU exit status 33 */
    jmp finish
failed:
    mark 'F'
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

