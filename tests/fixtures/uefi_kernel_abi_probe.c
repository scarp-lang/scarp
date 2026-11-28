/* Test firmware ABI and retry logic without entering a privileged kernel. */
#define _GNU_SOURCE
#include <fcntl.h>
#include <signal.h>
#include <stdint.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <ucontext.h>
#include <unistd.h>

#define EFI_ERROR (UINT64_C(1) << 63)
#define EFI_INVALID_PARAMETER (EFI_ERROR | 2)
#define EFI_BUFFER_TOO_SMALL (EFI_ERROR | 5)
#define EFI_ABORTED (EFI_ERROR | 21)
#define IMAGE_HANDLE ((void *)0x12340)

typedef uint64_t (__attribute__((ms_abi)) *GetMap)(
    uint64_t *, void *, uint64_t *, uint64_t *, uint32_t *);
typedef uint64_t (__attribute__((ms_abi)) *ExitBoot)(void *, uint64_t);
typedef struct {
    uint8_t before_map[56];
    GetMap get_map;
    uint8_t before_exit[168];
    ExitBoot exit_boot;
} BootServices;

typedef struct {
    uint8_t before_services[96];
    BootServices *services;
} SystemTable;

typedef uint64_t (__attribute__((ms_abi)) *Entry)(void *, SystemTable *);

static volatile sig_atomic_t mode;
static volatile sig_atomic_t map_calls;
static volatile sig_atomic_t exit_calls;
static const uint8_t *text;
static size_t text_length;

static __attribute__((ms_abi)) uint64_t get_map(
    uint64_t *size, void *map, uint64_t *key, uint64_t *stride, uint32_t *version) {
    ++map_calls;
    if (*size != 128 * 1024 || !map || !key || !stride || !version) {
        _exit(10);
    }
    if (mode == 1 || (mode == 5 && map_calls == 2)) {
        *size += 4096;
        return EFI_BUFFER_TOO_SMALL;
    }
    *key = 0x100u + (unsigned)map_calls;
    *version = 1;
    *stride = mode == 0 ? 24 : 48;
    *size = mode == 6 ? *stride + 1 : *stride;
    ((uint8_t *)map)[0] = 7;
    return 0;
}

static __attribute__((ms_abi)) uint64_t exit_boot(void *image, uint64_t key) {
    ++exit_calls;
    if (image != IMAGE_HANDLE || key != 0x100u + (unsigned)map_calls || map_calls != exit_calls) {
        _exit(11);
    }
    if (mode == 4) {
        return EFI_ABORTED;
    }
    return mode == 3 && exit_calls == 2 ? 0 : EFI_INVALID_PARAMETER;
}

static void at_privileged_instruction(int signal, siginfo_t *info, void *context) {
    (void)signal;
    (void)info;
    const ucontext_t *cpu = context;
    uintptr_t pc = (uintptr_t)cpu->uc_mcontext.gregs[REG_RIP];
    if (pc < (uintptr_t)text || pc >= (uintptr_t)text + text_length - 1) {
        _exit(12);
    }
    const uint8_t *instruction = (const uint8_t *)pc;
    if (instruction[0] != 0xfa) {
        _exit(13);
    }
    if (mode == 3) {
        /* Successful ExitBootServices enters CLI followed by CLD. */
        _exit(instruction[1] == 0xfc && map_calls == 2 && exit_calls == 2 ? 0 : 14);
    }
    /* A failed post-exit path reaches the terminal CLI/HLT loop. */
    int expected_maps = mode == 2 ? 3 : mode == 5 ? 2 : 1;
    int expected_exits = mode == 2 ? 3 : 1;
    int can_halt = mode == 2 || mode == 4 || mode == 5;
    _exit(can_halt && instruction[1] == 0xf4
          && map_calls == expected_maps && exit_calls == expected_exits ? 0 : 15);
}

int main(int argc, char **argv) {
    if (argc != 7) {
        return 1;
    }
    int fd = open(argv[1], O_RDONLY);
    struct stat meta;
    if (fd < 0 || fstat(fd, &meta) != 0) {
        return 2;
    }
    uint8_t *image = mmap(NULL, meta.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    close(fd);
    size_t text_rva = strtoul(argv[2], NULL, 10);
    text_length = strtoul(argv[3], NULL, 10);
    size_t bss_rva = strtoul(argv[4], NULL, 10);
    size_t bss_length = strtoul(argv[5], NULL, 10);
    mode = (sig_atomic_t)atoi(argv[6]);
    if (image == MAP_FAILED || (uintptr_t)image == 0x400000
        || mprotect(image + text_rva, text_length, PROT_READ | PROT_EXEC) != 0
        || mprotect(image + bss_rva, bss_length, PROT_READ | PROT_WRITE) != 0) {
        return 3;
    }
    text = image + text_rva;
    struct sigaction handler = { 0 };
    handler.sa_sigaction = at_privileged_instruction;
    handler.sa_flags = SA_SIGINFO;
    sigemptyset(&handler.sa_mask);
    if (sigaction(SIGSEGV, &handler, NULL) != 0) {
        return 4;
    }
    BootServices boot = { {0}, get_map, {0}, exit_boot };
    SystemTable system = { {0}, &boot };
    Entry entry = (Entry)(image + text_rva);
    uint64_t status = entry(IMAGE_HANDLE, &system);
    return (mode == 0 || mode == 1 || mode == 6)
        && status == EFI_ABORTED && map_calls == 1 && exit_calls == 0 ? 0 : 5;
}
