// 独立零业务探针：监听就绪后真实交换，所有等待共享六秒截止时间。
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>
static long long deadline;
static long long now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (long long)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}
static int ready(int fd, short events, const char *phase) {
    struct pollfd p = {fd, events, 0};
    long long remaining = deadline - now_ms();
    int rc = remaining > 0 ? poll(&p, 1, (int)remaining) : 0;
    printf("phase=%s poll=%d revents=%d remaining_ms=%lld\n", phase, rc, p.revents, remaining);
    return rc > 0 && (p.revents & events);
}
static int four(int fd, char *bytes, int writing, const char *phase) {
    size_t n = 0;
    while (n < 4) {
        if (!ready(fd, writing ? POLLOUT : POLLIN, phase)) return 0;
        ssize_t rc = writing ? send(fd, bytes + n, 4 - n, 0) : recv(fd, bytes + n, 4 - n, 0);
        printf("phase=%s transferred=%zd errno=%d\n", phase, rc, errno);
        if (rc <= 0) return 0;
        n += (size_t)rc;
    }
    return 1;
}
int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc != 2) return 2;
    deadline = now_ms() + 6000;
    int ls = socket(AF_INET, SOCK_STREAM, 0), client = -1, peer = -1, ok = 0, accepted = 0;
    struct sockaddr_in a = {0};
    a.sin_family = AF_INET;
    a.sin_addr.s_addr = htonl(INADDR_ANY);
    if (ls < 0 || bind(ls, (struct sockaddr *)&a, sizeof(a)) || listen(ls, 4)) {
        perror("bind/listen"); goto done;
    }
    socklen_t len = sizeof(a);
    if (getsockname(ls, (struct sockaddr *)&a, &len)) goto done;
    printf("pid=%d bind=0.0.0.0:%u connect=%s:%u listening=true\n", getpid(), ntohs(a.sin_port), argv[1], ntohs(a.sin_port));
    if (inet_pton(AF_INET, argv[1], &a.sin_addr) != 1) goto done;
    client = socket(AF_INET, SOCK_STREAM, 0);
    if (client < 0 || fcntl(client, F_SETFL, O_NONBLOCK) < 0) goto done;
    int rc = connect(client, (struct sockaddr *)&a, sizeof(a));
    if (rc < 0 && errno != EINPROGRESS) { perror("connect"); goto done; }
    if (!ready(client, POLLOUT, "connect")) goto done;
    int error = 0; len = sizeof(error);
    if (getsockopt(client, SOL_SOCKET, SO_ERROR, &error, &len) || error) goto done;
    printf("client connected\n");
    char ping[4] = {'P','I','N','G'}, pong[4] = {'P','O','N','G'}, bytes[4];
    if (!four(client, ping, 1, "client-write")) goto done;
    if (!ready(ls, POLLIN, "listener-accept")) goto done;
    peer = accept(ls, NULL, NULL);
    if (peer < 0 || fcntl(peer, F_SETFL, O_NONBLOCK) < 0) goto done;
    accepted = 1; printf("server accepted\n");
    if (!four(peer, bytes, 0, "server-recv") || memcmp(bytes, ping, 4)) goto done;
    if (!four(peer, pong, 1, "server-write")) goto done;
    if (!four(client, bytes, 0, "client-recv") || memcmp(bytes, pong, 4)) goto done;
    ok = 1;
done:
    if (peer >= 0) close(peer);
    if (client >= 0) close(client);
    if (ls >= 0) close(ls);
    printf("PROBE RESULT: %s accepted=%d\n", ok ? "ok" : "blocked-or-error", accepted);
    return ok ? 0 : 1;
}
