#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <pty.h>
#include <utmp.h>
#include <fcntl.h>
#include <sys/wait.h>

int main(int argc, char *argv[]) {
    int master, slave;
    if (openpty(&master, &slave, NULL, NULL, NULL) < 0) {
        perror("openpty");
        return 1;
    }

    pid_t pid = fork();
    if (pid < 0) {
        perror("fork");
        return 1;
    }

    if (pid == 0) {
        close(master);
        login_tty(slave);

        const char *hx_bin = getenv("HELIX_BIN");
        if (!hx_bin || hx_bin[0] == '\0') {
            hx_bin = "hx";
        }

        const char *cfg = getenv("HELIX_BENCH_CONFIG");
        if (!cfg || cfg[0] == '\0') {
            cfg = "/tmp/lume-bench-helix-config.toml";
        }

        char **new_argv = malloc((argc + 4) * sizeof(char *));
        if (!new_argv) {
            _exit(1);
        }
        new_argv[0] = (char *)hx_bin;
        new_argv[1] = "-c";
        new_argv[2] = (char *)cfg;
        for (int i = 1; i < argc; i++) {
            new_argv[2 + i] = argv[i];
        }
        new_argv[2 + argc] = NULL;

        execvp(hx_bin, new_argv);
        perror("execvp hx");
        free(new_argv);
        _exit(1);
    }

    close(slave);
    const char *resp = "\x1b[?6cq";
    ssize_t w = write(master, resp, 6);
    (void)w;

    int flags = fcntl(master, F_GETFL, 0);
    fcntl(master, F_SETFL, flags | O_NONBLOCK);

    char buf[4096];
    int status = 0;
    while (waitpid(pid, &status, WNOHANG) == 0) {
        while (read(master, buf, sizeof(buf)) > 0);
        usleep(500);
    }
    close(master);
    return WIFEXITED(status) ? WEXITSTATUS(status) : 1;
}
