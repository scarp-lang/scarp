/* Linux test host for the emitted UEFI entry and console calling conventions. */
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

typedef struct Console Console;
typedef uint64_t (__attribute__((ms_abi)) *OutputString)(Console *, const uint16_t *);

struct Console {
    void *reset;
    OutputString output_string;
};

typedef struct {
    uint64_t before_console[8];
    Console *console_out;
} SystemTable;

typedef uint64_t (__attribute__((ms_abi)) *Entry)(void *, SystemTable *);

static unsigned calls;
static int check_text = 1;
static int warn;
static __attribute__((ms_abi, noinline)) uint64_t efi_output(Console *, const uint16_t *);
static Console console = { NULL, efi_output };

static __attribute__((ms_abi, noinline)) uint64_t efi_output(Console *self, const uint16_t *text) {
    const uint16_t expected[] = { 'O', 'K', 13, 10, 0 };
    ++calls;
    if (self != &console) {
        check_text = 0;
    }
    for (unsigned i = 0; i < sizeof(expected) / sizeof(*expected); ++i) {
        if (text[i] != expected[i]) {
            check_text = 0;
            break;
        }
    }
    /* The firmware is entitled to clobber both volatile registers. */
    __asm__ volatile("xor %%r10d, %%r10d; xor %%r11d, %%r11d" ::: "r10", "r11", "cc");
    return warn ? 1 : 0;
}

int main(int argc, char **argv) {
    if (argc != 5) {
        return 1;
    }
    int fd = open(argv[1], O_RDONLY);
    struct stat meta;
    if (fd < 0 || fstat(fd, &meta) != 0) {
        return 2;
    }
    unsigned char *image = mmap(NULL, meta.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    close(fd);
    size_t text_rva = strtoul(argv[2], NULL, 10);
    size_t text_size = strtoul(argv[3], NULL, 10);
    if (image == MAP_FAILED || (uintptr_t)image == 0x400000
        || mprotect(image + text_rva, text_size, PROT_READ | PROT_EXEC) != 0) {
        return 3;
    }
    int mode = atoi(argv[4]);
    warn = mode == 1;
    SystemTable table = { {0}, mode == 2 ? NULL : &console };
    Entry entry = (Entry)(image + text_rva);

    register uint64_t rdi __asm__("rdi") = 456;
    register uint64_t rsi __asm__("rsi") = 789;
    register uint64_t r15 __asm__("r15") = 147;
    __asm__ volatile("" : "+D"(rdi), "+S"(rsi), "+r"(r15));
    uint64_t status = entry(NULL, mode == 3 ? NULL : &table);
    __asm__ volatile("" : "+D"(rdi), "+S"(rsi), "+r"(r15));

    unsigned expected_calls = mode == 0 ? 2 : mode == 1 ? 1 : 0;
    uint64_t expected_status = mode == 0 ? 0 : (UINT64_C(1) << 63) | 21;
    if (status != expected_status || calls != expected_calls || !check_text
        || rdi != 456 || rsi != 789 || r15 != 147) {
        fprintf(stderr, "mode %d returned %#llx and printed %u lines\n",
                mode, (unsigned long long)status, calls);
        return 4;
    }
    return 0;
}
