#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int fsync(int fd) {
    int (*real_fsync)(int) = dlsym(RTLD_NEXT, "fsync");
    const char *source = getenv("AKASHA_PROBE_SOURCE");
    const char *target = getenv("AKASHA_PROBE_TARGET");
    const char *prefix = getenv("AKASHA_PROBE_PREFIX");
    char bytes[64] = {0};
    if (source && target && prefix && pread(fd, bytes, sizeof(bytes)-1, 0) > 0
        && strncmp(bytes, prefix, strlen(prefix)) == 0) {
        if (rename(source, target) != 0) {
            perror("controlled source move");
            _exit(97);
        }
        unsetenv("AKASHA_PROBE_SOURCE");
        fprintf(stderr, "AKASHA_PROBE_SOURCE_MOVED_BEFORE_METADATA_FSYNC\n");
    }
    return real_fsync(fd);
}
